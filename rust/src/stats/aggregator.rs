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
  collections::VecDeque,
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
  steps_buf: VecDeque<Stats>,
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
    Self {
      step_size,
      max_steps,
      evict_threshold,
      locked_part: RwLock::new(LockedPart {
        first_step_idx: 0,
        steps_buf: VecDeque::with_capacity(max_steps),
      }),
      stats_writer: stats_writer.map(|f| Box::new(f) as _),
    }
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
    if step < read_lock.first_step_idx {
      return false;
    }
    let step_buf_idx = step - read_lock.first_step_idx;
    if step_buf_idx >= read_lock.steps_buf.len() {
      drop(read_lock);
      let mut write_lock = self.locked_part.write().unwrap();

      // Evict any steps that are too old.
      while time.saturating_sub(self.evict_threshold) > write_lock.first_step_idx as u64 * self.step_size {
        if let Some(s) = write_lock.steps_buf.pop_front() {
          if let Some(stats_writer) = &self.stats_writer {
            stats_writer(write_lock.first_step_idx as u64 * self.step_size, &s);
          }
          write_lock.first_step_idx += 1;
        } else {
          // The queue is completely empty, so we can just reset the first step index.
          write_lock.first_step_idx = step;
          break;
        }
      }

      // Now insert the new step, if it does not exist.  (Due to the
      // pre-allocation, this is really just a memset 0)
      while write_lock.first_step_idx + write_lock.steps_buf.len() <= step {
        write_lock.steps_buf.push_back(Default::default());
        if write_lock.steps_buf.len() > self.max_steps {
          // This suggest that max_steps is too small to hold even a single new
          // step after eviction. Either it is too small, or the eviction
          // threshold is too large.  At this point let's just panic.
          panic!("max_steps is too small to hold new steps after eviction");
        }
      }

      f(&write_lock.steps_buf[step - write_lock.first_step_idx]);
      true
    } else {
      f(&read_lock.steps_buf[step_buf_idx]);
      true
    }
  }
}
