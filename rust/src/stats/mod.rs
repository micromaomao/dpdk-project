mod aggregator;
use std::time::{Duration, Instant};

pub use aggregator::*;

mod csv_writer;
pub use csv_writer::*;

pub fn get_time_value(start: Instant, current: Instant) -> u64 {
  get_time_value_from_duration(current.duration_since(start))
}

pub fn get_time_value_now(start: Instant) -> u64 {
  get_time_value(start, Instant::now())
}

pub fn get_time_value_from_duration(dur: Duration) -> u64 {
  dur.as_millis() as u64
}
