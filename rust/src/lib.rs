mod cmdargs;
mod misc_types;
mod stats;

use etherparse::{Ethernet2Header, Ipv4Header, UdpHeader};
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
    unwrap!(udp.calc_checksum_ipv4(&ip, payload));
  }
  if need_ip_checksum {
    unwrap!(ip.calc_header_checksum());
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
  pub dst_mac: EtherAddr,
  pub src_ip: [u8; 4],
  pub dst_ip: [u8; 4],
  pub src_port: u16,
  pub dst_port: u16,
  pub index: u64,
  pub timestamp: u64,
  pub need_ip_checksum: bool,
  pub need_udp_checksum: bool,
}

#[no_mangle]
pub unsafe extern "C" fn dp_make_packet(pkt_buf: *mut u8, pkt_buf_sz: usize, args: &DpMakePacketArgs) -> usize {
  let mut pkt_buf = std::slice::from_raw_parts_mut(pkt_buf, pkt_buf_sz);
  unimplemented!()
}
