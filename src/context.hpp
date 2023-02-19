#pragma once

extern "C" {
#include <rte_ethdev.h>
#include <rte_ether.h>
}

#include "bindings.h"
#include <vector>

struct port_context;
struct lcore_context {
  const port_context *port_ctx;
  struct rte_mempool *mbuf_pool;
  unsigned pool_size;
  unsigned rte_lcore_id;
  int rx_qid, tx_qid;

  lcore_context(const port_context *port_ctx, struct rte_mempool *mbuf_pool,
                unsigned pool_size, unsigned rte_lcore_id);
};

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

  port_context(DPRunMode mode, uint16_t rte_port_id, uint32_t txq,
               uint32_t rxq);
  ~port_context();
  void assign_lcores(const std::vector<unsigned> &lcores);
  bool config_port();
  bool start();
};
