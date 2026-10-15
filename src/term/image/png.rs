//! Just enough PNG to know an image's size: the IHDR chunk.

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Width and height of a PNG, from its header; `None` if it is not one.
pub fn dimensions(data: &[u8]) -> Option<(u32, u32)> {
  if data.len() < 24 || data[..8] != SIGNATURE {
    return None;
  }
  if &data[12..16] != b"IHDR" {
    return None;
  }
  let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
  let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
  if width == 0 || height == 0 {
    return None;
  }
  Some((width, height))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_the_header() {
    let mut png = SIGNATURE.to_vec();
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&640u32.to_be_bytes());
    png.extend_from_slice(&480u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    assert_eq!(dimensions(&png), Some((640, 480)));
    assert_eq!(dimensions(b"not a png at all, definitely not"), None);
  }
}
