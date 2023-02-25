/**
 * Does some boring work, like initializing ports, creating queues and mbuf
 * pools, etc
 */
#pragma once

extern "C" {
#include <rte_ethdev.h>
#include <rte_ether.h>
}

#include "bindings.h"
#include <atomic>
#include <vector>

struct port_context;
struct lcore_context {
  port_context *port_ctx;
  struct rte_mempool *mbuf_pool;
  unsigned pool_size;
  unsigned rte_lcore_id;
  int rx_qid, tx_qid;

  lcore_context(struct rte_mempool *mbuf_pool, unsigned pool_size,
                unsigned rte_lcore_id);

  bool is_send() const { return this->tx_qid != -1; }
  bool is_recv() const { return this->rx_qid != -1; }
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
  StatsAggregator *stats_aggregator;
  RustInstant *start_time;
  SendConfig *send_config;
  DPCmdArgs *cli_args;

  bool ip4_checksum_offload;
  bool udp_checksum_offload;

  std::atomic_int64_t tx_idx;

  port_context(DPRunMode mode, uint16_t rte_port_id, uint32_t txq, uint32_t rxq,
               StatsAggregator *stats_aggregator, RustInstant *start_time,
               SendConfig *send_config, DPCmdArgs *cli_args);

  port_context(const port_context &) = delete;
  port_context &operator=(const port_context &) = delete;

  /// Needed because std::atomic is not movable
  port_context(port_context &&other)
      : mode(other.mode), rte_port_id(other.rte_port_id), txq(other.txq),
        rxq(other.rxq), rte_port_started(other.rte_port_started),
        mac_addr(other.mac_addr), dev_info(other.dev_info),
        lcore_contexts(std::move(other.lcore_contexts)),
        stats_aggregator(other.stats_aggregator), start_time(other.start_time),
        send_config(other.send_config), cli_args(other.cli_args),
        ip4_checksum_offload(other.ip4_checksum_offload),
        udp_checksum_offload(other.udp_checksum_offload),
        tx_idx(other.tx_idx.load()) {}

  ~port_context();
  void assign_lcores(const std::vector<unsigned> &lcores);
  bool config_port();
  bool start();
};
