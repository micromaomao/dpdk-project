use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::errors::AppError;
use crate::stats::Stats;

/// A buffered CSV writer for stats.
///
/// This implementation flushes the buffer every second so that the user can see
/// the stats immediately.
struct CsvStatsFile {
  f: BufWriter<File>,
  last_flush: Instant,
}

impl CsvStatsFile {
  pub fn new(path: impl AsRef<Path>) -> Result<Self, AppError> {
    let mut f = File::create(path).map_err(|e| AppError::StatsFileError(e))?;
    write!(f, "time,tx_packets,rx_packets,drop_rate,avg_latency\n")
      .map_err(|e| AppError::StatsFileError(e))?;
    Ok(Self {
      f: BufWriter::new(f),
      last_flush: Instant::now(),
    })
  }

  pub fn write(&mut self, time: u64, stat: &Stats) -> Result<(), AppError> {
    let tx_packets = stat.tx_packets.load(Ordering::Acquire);
    let rx_packets_sent_here = stat.rx_packets_sent_here.load(Ordering::Acquire);
    let tot_latency = stat.total_latency_sent_here.load(Ordering::Acquire);
    write!(
      self.f,
      "{},{},{},{},{}\n",
      time,
      tx_packets,
      stat.rx_packets.load(Ordering::Acquire),
      if rx_packets_sent_here == 0 {
        0.0
      } else {
        1.0 - (rx_packets_sent_here as f64 / tx_packets as f64)
      },
      if rx_packets_sent_here == 0 {
        0.0
      } else {
        tot_latency as f64 / rx_packets_sent_here as f64
      },
    )
    .map_err(|e| AppError::StatsFileError(e))?;
    let now = Instant::now();
    if now - self.last_flush > Duration::from_secs(1) {
      self.f.flush().map_err(|e| AppError::StatsFileError(e))?;
      self.last_flush = now;
    }
    Ok(())
  }
}

pub fn get_csv_writer(
  path: impl AsRef<Path>,
) -> Result<impl for<'a> Fn(u64, &'a Stats) + Send + Sync + 'static, AppError> {
  let f = CsvStatsFile::new(path)?;
  let f = Mutex::new(f);
  Ok(move |time, stat: &Stats| {
    f.lock()
      .unwrap()
      .write(time, stat)
      .expect("failed to write stats")
  })
}
