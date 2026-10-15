//! Sixel decoding: `DCS Pa ; Pb ; Ph q <data> ST` into RGBA pixels.
//!
//! <https://vt100.net/docs/vt3xx-gp/chapter14.html>

/// A decoded sixel image.
pub struct SixelImage {
  pub width: u32,
  pub height: u32,
  /// RGBA, row-major.
  pub pixels: Vec<u8>,
}

/// The most pixels an image may have.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// Decodes the body of a sixel DCS: parameters, `q`, and the data.
pub fn decode(body: &[u8]) -> Option<SixelImage> {
  // Parameters: Pa (aspect ratio), Pb (background: 1 keeps pixels
  // transparent where nothing is drawn), Ph (grid size, ignored).
  let q = body.iter().position(|b| *b == b'q')?;
  let params: Vec<u32> = body[..q]
    .split(|b| *b == b';')
    .map(|p| {
      std::str::from_utf8(p)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
    })
    .collect();
  let transparent_background = params.get(1).copied().unwrap_or(0) == 1;
  let mut decoder = Decoder::new(transparent_background);
  decoder.run(&body[q + 1..])?;
  decoder.finish()
}

struct Decoder {
  /// Pixels as (x, y) -> color index + 1; 0 is background.
  canvas: Vec<u32>,
  width: u32,
  height: u32,
  /// Declared by raster attributes, if any.
  declared: Option<(u32, u32)>,
  palette: Vec<[u8; 3]>,
  color: usize,
  x: u32,
  y: u32,
  transparent: bool,
}

impl Decoder {
  fn new(transparent: bool) -> Self {
    let mut palette = Vec::with_capacity(256);
    // The VT340 default colors.
    for rgb in DEFAULT_PALETTE {
      palette.push(rgb);
    }
    Decoder {
      canvas: Vec::new(),
      width: 0,
      height: 0,
      declared: None,
      palette,
      color: 0,
      x: 0,
      y: 0,
      transparent,
    }
  }

  fn run(&mut self, data: &[u8]) -> Option<()> {
    let mut i = 0;
    let mut repeat: Option<u32> = None;
    while i < data.len() {
      let b = data[i];
      match b {
        b'"' => {
          // Raster attributes: Pan ; Pad ; Ph ; Pv
          let (params, next) = read_params(&data[i + 1..]);
          i += 1 + next;
          if let (Some(w), Some(h)) = (params.get(2), params.get(3))
            && *w > 0
            && *h > 0
            && u64::from(*w) * u64::from(*h) <= MAX_PIXELS
          {
            self.declared = Some((*w, *h));
            self.ensure(*w, *h)?;
          }
        }
        b'#' => {
          let (params, next) = read_params(&data[i + 1..]);
          i += 1 + next;
          let Some(index) = params.first() else {
            continue;
          };
          let index = (*index as usize).min(255);
          if params.len() >= 5 {
            // Color definition: Pc ; Pu ; Px ; Py ; Pz
            let rgb = match params[1] {
              1 => hls_to_rgb(params[2], params[3], params[4]),
              _ => [percent(params[2]), percent(params[3]), percent(params[4])],
            };
            while self.palette.len() <= index {
              self.palette.push([0, 0, 0]);
            }
            self.palette[index] = rgb;
          }
          self.color = index;
        }
        b'!' => {
          let (params, next) = read_params(&data[i + 1..]);
          i += 1 + next;
          repeat = Some(params.first().copied().unwrap_or(1).max(1));
        }
        b'$' => {
          self.x = 0;
          i += 1;
        }
        b'-' => {
          self.x = 0;
          self.y += 6;
          i += 1;
        }
        0x3F..=0x7E => {
          let bits = b - 0x3F;
          let count = repeat.take().unwrap_or(1);
          self.sixel(bits, count)?;
          i += 1;
        }
        // Whitespace and anything else between items.
        _ => i += 1,
      }
    }
    Some(())
  }

  fn ensure(&mut self, width: u32, height: u32) -> Option<()> {
    if width <= self.width && height <= self.height {
      return Some(());
    }
    let new_w = width.max(self.width);
    let new_h = height.max(self.height);
    if u64::from(new_w) * u64::from(new_h) > MAX_PIXELS {
      return None;
    }
    let mut canvas = vec![0u32; new_w as usize * new_h as usize];
    for y in 0..self.height as usize {
      let src =
        &self.canvas[y * self.width as usize..(y + 1) * self.width as usize];
      canvas[y * new_w as usize..y * new_w as usize + self.width as usize]
        .copy_from_slice(src);
    }
    self.canvas = canvas;
    self.width = new_w;
    self.height = new_h;
    Some(())
  }

