use std::ffi;
use std::ptr;
use clap::arg;

use crate::misc_types::{parse_mac, EtherAddr};

#[derive(Debug)]
#[repr(C)]
pub enum DPRunMode {
  Reflect = 1,
  SendRecv = 2,
}

#[derive(Debug)]
#[repr(C)]
pub struct DPCmdArgs {
  pub mode: DPRunMode,

  pub ports: *mut EtherAddr,
  pub nb_ports: u32,

  pub nb_rxq: u32,
  pub nb_txq: u32,
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
      arg!(-p --ports <ports> ...
          "Specify which ports to use (defaults to all ports discoverable), in the form of
           MAC addresses. This can be specified multiple times."),
      arg!(-r <rxq> "Number of receive queues per port (default: 1)")
        .default_value("1")
        .value_parser(clap::value_parser!(u32)),
      arg!(-t <txq> "Number of transmit queues per port (default: 1)")
        .default_value("1")
        .value_parser(clap::value_parser!(u32)),
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
}
