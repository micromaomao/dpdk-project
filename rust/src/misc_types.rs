use std::{net::SocketAddrV4, str::FromStr};

pub type EtherAddr = [u8; 6];

pub fn parse_mac(s: &str) -> Option<EtherAddr> {
  let mut mac = [0u8; 6];
  let mut i = 0;
  for b in s.split(':') {
    if i >= 6 {
      return None;
    }
    mac[i] = match u8::from_str_radix(b, 16) {
      Ok(v) => v,
      Err(_) => return None,
    };
    i += 1;
  }
  if i != 6 {
    return None;
  }
  Some(mac)
}

pub type Ip4Addr = [u8; 4];

pub fn parse_ip4(s: &str) -> Option<Ip4Addr> {
  std::net::Ipv4Addr::from_str(s).ok().map(|ip| ip.octets())
}

pub fn parse_ip4_and_port(s: &str) -> Option<(Ip4Addr, u16)> {
  let sockaddr = SocketAddrV4::from_str(s).ok()?;
  let ip = sockaddr.ip().octets();
  let port = sockaddr.port();
  Some((ip, port))
}
