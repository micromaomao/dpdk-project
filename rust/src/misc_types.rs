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
