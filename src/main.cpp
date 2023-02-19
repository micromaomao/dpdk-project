/* Part of this file has been adopted from the DPDK sample app "skeleton",
 * licensed under BSD-3-Clause, copyrighted by Intel Corporation */

extern "C" {
#include <inttypes.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_ether.h>
#include <rte_lcore.h>
#include <rte_mbuf.h>
#include <stdint.h>
#include <stdlib.h>
}

#include <algorithm>
#include <vector>

#include "bindings.h"

static const int RX_RING_SIZE = 1024;
static const int TX_RING_SIZE = 1024;
static const int NUM_MBUFS = 8191;
static const int MBUF_CACHE_SIZE = 250;
static const int BURST_SIZE = 32;

struct port_context;
struct lcore_context {
  const port_context *port_ctx;
  struct rte_mempool *mbuf_pool;
  unsigned pool_size;
  unsigned rte_lcore_id;
  int rx_qid, tx_qid;

  lcore_context(const port_context *port_ctx, struct rte_mempool *mbuf_pool,
                unsigned pool_size, unsigned rte_lcore_id)
      : port_ctx(port_ctx), mbuf_pool(mbuf_pool), pool_size(pool_size),
        rte_lcore_id(rte_lcore_id), rx_qid(-1), tx_qid(-1) {}
};

static __rte_noreturn int lcore_main_reflect(void *arg) {
  lcore_context *lcore_ctx = (lcore_context *)arg;
  uint16_t port = lcore_ctx->rte_lcore_id;

  while (true) {
    /* Get burst of RX packets */
    struct rte_mbuf *bufs[BURST_SIZE];
    const uint16_t nb_rx = rte_eth_rx_burst(port, 0, bufs, BURST_SIZE);

    if (unlikely(nb_rx == 0))
      continue;

    /* Send burst of TX packets */
    const uint16_t nb_tx = rte_eth_tx_burst(port, 0, bufs, nb_rx);

    /* Free any unsent packets. */
    if (unlikely(nb_tx < nb_rx)) {
      uint16_t buf;
      for (buf = nb_tx; buf < nb_rx; buf++)
        rte_pktmbuf_free(bufs[buf]);
    }
  }
}

/**
 * A context object for a specific port, which will manage resources like
 * mbuf_pool for individual cores, which core are assigned to which port, etc.
 */
struct port_context {
  DPRunMode mode;
  uint16_t rte_port_id;
  uint32_t txq, rxq;
  bool rte_port_started;
  struct rte_ether_addr mac_addr;
  struct rte_eth_dev_info dev_info;
  std::vector<lcore_context> lcore_contexts;

  bool ip4_checksum_offload;
  bool udp_checksum_offload;

  port_context(DPRunMode mode, uint16_t rte_port_id, uint32_t txq, uint32_t rxq)
      : mode(mode), rte_port_id(rte_port_id), txq(txq), rxq(rxq),
        rte_port_started(false), ip4_checksum_offload(false) {
    if (rte_eth_macaddr_get(rte_port_id, &mac_addr) != 0) {
      rte_exit(EXIT_FAILURE, "Cannot get MAC address for port %u\n",
               rte_port_id);
    }

    int port_socket_id = rte_eth_dev_socket_id(rte_port_id);

    int ret = rte_eth_dev_info_get(rte_port_id, &dev_info);
    if (ret != 0) {
      rte_exit(EXIT_FAILURE, "Error getting info for port %u: %s\n",
               rte_port_id, strerror(-ret));
    }

    if (dev_info.max_tx_queues < txq) {
      rte_exit(EXIT_FAILURE, "Port %u support a maximum of %u tx queues\n",
               rte_port_id, dev_info.max_tx_queues);
    }
    if (dev_info.max_rx_queues < rxq) {
      rte_exit(EXIT_FAILURE, "Port %u support a maximum of %u rx queues\n",
               rte_port_id, dev_info.max_rx_queues);
    }
  }

  ~port_context() {
    if (rte_port_started) {
      if (rte_eth_dev_stop(rte_port_id)) {
        fprintf(stderr, "Failed to stop rte port %u\n", rte_port_id);
      }
    }
    for (auto &lcore_ctx : lcore_contexts) {
      if (lcore_ctx.mbuf_pool) {
        rte_mempool_free(lcore_ctx.mbuf_pool);
        lcore_ctx.mbuf_pool = nullptr;
      }
    }
  }

  void assign_lcores(const std::vector<unsigned> &lcores) {
    int port_socket_id = rte_eth_dev_socket_id(rte_port_id);

    if (port_socket_id <= 0) {
      fprintf(stderr, "WARNING: port %u socket id could not be determined.\n",
              rte_port_id);
    }

    for (unsigned lcore : lcores) {
      int lcore_socket = rte_lcore_to_socket_id(lcore);
      if (lcore_socket != port_socket_id) {
        fprintf(stderr,
                "WARNING: port %u is on remote NUMA node to "
                "lcore %u.\n",
                rte_port_id, lcore);
      }
      char name[RTE_MEMPOOL_NAMESIZE];
      snprintf(name, sizeof(name), "mbuf_pool_lc%u", lcore);
      unsigned pool_size = NUM_MBUFS;
      rte_mempool *mbuf_pool =
          rte_pktmbuf_pool_create(name, pool_size, MBUF_CACHE_SIZE, 0,
                                  RTE_MBUF_DEFAULT_BUF_SIZE, lcore_socket);
      if (!mbuf_pool) {
        rte_exit(EXIT_FAILURE, "Cannot create mbuf pool for lcore %u\n", lcore);
      }

      lcore_contexts.emplace_back(this, mbuf_pool, pool_size, lcore);
    }
  }

