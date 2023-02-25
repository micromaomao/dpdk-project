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
  StatsAggregator *stats = lcore_ctx->port_ctx->stats_aggregator;
  RustInstant *start_time = lcore_ctx->port_ctx->start_time;

  printf("Core %u handling packets for port %u, rx queue %d, tx queue %d\n",
         lcore_ctx->rte_lcore_id, port, lcore_ctx->rx_qid, lcore_ctx->tx_qid);

  if (!lcore_ctx->port_ctx->rte_port_started) {
    fprintf(stderr, "Core %u: Port %u not started\n", lcore_ctx->rte_lcore_id,
            port);
  }
  assert(lcore_ctx->port_ctx->rte_port_started);

  const uint32_t max_pkt_size = RTE_MBUF_DEFAULT_BUF_SIZE;
  // To be used to concatenate the segments, if there are more than one.
  char linear_buf[max_pkt_size];

  bool need_ip_checksum = !lcore_ctx->port_ctx->ip4_checksum_offload;
  bool need_udp_checksum = !lcore_ctx->port_ctx->udp_checksum_offload;

  while (true) {
    // In this bufs array, a null pointer indicates that we want to drop the
    // packet.
    struct rte_mbuf *bufs[BURST_SIZE] = {0};

    const uint16_t nb_rx =
        rte_eth_rx_burst(port, lcore_ctx->rx_qid, bufs, BURST_SIZE);
    uint16_t nb_tx = 0;

    if (nb_rx == 0) {
      continue;
    }

    bool dropped_mbuf = false;

    uint64_t time_val = dp_get_time_value_since(start_time);

    for (int i = 0; i < nb_rx; i++) {
      struct rte_mbuf *&m = bufs[i];
      uint32_t len = rte_pktmbuf_pkt_len(m);
      assert(len <= max_pkt_size);
      int linearize_ret = rte_pktmbuf_linearize(m);
      uint8_t *data;

      if (linearize_ret != 0) {
        // Linearize failed.

        // For simplicity, we require all packets to have one segment before
        // being processed. Therefore, we throw away this mbuf and create a new
        // one if we cannot linearize it.
        const char *ptr = (const char *)rte_pktmbuf_read(m, 0, len, linear_buf);
        assert(ptr == linear_buf);
        rte_pktmbuf_free(m);
        m = rte_pktmbuf_alloc(lcore_ctx->mbuf_pool);
        if (!m) {
          dropped_mbuf = true;
          continue;
        }
        data = (uint8_t *)rte_pktmbuf_append(m, len);
        if (!data) {
          rte_pktmbuf_free(m);
          m = nullptr;
          dropped_mbuf = true;
          continue;
        }
        memcpy(data, linear_buf, len);
      } else {
        data = rte_pktmbuf_mtod(m, uint8_t *);
      }

      // Rust time!
      if (!dp_process_reflect_pkt(data, len, need_ip_checksum,
                                  need_udp_checksum)) {
        rte_pktmbuf_free(m);
        m = nullptr;
        dropped_mbuf = true;
      }
    }

    if (dropped_mbuf) {
      for (int i = 0; i < nb_rx; i++) {
        if (bufs[i]) {
          if (rte_eth_tx_burst(port, lcore_ctx->tx_qid, &bufs[i], 1) == 0) {
            rte_pktmbuf_free(bufs[i]);
          } else {
            nb_tx += 1;
          }
        }
      }
    } else {
      nb_tx = rte_eth_tx_burst(port, lcore_ctx->tx_qid, bufs, nb_rx);

      // Free any unsent packets
      for (int i = nb_tx; i < nb_rx; i++) {
        rte_pktmbuf_free(bufs[i]);
      }
    }

    dp_stats_add(stats, time_val, nb_tx, nb_rx, 0, 0);
  }
}

