//! How text turns into cells: which scalars start a new cell, which join
//! the one before, and how wide a cell is.
//!
//! The policy is the one modern terminals agree on: a scalar of width 0
//! (combining marks, variation selectors, ZWJ) joins the previous cell;
//! what follows a ZWJ joins too; an emoji modifier joins its base; a
//! pair of regional indicators is one flag; VS16 widens a narrow emoji to
//! two columns. DECSET 2027 does not change this: the screen always
//! clusters, and reports the mode as set so programs know it.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

/// The most text one cell holds: a ZWJ emoji sequence with skin tones is
/// about 40 bytes; beyond this, further marks are dropped, so a run of
/// combining characters cannot grow the grapheme table without bound.
pub const MAX_CLUSTER_BYTES: usize = 64;

pub const ZWJ: char = '\u{200D}';
pub const VS15: char = '\u{FE0E}';
pub const VS16: char = '\u{FE0F}';

pub fn is_regional_indicator(c: char) -> bool {
  ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

pub fn is_emoji_modifier(c: char) -> bool {
  ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

/// Width of a scalar on its own; `None` for a control character.
pub fn scalar_width(c: char) -> Option<u8> {
  if (c as u32) < 0x20 || (0x7F..0xA0).contains(&(c as u32)) {
    return None;
  }
  match c.width() {
    Some(0) => Some(0),
    Some(1) => Some(1),
    Some(_) => Some(2),
    // Private use and unassigned: narrow.
    None => Some(1),
  }
}

/// How a scalar relates to the cell written just before it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Attach {
  /// A new cell of this width.
  New(u8),
  /// Joins the previous cell without changing its width.
  Join,
  /// Joins the previous cell and makes it two columns wide.
  JoinWide,
}

/// What the previous cell looked like, as far as attachment cares.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Prev {
  /// There is a previous cell to join on this row.
  pub exists: bool,
  /// Its last scalar was a ZWJ.
  pub after_zwj: bool,
  /// It is one lone regional indicator.
  pub lone_ri: bool,
  /// Its first scalar is an emoji-presentation candidate (so VS16 and
  /// skin tones apply to it).
  pub emoji_base: bool,
  pub width: u8,
}

pub fn attach(c: char, prev: Prev) -> Attach {
  let width = scalar_width(c).unwrap_or(0);
  if !prev.exists {
    return Attach::New(width.max(if width == 0 { 1 } else { width }));
  }
  if prev.after_zwj && (width == 0 || is_emoji_base(c)) {
    return if width == 2 && prev.width < 2 {
      Attach::JoinWide
    } else {
      Attach::Join
    };
  }
  if c == VS16 {
    return if prev.width < 2 && prev.emoji_base {
      Attach::JoinWide
    } else {
      Attach::Join
    };
  }
  if width == 0 {
    return Attach::Join;
  }
  if is_regional_indicator(c) && prev.lone_ri {
    return Attach::JoinWide;
  }
  if is_emoji_modifier(c) && prev.emoji_base {
    return if prev.width < 2 {
      Attach::JoinWide
    } else {
      Attach::Join
    };
  }
  Attach::New(width)
}

/// Whether a scalar can take emoji presentation (so a following VS16 or
/// skin-tone modifier applies to it). Covers the blocks text-default
/// emoji live in; wide emoji are already two columns.
pub fn is_emoji_base(c: char) -> bool {
  matches!(c as u32,
    0x203C | 0x2049 | 0x2122 | 0x2139 | 0x2194..=0x2199 | 0x21A9..=0x21AA
    | 0x231A..=0x231B | 0x2328 | 0x23CF | 0x23E9..=0x23F3 | 0x23F8..=0x23FA
    | 0x24C2 | 0x25AA..=0x25AB | 0x25B6 | 0x25C0 | 0x25FB..=0x25FE
    | 0x2600..=0x27BF | 0x2934..=0x2935 | 0x2B05..=0x2B07 | 0x2B1B..=0x2B1C
    | 0x2B50 | 0x2B55 | 0x3030 | 0x303D | 0x3297 | 0x3299
    | 0x1F000..=0x1FAFF)
}

/// Width of UI text: its clusters' widths, as `Grid::draw_text` lays
/// them out.
pub fn str_width(text: &str) -> u16 {
  text
    .graphemes(true)
    .map(|g| u16::from(cluster_width(g).max(1)))
    .sum()
}

/// Width of a whole cluster written at once (UI text, history
/// reconstruction): the width the screen would give it scalar by scalar.
pub fn cluster_width(text: &str) -> u8 {
  let mut width = 0u8;
  let mut prev = Prev::default();
  for c in text.chars() {
    match attach(c, prev) {
      Attach::New(w) => {
        width = w;
        prev = Prev {
          exists: true,
          after_zwj: false,
          lone_ri: is_regional_indicator(c),
          emoji_base: is_emoji_base(c),
          width: w,
        };
      }
      Attach::Join => {
        prev.lone_ri = false;
      }
      Attach::JoinWide => {
        width = 2;
        prev.width = 2;
        prev.lone_ri = false;
      }
    }
    prev.after_zwj = c == ZWJ;
  }
  width
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn widths() {
    assert_eq!(cluster_width("a"), 1);
    assert_eq!(cluster_width("\u{6d4b}"), 2);
    assert_eq!(cluster_width("e\u{301}"), 1);
    // Heart with VS16 is wide; without it, narrow.
    assert_eq!(cluster_width("\u{2764}"), 1);
    assert_eq!(cluster_width("\u{2764}\u{FE0F}"), 2);
    // A flag.
    assert_eq!(cluster_width("\u{1F1FA}\u{1F1F8}"), 2);
    // Family: ZWJ sequence of wide emoji.
    assert_eq!(
      cluster_width("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"),
      2
    );
    // Skin tone on a wide base stays wide.
    assert_eq!(cluster_width("\u{1F44B}\u{1F3FD}"), 2);
    assert_eq!(scalar_width('\x1b'), None);
    assert_eq!(scalar_width(' '), Some(1));
  }

  #[test]
  fn attachment() {
    let none = Prev::default();
    assert_eq!(attach('a', none), Attach::New(1));
    // A lone combining mark starts a cell of its own.
    assert_eq!(attach('\u{301}', none), Attach::New(1));
    let after_a = Prev {
      exists: true,
      width: 1,
      ..none
    };
    assert_eq!(attach('\u{301}', after_a), Attach::Join);
    assert_eq!(attach('b', after_a), Attach::New(1));
    // VS16 after plain text does not widen it.
    assert_eq!(attach(VS16, after_a), Attach::Join);
    let after_heart = Prev {
      exists: true,
      emoji_base: true,
      width: 1,
      ..none
    };
    assert_eq!(attach(VS16, after_heart), Attach::JoinWide);
    let after_ri = Prev {
      exists: true,
      lone_ri: true,
      width: 1,
      ..none
    };
    assert_eq!(attach('\u{1F1F8}', after_ri), Attach::JoinWide);
    let after_zwj = Prev {
      exists: true,
      after_zwj: true,
      width: 2,
      ..none
    };
    assert_eq!(attach('\u{1F469}', after_zwj), Attach::Join);
    // Only emoji join through a ZWJ; text after a stray one starts a cell.
    assert_eq!(attach('a', after_zwj), Attach::New(1));
    assert_eq!(attach('\u{301}', after_zwj), Attach::Join);
  }
}
