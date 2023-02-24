//! This module implements a multi-threaded, simple statistics aggregator, which
//! allows us to track statistics like latency and drop rate across time.
//!
//! It works by dividing the timeline into small, fixed-size steps, and keeping
//! aggregated information for each step. There is also automatic eviction of
//! old steps to keep memory usage bounded. Whenever a step access is attempted
//! which goes over the range of steps we currently have, we evict all steps
//! older than a certain threshold.
//!
//! It allows inserting new values into any steps that are still in memory, and
//! supports exporting the aggregated information as a CSV file.
//!
//! The unit of the time values provided to this module can be arbitrary.

use std::{
  sync::{atomic::AtomicU64, RwLock},
};

pub struct StatsAggregator {
  /// Duration of each step.
  step_size: u64,

  /// Number of steps to keep in memory.
  max_steps: usize,

  /// Eviction threshold in time units.
  evict_threshold: u64,

  /// The buffer
  locked_part: RwLock<LockedPart>,

  stats_writer: Option<Box<dyn Fn(u64, &Stats) + Sync>>,
}

#[derive(Debug, Default)]
struct LockedPart {
  /// The index of the first step stored in the steps buffer.
  first_step_idx: usize,

  /// The steps buffer.
  steps_buf: Vec<Stats>,
}

/// Aggregated statistics for a single step.
#[derive(Debug, Default)]
pub struct Stats {
  /// Number of packets sent in this step.
  pub tx_packets: AtomicU64,

  /// Number of packets received in this step.
  pub rx_packets: AtomicU64,

  /// Number of packets received that was sent in this step.  This is used to
  /// calculate the drop rate.
  pub rx_packets_sent_here: AtomicU64,

  /// Total latency of all packets that were *sent* in this step.
  pub total_latency_sent_here: AtomicU64,
}

impl StatsAggregator {
  /// Creates a new [`StatsAggregator`].
  ///
  /// ## Parameters
  ///
  /// * `step_size`: The duration of each step.
  /// * `keep_time`: The total duration of time to keep in memory.
  /// * `evict_threshold`: The time threshold for evicting old steps.
  /// * `stats_writer`: An optional callback that will be called whenever a step
  ///   is evicted.  This can be used to write the aggregated statistics to a
  ///   file, for example.
  ///
  /// Passing a `evict_threshold` of 0 will disable eviction, and `stats_writer`
  /// will be called immediately for each step.
  pub fn new(
    step_size: u64,
    keep_time: u64,
    evict_threshold: u64,
    stats_writer: Option<impl Fn(u64, &Stats) + Sync + 'static>,
  ) -> Self {
    let max_steps = (keep_time / step_size + 1) as usize;
    let s = Self {
      step_size,
      max_steps,
      evict_threshold,
      locked_part: RwLock::new(LockedPart {
        first_step_idx: 0,
        steps_buf: Vec::with_capacity(max_steps),
      }),
      stats_writer: stats_writer.map(|f| Box::new(f) as _),
    };
    s.locked_part.write().unwrap().steps_buf.resize_with(max_steps, Default::default);
    s
  }

  /// Use a callback to access the statistics for a given step, allowing
  /// modification of the statistics.  Will create new steps / evict old steps.
  ///
  /// ## Returns
  ///
  /// Returns `true` if the step was accessed, or `false` if the step was
  /// already evicted in the past.
  pub fn access_step(&self, time: u64, f: impl FnOnce(&Stats)) -> bool {
    let step: usize = (time / self.step_size).try_into().unwrap();
    let read_lock = self.locked_part.read().unwrap();
    debug_assert_eq!(read_lock.steps_buf.len(), self.max_steps);
    if step < read_lock.first_step_idx {
      return false;
    }
    let step_buf_idx = step - read_lock.first_step_idx;
    if step_buf_idx >= self.max_steps {
      drop(read_lock);
      let mut write_lock = self.locked_part.write().unwrap();

      // Evict any steps that are too old.
      let mut front_ptr = 0usize;
      let locked_part = &mut *write_lock;
      let first_step_idx = &mut locked_part.first_step_idx;
      let buf = &mut locked_part.steps_buf;
      while time.saturating_sub(self.evict_threshold) > *first_step_idx as u64 * self.step_size && front_ptr < self.max_steps {
        let s = &buf[front_ptr];
        if let Some(stats_writer) = &self.stats_writer {
          stats_writer(*first_step_idx as u64 * self.step_size, &s);
        }
        *first_step_idx += 1;
        front_ptr += 1;
      }

      drop(buf.drain(..front_ptr));
      buf.resize_with(self.max_steps, Default::default);

      f(&write_lock.steps_buf[step - write_lock.first_step_idx]);
      true
    } else {
      f(&read_lock.steps_buf[step_buf_idx]);
      true
    }
  }
}