  fn sixel(&mut self, bits: u8, count: u32) -> Option<()> {
    if bits != 0 {
      self.ensure(self.x.checked_add(count)?, self.y.checked_add(6)?)?;
      let color = self.color as u32 + 1;
      for n in 0..6 {
        if bits & (1 << n) != 0 {
          let y = (self.y + n) as usize;
          let row = y * self.width as usize;
          for x in self.x..self.x + count {
            self.canvas[row + x as usize] = color;
          }
        }
      }
    }
    self.x += count;
    Some(())
  }

  fn finish(self) -> Option<SixelImage> {
    let (width, height) = match self.declared {
      Some((w, h)) => (w.min(self.width), h.min(self.height)),
      None => (self.width, self.height),
    };
    if width == 0 || height == 0 {
      return None;
    }
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
      for x in 0..width as usize {
        let index = self.canvas[y * self.width as usize + x];
        if index == 0 {
          if self.transparent {
            pixels.extend_from_slice(&[0, 0, 0, 0]);
          } else {
            let bg = self.palette.first().copied().unwrap_or([0, 0, 0]);
            pixels.extend_from_slice(&[bg[0], bg[1], bg[2], 255]);
          }
        } else {
          let rgb = self
            .palette
            .get(index as usize - 1)
            .copied()
            .unwrap_or([0, 0, 0]);
          pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
      }
    }
    Some(SixelImage {
      width,
      height,
      pixels,
    })
  }
}

/// Reads `;`-separated numbers; returns them and how many bytes they took.
fn read_params(data: &[u8]) -> (Vec<u32>, usize) {
  let mut params = Vec::new();
  let mut current: Option<u32> = None;
  let mut i = 0;
  while i < data.len() {
    match data[i] {
      b'0'..=b'9' => {
        let d = u32::from(data[i] - b'0');
        current =
          Some(current.unwrap_or(0).saturating_mul(10).saturating_add(d));
      }
      b';' => {
        params.push(current.take().unwrap_or(0));
      }
      _ => break,
    }
    i += 1;
  }
  if let Some(n) = current {
    params.push(n);
  }
  (params, i)
}

fn percent(v: u32) -> u8 {
  (v.min(100) * 255 / 100) as u8
}

/// HLS as sixel gives it: hue 0..360 (blue at 0), lightness and
/// saturation in percent.
fn hls_to_rgb(h: u32, l: u32, s: u32) -> [u8; 3] {
  let h = ((h as f64 + 240.0) % 360.0) / 60.0;
  let l = l.min(100) as f64 / 100.0;
  let s = s.min(100) as f64 / 100.0;
  let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
  let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
  let (r, g, b) = match h as u32 {
    0 => (c, x, 0.0),
    1 => (x, c, 0.0),
    2 => (0.0, c, x),
    3 => (0.0, x, c),
    4 => (x, 0.0, c),
    _ => (c, 0.0, x),
  };
  let m = l - c / 2.0;
  let to = |v: f64| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
  [to(r), to(g), to(b)]
}

/// The VT340's 16 default colors.
const DEFAULT_PALETTE: [[u8; 3]; 16] = [
  [0, 0, 0],
  [51, 51, 204],
  [204, 36, 36],
  [51, 204, 51],
  [204, 51, 204],
  [51, 204, 204],
  [204, 204, 51],
  [120, 120, 120],
  [69, 69, 69],
  [84, 84, 153],
  [153, 66, 66],
  [84, 153, 84],
  [153, 84, 153],
  [84, 153, 153],
  [153, 153, 84],
  [204, 204, 204],
];

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn decodes_a_small_image() {
    // Two colors, 3 pixels wide, one sixel row: `@` is the top pixel
    // only, `~` all six. Red tops on columns 0-2, then back to the
    // start and two full green columns over them.
    let body = b"0;1;0q#1;2;100;0;0#2;2;0;100;0#1@@@$#2!2~";
    let image = decode(body).unwrap();
    assert_eq!(image.width, 3);
    assert_eq!(image.height, 6);
    let px = |x: usize, y: usize| {
      let i = (y * 3 + x) * 4;
      [
        image.pixels[i],
        image.pixels[i + 1],
        image.pixels[i + 2],
        image.pixels[i + 3],
      ]
    };
    assert_eq!(px(0, 0), [0, 255, 0, 255]);
    assert_eq!(px(2, 0), [255, 0, 0, 255]);
    assert_eq!(px(1, 5), [0, 255, 0, 255]);
    assert_eq!(px(2, 1), [0, 0, 0, 0], "transparent background");
  }

  #[test]
  fn raster_attributes_and_newlines() {
    let body = b"q\"1;1;2;12#0~-#0~";
    let image = decode(body).unwrap();
    assert_eq!((image.width, image.height), (2, 12));
    assert!(decode(b"q").is_none());
  }

  #[test]
  fn hls() {
    assert_eq!(hls_to_rgb(0, 50, 100), [0, 0, 255]);
    assert_eq!(hls_to_rgb(120, 50, 100), [255, 0, 0]);
    assert_eq!(hls_to_rgb(240, 50, 100), [0, 255, 0]);
  }
}
