use super::color::{Color, Rgb};

/// How a screen's colors become RGB: the default colors and the 16 ANSI
/// ones; 16-231 are the standard cube and 232-255 the grays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Palette {
  pub fg: Rgb,
  pub bg: Rgb,
  pub ansi: [Rgb; 16],
}

impl Palette {
  pub fn fg(&self, color: Color) -> Rgb {
    self.resolve(color, self.fg)
  }

  pub fn bg(&self, color: Color) -> Rgb {
    self.resolve(color, self.bg)
  }

  fn resolve(&self, color: Color, default: Rgb) -> Rgb {
    match color {
      Color::Default => default,
      Color::Idx(i) => self.idx(i),
      Color::Rgb(r, g, b) => Rgb(r, g, b),
    }
  }

  pub fn idx(&self, i: u8) -> Rgb {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match i {
      0..=15 => self.ansi[i as usize],
      16..=231 => {
        let i = (i - 16) as usize;
        Rgb(LEVELS[i / 36], LEVELS[i / 6 % 6], LEVELS[i % 6])
      }
      232..=255 => {
        let v = 8 + (i - 232) * 10;
        Rgb(v, v, v)
      }
    }
  }

  /// xterm's default colors: what programs assume when nothing says
  /// otherwise, and what color downgrades measure against.
  pub fn xterm() -> Palette {
    Palette {
      fg: Rgb(0xe5, 0xe5, 0xe5),
      bg: Rgb(0x00, 0x00, 0x00),
      ansi: [
        Rgb(0x00, 0x00, 0x00),
        Rgb(0xcd, 0x00, 0x00),
        Rgb(0x00, 0xcd, 0x00),
        Rgb(0xcd, 0xcd, 0x00),
        Rgb(0x00, 0x00, 0xee),
        Rgb(0xcd, 0x00, 0xcd),
        Rgb(0x00, 0xcd, 0xcd),
        Rgb(0xe5, 0xe5, 0xe5),
        Rgb(0x7f, 0x7f, 0x7f),
        Rgb(0xff, 0x00, 0x00),
        Rgb(0x00, 0xff, 0x00),
        Rgb(0xff, 0xff, 0x00),
        Rgb(0x5c, 0x5c, 0xff),
        Rgb(0xff, 0x00, 0xff),
        Rgb(0x00, 0xff, 0xff),
        Rgb(0xff, 0xff, 0xff),
      ],
    }
  }

  /// The colors a screen answers OSC 4/10/11 queries with when nothing
  /// else is known: a dark theme matching the console's.
  pub fn dark() -> Palette {
    Palette {
      fg: Rgb(0xd1, 0xd7, 0xe0),
      bg: Rgb(0x16, 0x1b, 0x22),
      ansi: [
        Rgb(0x48, 0x4f, 0x58),
        Rgb(0xff, 0x7b, 0x72),
        Rgb(0x3f, 0xb9, 0x50),
        Rgb(0xd2, 0x99, 0x22),
        Rgb(0x58, 0xa6, 0xff),
        Rgb(0xbc, 0x8c, 0xff),
        Rgb(0x39, 0xc5, 0xcf),
        Rgb(0xb1, 0xba, 0xc4),
        Rgb(0x6e, 0x76, 0x81),
        Rgb(0xff, 0xa1, 0x98),
        Rgb(0x56, 0xd3, 0x64),
        Rgb(0xe3, 0xb3, 0x41),
        Rgb(0x79, 0xc0, 0xff),
        Rgb(0xd2, 0xa8, 0xff),
        Rgb(0x56, 0xd4, 0xdd),
        Rgb(0xff, 0xff, 0xff),
      ],
    }
  }
}
