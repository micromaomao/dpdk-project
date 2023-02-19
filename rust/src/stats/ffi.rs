//! Contains FFI bindings for the stats module, to be used in the DPDK project.

use std::os::raw::c_char;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::{ffi::CStr, time::Instant};

use crate::stats::{csv_writer::get_csv_writer, StatsAggregator};

#[no_mangle]
pub unsafe extern "C" fn dp_new_stats_aggregator(
  step_size: u64,
  keep_time: u64,
  evict_threshold: u64,
  csv_file_name: *const c_char,
) -> *mut StatsAggregator {
  let writer = Some(
    get_csv_writer(Path::new(
      CStr::from_ptr(csv_file_name)
        .to_str()
        .expect("Invalid path"),
    ))
    .expect("Failed to create CSV writer"),
  );
  Box::into_raw(Box::new(StatsAggregator::new(
    step_size,
    keep_time,
    evict_threshold,
    writer,
  )))
}

pub struct RustInstant(Instant);

#[no_mangle]
pub unsafe extern "C" fn dp_free_stats_aggregator(aggregator: *mut StatsAggregator) {
  drop(Box::from_raw(aggregator));
}

#[no_mangle]
pub unsafe extern "C" fn dp_get_reference_time() -> *mut RustInstant {
  Box::into_raw(Box::new(RustInstant(Instant::now())))
}

#[no_mangle]
pub unsafe extern "C" fn dp_get_time_value_since(reference: *mut RustInstant) -> u64 {
  let time = reference.as_ref().unwrap().0.elapsed().as_millis();
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
