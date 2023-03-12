use clap::arg;
use clap::ArgMatches;
use std::ffi;
use std::ptr;
use std::time::Duration;

use crate::misc_types::parse_ip4;
use crate::misc_types::parse_ip4_and_port;
use crate::misc_types::{parse_mac, EtherAddr, Ip4Addr};
use crate::stats::get_time_value_from_duration;
use crate::stats::StatsAggregator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum DPRunMode {
  Reflect = 1,
  SendRecv = 2,
}

#[derive(Debug)]
pub struct SendConfig {
  pub source_ip: Ip4Addr,
  pub source_port_range: (u16, u16),
  pub dest_mac: EtherAddr,
  pub dest_ip: Ip4Addr,
  pub dest_port: u16,
  pub seed: u64,
  pub packet_size: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct DPCmdArgs {
  pub mode: DPRunMode,

  pub ports: *mut EtherAddr,
  pub nb_ports: u32,

  /// This is opaque from C
  pub send_config: *mut SendConfig,

  /// A copy of the value in send_config to enable access from C
  pub packet_size: u32,
  /// A copy of the value in send_config to enable access from C
  pub seed: u64,

  pub nb_rxq: u32,
  pub nb_txq: u32,
  pub burst_size: u32,

  pub rate_limit: u64,

  pub stats: *mut StatsAggregator,
}

fn make_stats_aggregator_from_arg(arg: &ArgMatches) -> StatsAggregator {
  let stats_file = &arg.get_one::<String>("stats-file");
  let writer;
  if let Some(stats_file) = stats_file {
    writer = Some(
      crate::stats::get_csv_writer(stats_file).expect("Unable to open stats file for writing"),
    );
  } else {
    writer = None;
  }
  let stats = StatsAggregator::new(
    get_time_value_from_duration(Duration::from_millis(
      *arg.get_one::<u64>("stats-interval-ms").unwrap(),
    )),
    get_time_value_from_duration(Duration::from_secs(
      *arg.get_one::<u64>("stats-evict-interval-secs").unwrap(),
    )),
    get_time_value_from_duration(Duration::from_secs(
      *arg.get_one::<u64>("stats-evict-threshold-secs").unwrap(),
    )),
    writer,
  );
  stats
}

#[no_mangle]
pub unsafe extern "C" fn dp_parse_args(
  argc: ffi::c_int,
  argv: *const *const ffi::c_char,
) -> *mut DPCmdArgs {
  let argv = (0..argc)
    .map(|i| {
      ffi::CStr::from_ptr(argv.add(i.try_into().unwrap()).read())
        .to_str()
        .expect("invalid UTF-8 in argument")
    })
    .collect::<Vec<_>>();

  let matches = clap::Command::new("dpdk-project")
    .version(env!("CARGO_PKG_VERSION"))
    .args([
      arg!(<mode> "Run mode, either 'reflect' or 'sendrecv'").required(true),
      arg!(-p --ports <mac> ...
          "Specify which ports to use (defaults to all ports discoverable), in the form of
           MAC addresses. This can be specified multiple times.")
        .required(true),
      arg!(-r --rxq <number> "Number of receive queues per port (default: 1)")
        .default_value("1")
        .value_parser(clap::value_parser!(u32)),
      arg!(-t --txq <number> "Number of transmit queues per port (default: 1)")
        .default_value("1")
        .value_parser(clap::value_parser!(u32)),
      arg!(--"burst-size" <number> "Number of packets to pass to the NIC driver at once")
        .default_value("32")
        .value_parser(clap::value_parser!(u32).range(1..=128)),
      arg!(-s --"stats-file" <file> "Output packet stats to CSV.")
        .required(false),
      arg!(-i --"stats-interval-ms" <millis> "Interval in milliseconds between stat steps.")
        .default_value("100")
        .value_parser(clap::value_parser!(u64).range(1..)),
      arg!(--"stats-evict-interval-secs" <secs> "Number of seconds between stats dump.")
        .default_value("60")
        .value_parser(clap::value_parser!(u64).range(1..)),
      arg!(--"stats-evict-threshold-secs" <secs> "On each stats dump, stats older than this many seconds will be dumped.")
        .default_value("10")
        .value_parser(clap::value_parser!(u64).range(1..)),
      arg!(--"packet-size" <bytes> "Size of packets to send")
        .default_value("1000")
        .value_parser(clap::value_parser!(u32).range(1..)),
      arg!(--src <ip> "Source IP address for sendrecv mode")
        .required_if_eq("mode", "sendrecv"),
      arg!(--"src-port-range" <port_range> "Source port range for sendrecv mode. Each packet may be sent from any port within this range.")
        .default_value("10000-11000"),
      arg!(--dest <ipport> "Destination address for sendrecv mode (ip:port)")
        .required_if_eq("mode", "sendrecv"),
      arg!(--"dest-mac" <mac> "Destination MAC address for sendrecv mode. This is required because we don't currently implement ARP.")
        .required_if_eq("mode", "sendrecv"),
      arg!(--seed <seed> "Seed for sendrecv mode, 64 bit positive integer")
        .default_value("4107683144946382073")
        .value_parser(clap::value_parser!(u64)),
      arg!(-R --"rate-limit" <nb_pkts> "Limit rate of sending, in packets per stats interval, 0 for no limit")
        .default_value("0")
        .value_parser(clap::value_parser!(u64)),
    ])
    .get_matches_from(&argv);

  let mut parsed_args = DPCmdArgs {
    mode: match matches.get_one::<String>("mode").unwrap().as_str() {
      "reflect" => DPRunMode::Reflect,
      "sendrecv" => DPRunMode::SendRecv,
      _ => panic!("Invalid mode - must be either 'reflect' or 'sendrecv'"),
    },
    ports: ptr::null_mut(),
    nb_ports: 0,
    nb_rxq: *matches.get_one::<u32>("rxq").unwrap(),
    nb_txq: *matches.get_one::<u32>("txq").unwrap(),
    burst_size: *matches.get_one::<u32>("burst-size").unwrap(),
    packet_size: *matches.get_one::<u32>("packet-size").unwrap(),
    rate_limit: *matches.get_one::<u64>("rate-limit").unwrap(),
    send_config: ptr::null_mut(),
    seed: *matches.get_one::<u64>("seed").unwrap(),
    stats: Box::into_raw(Box::new(make_stats_aggregator_from_arg(&matches))),
  };

  let ports = matches
    .get_many::<String>("ports")
    .map(|p| p.collect::<Vec<&String>>())
    .unwrap_or_default();
  if !ports.is_empty() {
    let mut ports_arr = Vec::new();
    for &p in ports.iter() {
      ports_arr.push(parse_mac(p.as_str()).expect("Invalid MAC address"));
    }
    parsed_args.ports = Box::into_raw(ports_arr.into_boxed_slice()) as *mut EtherAddr;
    parsed_args.nb_ports = ports.len() as u32;
  }

  if parsed_args.mode == DPRunMode::SendRecv {
    let (dest_ip, dest_port) =
      parse_ip4_and_port(matches.get_one::<String>("dest").unwrap().as_str())
        .expect("Invalid destination address");
    parsed_args.send_config = Box::into_raw(Box::new(SendConfig {
      source_ip: parse_ip4(matches.get_one::<String>("src").unwrap().as_str())
        .expect("Invalid source IP address"),
      source_port_range: {
        let s = matches.get_one::<String>("src-port-range").unwrap();
        let mut split = s.split('-');
        let start = split
          .next()
          .unwrap()
          .parse::<u16>()
          .expect("Invalid source port range");
        if let Some(end) = split.next() {
          let end = end.parse::<u16>().expect("Invalid source port range");
          if end < start {
            panic!("Invalid source port range");
          }
          (start, end)
        } else {
          (start, start)
        }
      },
      dest_ip,
      dest_port,
      dest_mac: parse_mac(matches.get_one::<String>("dest-mac").unwrap().as_str())
        .expect("Invalid destination MAC address"),
      seed: parsed_args.seed,
      packet_size: parsed_args.packet_size,
    }))
  }

  Box::into_raw(Box::new(parsed_args))
}

#[no_mangle]
pub unsafe extern "C" fn dp_free_args(args: *mut DPCmdArgs) {
  let arg = args.read();
  if !arg.ports.is_null() {
    drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
      arg.ports,
      arg.nb_ports as usize,
    )));
  }
  if !arg.stats.is_null() {
    drop(Box::from_raw(arg.stats));
  }
  if !arg.send_config.is_null() {
    drop(Box::from_raw(arg.send_config));
  }
}
