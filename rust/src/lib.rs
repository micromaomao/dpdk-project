mod cmdargs;
mod misc_types;
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
  pub pkt_size: u32,
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
  debug_assert!(args.pkt_size < u16::MAX as u32 - UDP_HDR_LEN as u32);

  let mut ip_hdr = Ipv4Header::new(
    args.pkt_size as u16 + UDP_HDR_LEN,
    64,
    IpNumber::Udp as u8,
    send_config.source_ip,
    send_config.dest_ip,
  );

  if args.need_ip_checksum {
    ip_hdr.header_checksum = unwrap!(ip_hdr.calc_header_checksum());
  }
  _ = ip_hdr.write(&mut pkt_buf);

  let (sp_start, sp_end) = send_config.source_port_range;
  let source_port = sp_start + (args.index % (sp_end as u64 - sp_start as u64 + 1)) as u16;
  let mut udp_hdr = UdpHeader {
    source_port,
    destination_port: send_config.dest_port,
    length: ip_hdr.payload_len,
    checksum: 0,
  };

  let payload = &mut pkt_buf[UDP_HDR_LEN as usize..];
  debug_assert!(payload.len() >= args.pkt_size as usize);
  // TODO: fill payload

  if args.need_udp_checksum {
    udp_hdr.checksum = unwrap!(udp_hdr.calc_checksum_ipv4(&ip_hdr, &payload));
  }
  pkt_buf[..UDP_HDR_LEN as usize].copy_from_slice(&udp_hdr.to_bytes());

  unimplemented!()
}
