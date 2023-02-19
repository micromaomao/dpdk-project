/* Part of this file has been adopted from the DPDK sample app "skeleton",
 * licensed under BSD-3-Clause, copyrighted by Intel Corporation */

#include <algorithm>
#include <vector>

#include "bindings.h"
#include "consts.hpp"
#include "context.hpp"
#include "main.hpp"

__rte_noreturn int lcore_main_reflect(void *arg) {
  lcore_context *lcore_ctx = (lcore_context *)arg;
  uint16_t port = lcore_ctx->port_ctx->rte_port_id;
  assert(lcore_ctx->port_ctx->rte_port_started);

  printf("Core %u handling packets for port %u, rx queue %d, tx queue %d\n",
         lcore_ctx->rte_lcore_id, port, lcore_ctx->rx_qid, lcore_ctx->tx_qid);

  while (true) {
    /* Get burst of RX packets */
    struct rte_mbuf *bufs[BURST_SIZE];
    const uint16_t nb_rx =
        rte_eth_rx_burst(port, lcore_ctx->rx_qid, bufs, BURST_SIZE);

    if (unlikely(nb_rx == 0))
      continue;

    /* Send burst of TX packets */
    const uint16_t nb_tx =
        rte_eth_tx_burst(port, lcore_ctx->tx_qid, bufs, nb_rx);

    /* Free any unsent packets. */
    if (unlikely(nb_tx < nb_rx)) {
      uint16_t buf;
      for (buf = nb_tx; buf < nb_rx; buf++)
        rte_pktmbuf_free(bufs[buf]);
    }
  }
}

/*
 * The main function, which does initialization and calls the per-lcore
 * functions.
 */
int main(int argc, char *argv[]) {
  /* Initializion the Environment Abstraction Layer (EAL). */
  int ret = rte_eal_init(argc, argv);
  if (ret < 0)
    rte_exit(EXIT_FAILURE, "Error with EAL initialization\n");

  // Note that ret_eal_init will modify argv such that what was originally
  // argv[0] will now be in argv[ret], effectively making it look as if the
  // EAL arguments were not specified in the first place.
  argc -= ret;
  argv += ret;
  auto parsed_args = dp_parse_args(argc, argv);

  // For each MAC specified, we find the rte port id that matches it, and do
  // some basic setup.

  // Stores a mapping from index in parsed_args->ports to the rte port id, or
  // if parsed_args->ports is empty, a list of rte port ids we will use later.
  std::vector<int> rte_ports_matched;

  for (int i = 0; i < parsed_args->nb_ports; i++) {
    struct rte_ether_addr expected_addr;
    memcpy(&expected_addr.addr_bytes, &parsed_args->ports[i], 6);
    bool found = false;

    uint16_t portid;
    RTE_ETH_FOREACH_DEV(portid) {
      if (!rte_eth_dev_is_valid_port(portid)) {
        continue;
      }

      struct rte_ether_addr addr;
      if (rte_eth_macaddr_get(portid, &addr) != 0) {
        rte_exit(EXIT_FAILURE, "Failed to get MAC address for port %u\n",
                 portid);
      }
      if (rte_is_same_ether_addr(&expected_addr, &addr)) {
        if (std::find(rte_ports_matched.begin(), rte_ports_matched.end(),
                      portid) != rte_ports_matched.end()) {
          printf("WARNING: same MAC address on two ports - ignoring port %u",
                 portid);
          continue;
        }
        assert(rte_ports_matched.size() == i);
        rte_ports_matched.push_back(portid);
        found = true;
        break;
      }
    }

    if (!found) {
      fprintf(stderr,
              "Failed to find port with MAC address %02x:%02x:"
              "%02x:%02x:%02x:%02x",
              expected_addr.addr_bytes[0], expected_addr.addr_bytes[1],
              expected_addr.addr_bytes[2], expected_addr.addr_bytes[3],
              expected_addr.addr_bytes[4], expected_addr.addr_bytes[5]);
      exit(1);
    }
  }

  if (parsed_args->nb_ports == 0) {
    uint16_t portid;
    RTE_ETH_FOREACH_DEV(portid) { rte_ports_matched.push_back(portid); }
  }

  if (rte_ports_matched.empty()) {
    rte_exit(EXIT_FAILURE, "No ports to use.\n");
  }

  int nb_ports = rte_ports_matched.size();
  if (nb_ports == 0) {
    rte_exit(EXIT_FAILURE, "No ports to use.\n");
  }

  printf("Found %d ports to use.\n", nb_ports);

  int cores_per_port = parsed_args->nb_rxq + parsed_args->nb_txq;
  if (parsed_args->mode == DPRunMode::Reflect) {
    if (parsed_args->nb_rxq != parsed_args->nb_txq) {
      fprintf(stderr,
              "In reflect mode, the number of rx queues must be equal to the "
              "number of tx queues.\n");
      exit(1);
    }
    // In reflect mode, one core uses both the rx and tx queue.
    cores_per_port = parsed_args->nb_rxq;
  }

  int expected_nb_cores = nb_ports * cores_per_port;

  int nb_cores = rte_lcore_count();
  if (expected_nb_cores != nb_cores) {
    fprintf(stderr,
            "Number of lcores (set to %d) must be equal to %d. Use the -l EAL "
            "argument to set the set of lcores.\n",
            nb_cores, expected_nb_cores);
    exit(1);
  }

  std::vector<port_context> contexts;

  // Make sure we launch on main last.

  std::vector<unsigned> available_lcores;
  unsigned lcore_id;
  RTE_LCORE_FOREACH(lcore_id) {
    if (lcore_id != rte_lcore_id()) {
      available_lcores.push_back(lcore_id);
    }
  }
  available_lcores.push_back(rte_lcore_id());
  int next_lcore_in_list = 0;

  std::vector<unsigned> lcore_assignment;

  for (int i = 0; i < nb_ports; i++) {
    contexts.emplace_back(parsed_args->mode, rte_ports_matched[i],
                          parsed_args->nb_txq, parsed_args->nb_rxq);
    lcore_assignment.clear();
    for (int j = 0; j < cores_per_port; j++) {
      lcore_assignment.push_back(available_lcores.at(next_lcore_in_list++));
    }
    contexts.back().assign_lcores(lcore_assignment);
  }

  dp_free_args(parsed_args);
  parsed_args = nullptr;

  bool success = true;

  for (auto &ctx : contexts) {
    if (!ctx.config_port()) {
      fprintf(stderr, "Failed to setup port %d\n", ctx.rte_port_id);
      success = false;
      break;
    }
  }

  if (success) {
    for (auto &ctx : contexts) {
      if (!ctx.start()) {
        fprintf(stderr, "Failed to start port %d\n", ctx.rte_port_id);
      }
    }
  }

  rte_eal_mp_wait_lcore();

  // Run context deallocators, which may free rte resources.
  contexts.clear();

  rte_eal_cleanup();

  return 0;
}
