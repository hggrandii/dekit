#[macro_export]
macro_rules! color {
  ($hex:literal) => {{
    const COLOR: $crate::term::Color = {
      let bytes = $hex.as_bytes();
      assert!(bytes.len() == 7, "expected #RRGGBB format");
      assert!(bytes[0] == b'#', "expected #RRGGBB format");

      const fn hex_digit(b: u8) -> u8 {
        match b {
          b'0'..=b'9' => b - b'0',
          b'a'..=b'f' => b - b'a' + 10,
          b'A'..=b'F' => b - b'A' + 10,
          _ => panic!("invalid hex digit"),
        }
      }

      let r = hex_digit(bytes[1]) * 16 + hex_digit(bytes[2]);
      let g = hex_digit(bytes[3]) * 16 + hex_digit(bytes[4]);
      let b = hex_digit(bytes[5]) * 16 + hex_digit(bytes[6]);
      $crate::term::Color::Rgb(r, g, b)
    };
    COLOR
  }};
}

/// Represents a foreground or background color for cells.
#[derive(Eq, PartialEq, Debug, Default, Copy, Clone, Hash)]
pub enum Color {
  /// The default terminal color.
  #[default]
  Default,

  /// An indexed terminal color.
  Idx(u8),

  /// An RGB terminal color. The parameters are (red, green, blue).
  Rgb(u8, u8, u8),
}

#[allow(dead_code)]
impl Color {
  pub const BLACK: Self = Color::Idx(0);
  pub const RED: Self = Color::Idx(1);
  pub const GREEN: Self = Color::Idx(2);
  pub const YELLOW: Self = Color::Idx(3);
  pub const BLUE: Self = Color::Idx(4);
  pub const MAGENTA: Self = Color::Idx(5);
  pub const CYAN: Self = Color::Idx(6);
  pub const WHITE: Self = Color::Idx(7);

  pub const BRIGHT_BLACK: Self = Color::Idx(8);
  pub const BRIGHT_RED: Self = Color::Idx(9);
  pub const BRIGHT_GREEN: Self = Color::Idx(10);
  pub const BRIGHT_YELLOW: Self = Color::Idx(11);
  pub const BRIGHT_BLUE: Self = Color::Idx(12);
  pub const BRIGHT_MAGENTA: Self = Color::Idx(13);
  pub const BRIGHT_CYAN: Self = Color::Idx(14);
  pub const BRIGHT_WHITE: Self = Color::Idx(15);
}

/// How many colors a terminal can show.
#[derive(
  Clone,
  Copy,
  Debug,
  Default,
  Eq,
  PartialEq,
  Hash,
  serde::Serialize,
  serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ColorDepth {
  Ansi16,
  Ansi256,
  #[default]
  TrueColor,
}

impl Color {
  /// Only RGB colors move; the others have no value to move from.
  pub fn blend(self, to: Rgb, amount: u8) -> Self {
    match self {
      Color::Rgb(r, g, b) => Rgb(r, g, b).blend(to, amount).into(),
      Color::Default | Color::Idx(_) => self,
    }
  }

  /// The nearest color a terminal of `depth` can show.
  pub fn downgrade(self, depth: ColorDepth) -> Self {
    match (depth, self) {
      (ColorDepth::TrueColor, c) => c,
      (_, Color::Default) => Color::Default,
      (ColorDepth::Ansi256, Color::Idx(i)) => Color::Idx(i),
      (ColorDepth::Ansi256, Color::Rgb(r, g, b)) => {
        Color::Idx(Rgb(r, g, b).to_ansi256())
      }
      (ColorDepth::Ansi16, Color::Idx(i)) if i < 16 => Color::Idx(i),
      (ColorDepth::Ansi16, Color::Idx(i)) => {
        Color::Idx(Palette::xterm().idx(i).to_ansi16())
      }
      (ColorDepth::Ansi16, Color::Rgb(r, g, b)) => {
        Color::Idx(Rgb(r, g, b).to_ansi16())
      }
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
  /// Moves `amount`/255 of the way to `to`.
  pub fn blend(self, to: Rgb, amount: u8) -> Rgb {
    let amount = amount as u16;
    let mix = |a: u8, b: u8| {
      ((a as u16 * (255 - amount) + b as u16 * amount) / 255) as u8
    };
    Rgb(mix(self.0, to.0), mix(self.1, to.1), mix(self.2, to.2))
  }

  fn distance(self, other: Rgb) -> u32 {
    let d = |a: u8, b: u8| {
      let d = a as i32 - b as i32;
      (d * d) as u32
    };
    d(self.0, other.0) + d(self.1, other.1) + d(self.2, other.2)
  }

  /// The nearest of the 256 xterm colors, by the cube and the grays.
  pub fn to_ansi256(self) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |v: u8| -> u8 {
      let mut best = 0;
      for (i, l) in LEVELS.iter().enumerate() {
        if (v as i32 - *l as i32).abs() < (v as i32 - LEVELS[best] as i32).abs()
        {
          best = i;
        }
      }
      best as u8
    };
    let (r, g, b) = (level(self.0), level(self.1), level(self.2));
    let cube = 16 + 36 * r + 6 * g + b;
    let cube_rgb =
      Rgb(LEVELS[r as usize], LEVELS[g as usize], LEVELS[b as usize]);
    // The gray ramp may be closer for near-gray colors.
    let avg = (self.0 as u32 + self.1 as u32 + self.2 as u32) / 3;
    let gray_i = ((avg as i32 - 8 + 5) / 10).clamp(0, 23) as u8;
    let gray_v = 8 + gray_i * 10;
    let gray_rgb = Rgb(gray_v, gray_v, gray_v);
    if self.distance(gray_rgb) < self.distance(cube_rgb) {
      232 + gray_i
    } else {
      cube
    }
  }

  /// The nearest of the 16 ANSI colors, as xterm shows them.
  pub fn to_ansi16(self) -> u8 {
    let palette = Palette::xterm();
    let mut best = 0;
    for i in 0..16u8 {
      if self.distance(palette.idx(i)) < self.distance(palette.idx(best)) {
        best = i;
      }
    }
    best
  }
}

impl From<Rgb> for Color {
  fn from(Rgb(r, g, b): Rgb) -> Self {
    Color::Rgb(r, g, b)
  }
}

pub use super::palette::Palette;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn downgrades() {
    assert_eq!(Rgb(0, 0, 0).to_ansi256(), 16);
    assert_eq!(Rgb(255, 255, 255).to_ansi256(), 231);
    assert_eq!(Rgb(128, 128, 128).to_ansi256(), 244);
    assert_eq!(Rgb(255, 0, 0).to_ansi256(), 196);
    assert_eq!(Rgb(255, 0, 0).to_ansi16(), 9);
    assert_eq!(Rgb(0, 0, 0).to_ansi16(), 0);
    assert_eq!(
      Color::Rgb(1, 2, 3).downgrade(ColorDepth::TrueColor),
      Color::Rgb(1, 2, 3)
    );
    assert_eq!(Color::Idx(196).downgrade(ColorDepth::Ansi16), Color::Idx(9));
    assert_eq!(Color::Default.downgrade(ColorDepth::Ansi16), Color::Default);
  }
}
