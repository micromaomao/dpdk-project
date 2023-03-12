mod cmdargs;
mod misc_types;
mod pkt;
mod stats;

use cmdargs::SendConfig;
use etherparse::{EtherType, Ethernet2Header, IpNumber, Ipv4Header, UdpHeader};
use misc_types::EtherAddr;
use std::mem::swap;

macro_rules! unwrap {
  ($val:expr) => {
    match $val {
      Ok(val) => val,
      Err(_) => {
        return false;
      }
    }
  };
  ($val:expr, $falseret:expr) => {
    match $val {
      Ok(val) => val,
      Err(_) => {
        return $falseret;
      }
    }
  };
}

#[no_mangle]
pub unsafe extern "C" fn dp_process_reflect_pkt(
  pkt: *mut u8,
  len: u32,
  need_ip_checksum: bool,
  need_udp_checksum: bool,
) -> bool {
  let mut pkt = std::slice::from_raw_parts_mut(pkt, len as usize);

  // Swap ether source and destination
  let (mut ether, _rest) = unwrap!(Ethernet2Header::from_slice(pkt));
  swap(&mut ether.source, &mut ether.destination);
  _ = ether.write(&mut pkt);

  // Parse out ip and udp header
  let (mut ip, ip_rest) = unwrap!(Ipv4Header::from_slice(pkt));
  let (mut udp, payload) = unwrap!(UdpHeader::from_slice(ip_rest));

  // Swap IP source and destination
  ip = Ipv4Header::new(ip.payload_len, 64, ip.protocol, ip.destination, ip.source);
  swap(&mut udp.source_port, &mut udp.destination_port);

  if need_udp_checksum {
    udp.checksum = unwrap!(udp.calc_checksum_ipv4(&ip, payload));
  }
  if need_ip_checksum {
    ip.header_checksum = unwrap!(ip.calc_header_checksum());
  }

  // Write back headers
  _ = ip.write(&mut pkt);
  _ = udp.write(&mut pkt);

  true
}

#[repr(C)]
pub struct DpMakePacketArgs {
  pub src_mac: EtherAddr,
  pub send_config: *const SendConfig,
  pub index: u64,
  pub timestamp: u64,
  pub need_ip_checksum: bool,
  pub need_udp_checksum: bool,
}

#[no_mangle]
pub unsafe extern "C" fn dp_make_packet(
  pkt_buf: *mut u8,
  pkt_buf_sz: usize,
  args: &DpMakePacketArgs,
) -> usize {
  let mut pkt_buf = std::slice::from_raw_parts_mut(pkt_buf, pkt_buf_sz);
  let send_config = &*args.send_config;
  let eth_hdr = Ethernet2Header {
    ether_type: EtherType::Ipv4 as u16,
    source: args.src_mac,
    destination: send_config.dest_mac,
  };
  _ = eth_hdr.write(&mut pkt_buf);

  const UDP_HDR_LEN: u16 = 8;
  let usr_payload_size = send_config.packet_size;
  debug_assert!(usr_payload_size < u16::MAX as u32 - UDP_HDR_LEN as u32);

  let mut ip_hdr = Ipv4Header::new(
    usr_payload_size as u16 + UDP_HDR_LEN,
    64,
    IpNumber::Udp as u8,
    send_config.source_ip,
    send_config.dest_ip,
  );

  if args.need_ip_checksum {
    ip_hdr.header_checksum = unwrap!(ip_hdr.calc_header_checksum(), 0);
  }
  _ = ip_hdr.write(&mut pkt_buf);
  pkt_buf = &mut pkt_buf[..ip_hdr.payload_len as usize];

  let (sp_start, sp_end) = send_config.source_port_range;
  let source_port = sp_start + (args.index % (sp_end as u64 - sp_start as u64 + 1)) as u16;
  let mut udp_hdr = UdpHeader {
    source_port,
    destination_port: send_config.dest_port,
    length: ip_hdr.payload_len,
    checksum: 0,
  };

  let payload = &mut pkt_buf[UDP_HDR_LEN as usize..];
  debug_assert_eq!(payload.len(), usr_payload_size as usize);
  pkt::write_packet(send_config.seed, args.index, args.timestamp, payload);

  if args.need_udp_checksum {
    udp_hdr.checksum = unwrap!(udp_hdr.calc_checksum_ipv4(&ip_hdr, &payload), 0);
  }
  pkt_buf[..UDP_HDR_LEN as usize].copy_from_slice(&udp_hdr.to_bytes());

  eth_hdr.header_len() as usize + ip_hdr.total_len() as usize
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ParsePacketRes {
  pub ok: bool,
  pub index: u64,
  pub send_time: u64,
}

impl ParsePacketRes {
  pub fn err() -> Self {
    Self {
      ok: false,
      index: 0,
      send_time: 0,
    }
  }
}

#[no_mangle]
pub unsafe extern "C" fn dp_parse_packet(
  pkt_buf: *const u8,
  pkt_buf_sz: usize,
  seed: u64,
  expected_payload_size: usize,
) -> ParsePacketRes {
  debug_assert!(expected_payload_size >= pkt::PACKET_HEAD_SIZE);
  let pkt = std::slice::from_raw_parts(pkt_buf, pkt_buf_sz);
  let (_ether, pkt) = unwrap!(Ethernet2Header::from_slice(pkt), ParsePacketRes::err());
  let (_ip, pkt) = unwrap!(Ipv4Header::from_slice(pkt), ParsePacketRes::err());
  let (udp, _rest) = unwrap!(UdpHeader::from_slice(pkt), ParsePacketRes::err());
  let pkt = &pkt[udp.header_len()..udp.length as usize];
  if pkt.len() != expected_payload_size {
    return ParsePacketRes::err();
  }
  match pkt::parse_packet(seed, pkt) {
    Ok(pkt_hdr) => ParsePacketRes {
      ok: true,
      index: pkt_hdr.index,
      send_time: pkt_hdr.send_time,
    },
    Err(_) => ParsePacketRes::err(),
  }
}
