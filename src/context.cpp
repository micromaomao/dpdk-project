#include "context.hpp"
#include "consts.hpp"
#include "main.hpp"

lcore_context::lcore_context(const port_context *port_ctx,
                             struct rte_mempool *mbuf_pool, unsigned pool_size,
                             unsigned rte_lcore_id)
    : port_ctx(port_ctx), mbuf_pool(mbuf_pool), pool_size(pool_size),
      rte_lcore_id(rte_lcore_id), rx_qid(-1), tx_qid(-1) {}

port_context::port_context(DPRunMode mode, uint16_t rte_port_id, uint32_t txq,
                           uint32_t rxq)
    : mode(mode), rte_port_id(rte_port_id), txq(txq), rxq(rxq),
      rte_port_started(false), ip4_checksum_offload(false) {
  if (rte_eth_macaddr_get(rte_port_id, &mac_addr) != 0) {
    rte_exit(EXIT_FAILURE, "Cannot get MAC address for port %u\n", rte_port_id);
  }

  int port_socket_id = rte_eth_dev_socket_id(rte_port_id);

  int ret = rte_eth_dev_info_get(rte_port_id, &dev_info);
  if (ret != 0) {
    rte_exit(EXIT_FAILURE, "Error getting info for port %u: %s\n", rte_port_id,
             strerror(-ret));
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

port_context::~port_context() {
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

void port_context::assign_lcores(const std::vector<unsigned> &lcores) {
  int port_socket_id = rte_eth_dev_socket_id(rte_port_id);

  if (port_socket_id < 0) {
    fprintf(stderr, "WARNING: port %u socket id could not be determined.\n",
            rte_port_id);
    port_socket_id = SOCKET_ID_ANY;
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

bool port_context::config_port() {
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
      fprintf(stderr,
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
      fprintf(stderr,
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
      fprintf(stderr,
              "Failed to call rte_eth_tx_queue_setup(port=%d, queue=%d): %s\n",
              rte_port_id, txq_idx, strerror(-retval));
      return false;
    }
    lcore_ctx.tx_qid = txq_idx;
    txq_idx += 1;
  }

  printf("Port %u: set up %d rx queues and %d tx queues\n", rte_port_id,
         rxq_idx, txq_idx);

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

bool launch_on(int lcore, lcore_function_t *func, void *arg) {
  if (lcore == rte_lcore_id()) {
    fprintf(stderr, "ERROR: trying to launch function on main lcore.\n");
    return false;
  } else {
    int res = rte_eal_remote_launch(func, arg, lcore);
    if (res != 0) {
      fprintf(stderr, "Failed to launch on lcore %d: %s\n", lcore,
              strerror(-res));
      return false;
    }
    return true;
  }
}

bool port_context::start() {
  bool success = true;

  if (mode == DPRunMode::Reflect) {
    for (auto &lcore_ctx : lcore_contexts) {
      assert(lcore_ctx.rx_qid != -1 && lcore_ctx.tx_qid != -1);
      if (!launch_on(lcore_ctx.rte_lcore_id, &lcore_main_reflect, &lcore_ctx)) {
        success = false;
      }
    }
  } else if (mode == DPRunMode::SendRecv) {
    fprintf(stderr, "Unimplemented\n");
    success = false;
  } else {
    assert(false);
    success = false;
  }

  return success;
}
