//! Contains FFI bindings for the stats module, to be used in the DPDK project.

use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::stats::StatsAggregator;

use super::get_time_value_from_duration;

pub struct RustInstant(Instant);

#[no_mangle]
pub unsafe extern "C" fn dp_get_reference_time() -> *mut RustInstant {
  Box::into_raw(Box::new(RustInstant(Instant::now())))
}

#[no_mangle]
pub unsafe extern "C" fn dp_get_time_value_since(reference: *mut RustInstant) -> u64 {
  let time = get_time_value_from_duration(reference.as_ref().unwrap().0.elapsed());
  time as u64
}

#[no_mangle]
pub unsafe extern "C" fn dp_free_reference_time(reference: *mut RustInstant) {
  drop(Box::from_raw(reference));
}

#[no_mangle]
pub unsafe extern "C" fn dp_stats_add(
  aggregator: *mut StatsAggregator,
  time: u64,
  tx: u64,
  rx: u64,
  rx_sent_here: u64,
  latency: u64,
) {
  let agg = aggregator.as_ref().unwrap();
  agg.access_step(time, move |stats| {
    stats.tx_packets.fetch_add(tx, Ordering::Relaxed);
    stats.rx_packets.fetch_add(rx, Ordering::Relaxed);
    if rx_sent_here != 0 {
      stats
        .rx_packets_sent_here
        .fetch_add(rx_sent_here, Ordering::Relaxed);
      stats
        .total_latency_sent_here
        .fetch_add(latency, Ordering::Relaxed);
    }
  });
}

#[no_mangle]
pub unsafe extern "C" fn dp_stats_get_tx(aggregator: *mut StatsAggregator, time: u64) -> u64 {
  let agg = aggregator.as_ref().unwrap();
  let mut res = 0;
  agg.access_step(time, |stats| {
    res = stats.tx_packets.load(Ordering::Relaxed);
  });
  res
}

/*
/// Add the number of packets (about to be) transmitted to the stats aggregator.
///
/// This function deals with rate limiting, and so will return the actual number
/// added. Caller should actually only send out this many packets.
///
/// A rate limit of 0 means no rate limiting.
#[no_mangle]
pub unsafe extern "C" fn dp_stats_add_tx(
  aggregator: *mut StatsAggregator,
  time: u64,
  nb_tx: u64,
  rate_limit: u64,
) -> u64 {
  let agg = aggregator.as_ref().unwrap();
  let mut res = 0;
  agg.access_step(time, |stats| {
    let new_tx_tot = stats.tx_packets.fetch_add(nb_tx, Ordering::Relaxed) + nb_tx;
    if rate_limit > 0 && new_tx_tot > rate_limit {
      let overshot = new_tx_tot - rate_limit;
      if overshot >= nb_tx {
        // We have already reached or exceeded the limit even before this call.
        // Hence revert all packets.
        stats.tx_packets.fetch_sub(nb_tx, Ordering::Relaxed);
        res = 0;
      } else {
        stats.tx_packets.fetch_sub(overshot, Ordering::Relaxed);
        res = nb_tx - overshot;
      }
    } else {
      res = nb_tx;
    }
  });
  res
}

*/