  bool config_port() {
    assert(!rte_port_started);
    struct rte_eth_conf port_conf;

    memset(&port_conf, 0, sizeof(struct rte_eth_conf));

    if (dev_info.tx_offload_capa & RTE_ETH_TX_OFFLOAD_MBUF_FAST_FREE) {
      port_conf.txmode.offloads |= RTE_ETH_TX_OFFLOAD_MBUF_FAST_FREE;
    }
    if (dev_info.tx_offload_capa & RTE_ETH_TX_OFFLOAD_IPV4_CKSUM) {
      port_conf.txmode.offloads |= RTE_ETH_TX_OFFLOAD_IPV4_CKSUM;
      ip4_checksum_offload = true;
    }
    if (dev_info.tx_offload_capa & RTE_ETH_TX_OFFLOAD_UDP_CKSUM) {
      port_conf.txmode.offloads |= RTE_ETH_TX_OFFLOAD_UDP_CKSUM;
      udp_checksum_offload = true;
    }

    struct rte_eth_txconf txconf = dev_info.default_txconf;
    txconf.offloads = port_conf.txmode.offloads;

    int retval = rte_eth_dev_configure(rte_port_id, rxq, txq, &port_conf);
    if (retval != 0)
      return retval;

    int rxq_idx = 0, txq_idx = 0;

    for (auto &lcore_ctx : lcore_contexts) {
      uint16_t nb_rxd = RX_RING_SIZE;
      uint16_t nb_txd = TX_RING_SIZE;
      retval = rte_eth_dev_adjust_nb_rx_tx_desc(rte_port_id, &nb_rxd, &nb_txd);
      if (retval != 0) {
        fprintf(
            stderr,
            "Failed to call rte_eth_dev_adjust_nb_rx_tx_desc(port=%d): %s\n",
            rte_port_id, strerror(-retval));
        return false;
      }

      // TODO: check this->mode

      retval =
          rte_eth_rx_queue_setup(rte_port_id, rxq_idx, nb_rxd,
                                 rte_lcore_to_socket_id(lcore_ctx.rte_lcore_id),
                                 NULL, lcore_ctx.mbuf_pool);
      if (retval < 0) {
        fprintf(
            stderr,
            "Failed to call rte_eth_rx_queue_setup(port=%d, queue=%d): %s\n",
            rte_port_id, rxq_idx, strerror(-retval));
        return false;
      }
      lcore_ctx.rx_qid = rxq_idx;
      rxq_idx += 1;

      retval = rte_eth_tx_queue_setup(
          rte_port_id, txq_idx, nb_txd,
          rte_lcore_to_socket_id(lcore_ctx.rte_lcore_id), &txconf);
      if (retval < 0) {
        fprintf(
            stderr,
            "Failed to call rte_eth_tx_queue_setup(port=%d, queue=%d): %s\n",
            rte_port_id, txq_idx, strerror(-retval));
        return false;
      }
      lcore_ctx.tx_qid = txq_idx;
      txq_idx += 1;
    }

    retval = rte_eth_dev_start(rte_port_id);
    if (retval < 0) {
      fprintf(stderr, "Failed to start port %d: %s\n", rte_port_id,
              strerror(-retval));
      return false;
    }

    rte_port_started = true;

    // rte_eth_promiscuous_enable(rte_port_id);

    return true;
  }

  bool start() {
    if (mode == DPRunMode::Reflect) {
      for (auto &lcore_ctx : lcore_contexts) {
        assert(lcore_ctx.rx_qid != -1 && lcore_ctx.tx_qid != -1);
        if (lcore_ctx.rte_lcore_id == rte_lcore_id()) {
          lcore_main_reflect(&lcore_ctx);
        } else {
          rte_eal_remote_launch(&lcore_main_reflect, &lcore_ctx,
                                lcore_ctx.rte_lcore_id);
        }
      }
      return true;
    } else if (mode == DPRunMode::SendRecv) {
      fprintf(stderr, "Unimplemented\n");
      return false;
    } else {
      assert(false);
      return false;
    }
  }
};

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

  int expected_nb_cores =
      nb_ports * (parsed_args->nb_rxq + parsed_args->nb_txq);

  // In reflect mode, one core uses both the rx and tx queue.
  if (parsed_args->mode == DPRunMode::Reflect) {
    if (parsed_args->nb_rxq != parsed_args->nb_txq) {
      fprintf(stderr,
              "In reflect mode, the number of rx queues must be equal to the "
              "number of tx queues.\n");
      exit(1);
    }
    expected_nb_cores = nb_ports * parsed_args->nb_rxq;
  }

  int nb_cores = rte_lcore_count();
  if (expected_nb_cores != nb_cores) {
    fprintf(stderr,
            "Number of lcores (set to %d) must be equal to %d. Use the -l EAL "
            "argument to set the set of lcores.\n",
            nb_cores, expected_nb_cores);
    exit(1);
  }

  std::vector<port_context> contexts;

  for (int i = 0; i < nb_ports; i++) {
    contexts.emplace_back(parsed_args->mode, rte_ports_matched[i],
                          parsed_args->nb_txq, parsed_args->nb_rxq);
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
