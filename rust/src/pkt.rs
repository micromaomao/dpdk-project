//! Packet generation and validation utils
//!
//! This module contains helper functions for generating packets with useful
//! metadata that would allow us to track statistics like latency and drop rate
//! across time. It also uses the Pcg64 pesudo-random number generator to
//! generate paddings that can be used to fill up packets to a certain size, and
//! validated on the receiving end.
//!
//! Time values provided to this module can be in any unit.

use rand::RngCore;

/// The header which appears on every packet, which contains useful metadata which
/// aids statistics.
#[repr(C)]
pub struct PacketHeader {
  pub index: u64,
  pub send_time: u64,
}

pub const PACKET_HEAD_SIZE: usize = std::mem::size_of::<PacketHeader>();

/// Write a packet to the given buffer.
///
/// The size of the packet will be determined by the size of the buffer. This
/// size must be at least [`PACKET_HEAD_SIZE`] bytes.
///
/// The provided seed will be used to generate packet padding, and the same
/// value should be provided to parse_packet.
pub fn write_packet(seed: u64, index: u64, send_time: u64, buf: &mut [u8]) {
  debug_assert!(buf.len() >= PACKET_HEAD_SIZE);

  let ph = PacketHeader { index, send_time };

  // Put the header at the beginning of the buffer.
  //
  // Safety:
  //  - The buffer is at least PACKET_HEAD_SIZE bytes long.
  //  - Alignment is not an issue since we're copying byte-by-byte.
  unsafe {
    std::ptr::copy_nonoverlapping(
      &ph as *const PacketHeader as *const u8,
      buf.as_mut_ptr(),
      PACKET_HEAD_SIZE,
    );
  }

  let mut rng = rand_pcg::Pcg64Mcg::new(((seed as u128) << 64) | (index as u128));
  rng.fill_bytes(&mut buf[PACKET_HEAD_SIZE..]);
}

/// Parse a packet from the given buffer, validate the padding, and return the
/// packet header.
///
/// If the packet fails to validate, an error will be returned.
pub fn parse_packet(seed: u64, buf: &[u8]) -> Result<PacketHeader, ()> {
  debug_assert!(buf.len() >= PACKET_HEAD_SIZE);

  // Read the header from the beginning of the buffer.
  //
  // Safety:
  //  - The buffer is at least PACKET_HEAD_SIZE bytes long.
  //  - We use read_unaligned to avoid alignment issues.
  let ph = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const PacketHeader) };

  let mut rng = rand_pcg::Pcg64Mcg::new(((seed as u128) << 64) | (ph.index as u128));

  // We do not want to allocate a buffer to hold the computed padding since this
  // function is likely to be called in a tight loop. Instead, we will compute
  // the padding in chunks of 64 bytes, and each time compare it to the
  // corresponding region in the packet.
  let mut remaining = &buf[PACKET_HEAD_SIZE..];
  while !remaining.is_empty() {
    let mut chunk = [0u8; 64];
    rng.fill_bytes(&mut chunk);

    let len = std::cmp::min(remaining.len(), chunk.len());
    if &remaining[..len] != &chunk[..len] {
      return Err(());
    }

    remaining = &remaining[len..];
  }

  Ok(ph)
}