__rte_noreturn int lcore_main_send(void *arg) {
  lcore_context *lcore_ctx = (lcore_context *)arg;
  uint16_t port = lcore_ctx->port_ctx->rte_port_id;
  StatsAggregator *stats = lcore_ctx->port_ctx->stats_aggregator;
  RustInstant *start_time = lcore_ctx->port_ctx->start_time;

  printf("Core %u sending packets for port %u on tx queue %d\n",
         lcore_ctx->rte_lcore_id, port, lcore_ctx->tx_qid);

  if (!lcore_ctx->port_ctx->rte_port_started) {
    fprintf(stderr, "Core %u: Port %u not started\n", lcore_ctx->rte_lcore_id,
            port);
  }
  assert(lcore_ctx->port_ctx->rte_port_started);

  int buf_size = lcore_ctx->port_ctx->cli_args->packet_size + 50;

  DpMakePacketArgs mk_args;

  memcpy(&mk_args.src_mac, &lcore_ctx->port_ctx->mac_addr.addr_bytes, 6);
  mk_args.send_config = lcore_ctx->port_ctx->send_config;
  mk_args.need_ip_checksum = !lcore_ctx->port_ctx->ip4_checksum_offload;
  mk_args.need_udp_checksum = !lcore_ctx->port_ctx->udp_checksum_offload;

  auto &idx_atomic = lcore_ctx->port_ctx->tx_idx;

  while (true) {
    struct rte_mbuf *bufs[BURST_SIZE] = {0};
    int nb_pkts = 0;

    uint64_t time_val = dp_get_time_value_since(start_time);

    for (int i = 0; i < BURST_SIZE; i += 1) {
      bufs[i] = rte_pktmbuf_alloc(lcore_ctx->mbuf_pool);
      if (!bufs[i]) {
        break;
      }
      if (!rte_pktmbuf_append(bufs[i], buf_size)) {
        rte_pktmbuf_free(bufs[i]);
        break;
      }
      nb_pkts += 1;
    }

    uint64_t idx_start =
        idx_atomic.fetch_add(nb_pkts, std::memory_order_relaxed);

    for (int i = 0; i < nb_pkts; i += 1) {
      uint8_t *data = rte_pktmbuf_mtod(bufs[i], uint8_t *);
      mk_args.index = idx_start++;
      mk_args.timestamp = time_val;
      size_t written_len = dp_make_packet(data, buf_size, &mk_args);
      assert(written_len <= buf_size);
      rte_pktmbuf_data_len(bufs[i]) = written_len;
    }

    int nb_tx = rte_eth_tx_burst(port, lcore_ctx->tx_qid, bufs, nb_pkts);

    // Free any unsent packets
    for (int i = nb_tx; i < nb_pkts; i++) {
      rte_pktmbuf_free(bufs[i]);
    }

    dp_stats_add(stats, time_val, nb_tx, 0, 0, 0);
  }
}

__rte_noreturn int lcore_main_recv(void *arg) {
  lcore_context *lcore_ctx = (lcore_context *)arg;
  uint16_t port = lcore_ctx->port_ctx->rte_port_id;
  StatsAggregator *stats = lcore_ctx->port_ctx->stats_aggregator;
  RustInstant *start_time = lcore_ctx->port_ctx->start_time;

  printf("Core %u receiving packets for port %u on rx queue %d\n",
         lcore_ctx->rte_lcore_id, port, lcore_ctx->rx_qid);

  if (!lcore_ctx->port_ctx->rte_port_started) {
    fprintf(stderr, "Core %u: Port %u not started\n", lcore_ctx->rte_lcore_id,
            port);
  }
  assert(lcore_ctx->port_ctx->rte_port_started);

  while (true) {
    struct rte_mbuf *bufs[BURST_SIZE] = {0};
    int nb_rx = 0;

    uint64_t time_val = dp_get_time_value_since(start_time);

    nb_rx = rte_eth_rx_burst(port, lcore_ctx->rx_qid, bufs, BURST_SIZE);

    if (nb_rx == 0) {
      continue;
    }

    for (int i = 0; i < nb_rx; i++) {
      // TODO: parse packet
      rte_pktmbuf_free(bufs[i]);
    }

    dp_stats_add(stats, time_val, 0, nb_rx, 0, 0);
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
  StatsAggregator *stats_agg = parsed_args->stats;
  RustInstant *start_time = dp_get_reference_time();

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
                          parsed_args->nb_txq, parsed_args->nb_rxq, stats_agg,
                          start_time, parsed_args->send_config, parsed_args);
    lcore_assignment.clear();
    for (int j = 0; j < cores_per_port; j++) {
      lcore_assignment.push_back(available_lcores.at(next_lcore_in_list++));
    }
    contexts.back().assign_lcores(lcore_assignment);
  }

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

  dp_free_reference_time(start_time);
  stats_agg = nullptr;

  dp_free_args(parsed_args);
  parsed_args = nullptr;

  rte_eal_cleanup();

  return 0;
}
