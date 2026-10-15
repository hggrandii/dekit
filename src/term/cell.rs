//! One cell of a grid: 8 bytes, a text handle and a style id.

use std::collections::HashMap;

use compact_str::CompactString;

use super::style::StyleId;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cell {
  pub text: CellText,
  pub style: StyleId,
}

impl Cell {
  pub const EMPTY: Cell = Cell {
    text: CellText::EMPTY,
    style: StyleId::DEFAULT,
  };

  pub fn new(text: CellText, style: StyleId) -> Cell {
    Cell { text, style }
  }

  /// A blank with a style: what erasing leaves behind.
  pub fn blank(style: StyleId) -> Cell {
    Cell {
      text: CellText::EMPTY,
      style,
    }
  }

  /// The blank left at the end of a row by a wide character that did not
  /// fit there: shown as a blank, but not part of the line's text.
  pub fn filler(style: StyleId) -> Cell {
    Cell {
      text: CellText::FILLER,
      style,
    }
  }

  pub fn width(&self) -> u16 {
    self.text.width()
  }

  pub fn is_wide(&self) -> bool {
    self.text.is_wide()
  }

  pub fn is_continuation(&self) -> bool {
    self.text.is_continuation()
  }

  pub fn has_contents(&self) -> bool {
    !self.text.is_empty() && !self.text.is_continuation()
  }
}

/// What a cell shows. The tag is in the top three bits:
///
/// - 0: empty (a blank).
/// - 1: one scalar; bits 0..21 the code point, 21..23 its width.
/// - 2: a grapheme cluster in the grid's `GraphemeTable`; bits 0..24 the
///   index, 24..26 the width.
/// - 3: the right half of a wide cell.
/// - 4: a kitty Unicode placeholder: bits 0..10 the placeholder slot,
///   10..19 the row, 19..28 the column in the image.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct CellText(u32);

const TAG_SHIFT: u32 = 29;
const TAG_EMPTY: u32 = 0;
const TAG_SCALAR: u32 = 1;
const TAG_GRAPHEME: u32 = 2;
const TAG_CONTINUATION: u32 = 3;
const TAG_IMAGE: u32 = 4;

pub const MAX_GRAPHEMES: usize = 1 << 24;
pub const MAX_PLACEHOLDER_SLOTS: u32 = 1 << 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextKind {
  Empty,
  Scalar(char),
  Grapheme(u32),
  Continuation,
  Image { slot: u32, row: u16, col: u16 },
}

impl CellText {
  pub const EMPTY: CellText = CellText(0);
  /// A blank that is padding, not text: see `Cell::filler`.
  pub const FILLER: CellText = CellText(1);
  pub const CONTINUATION: CellText = CellText(TAG_CONTINUATION << TAG_SHIFT);

  pub fn scalar(c: char, width: u8) -> CellText {
    CellText(
      (TAG_SCALAR << TAG_SHIFT) | ((width as u32 & 0b11) << 21) | (c as u32),
    )
  }

  pub fn grapheme(index: u32, width: u8) -> CellText {
    CellText(
      (TAG_GRAPHEME << TAG_SHIFT)
        | ((width as u32 & 0b11) << 24)
        | (index & 0x00FF_FFFF),
    )
  }

  pub fn image(slot: u32, row: u16, col: u16) -> CellText {
    CellText(
      (TAG_IMAGE << TAG_SHIFT)
        | ((col as u32 & 0x1FF) << 19)
        | ((row as u32 & 0x1FF) << 10)
        | (slot & 0x3FF),
    )
  }

  fn tag(self) -> u32 {
    self.0 >> TAG_SHIFT
  }

  pub fn kind(self) -> TextKind {
    match self.tag() {
      TAG_EMPTY => TextKind::Empty,
      TAG_SCALAR => TextKind::Scalar(
        char::from_u32(self.0 & 0x1F_FFFF).unwrap_or('\u{FFFD}'),
      ),
      TAG_GRAPHEME => TextKind::Grapheme(self.0 & 0x00FF_FFFF),
      TAG_CONTINUATION => TextKind::Continuation,
      _ => TextKind::Image {
        slot: self.0 & 0x3FF,
        row: ((self.0 >> 10) & 0x1FF) as u16,
        col: ((self.0 >> 19) & 0x1FF) as u16,
      },
    }
  }

  pub fn is_empty(self) -> bool {
    self.tag() == TAG_EMPTY
  }

  pub fn is_filler(self) -> bool {
    self == CellText::FILLER
  }

  pub fn is_continuation(self) -> bool {
    self.tag() == TAG_CONTINUATION
  }

  pub fn is_image(self) -> bool {
    self.tag() == TAG_IMAGE
  }

  /// Columns the cell takes: 0 for a blank or a continuation.
  pub fn width(self) -> u16 {
    match self.tag() {
      TAG_SCALAR => ((self.0 >> 21) & 0b11) as u16,
      TAG_GRAPHEME => ((self.0 >> 24) & 0b11) as u16,
      TAG_IMAGE => 1,
      _ => 0,
    }
  }

  pub fn is_wide(self) -> bool {
    self.width() >= 2
  }

  /// The same text at another width (a VS16 widening a narrow emoji).
  pub fn with_width(self, width: u8) -> CellText {
    match self.tag() {
      TAG_SCALAR => {
        CellText((self.0 & !(0b11 << 21)) | ((width as u32 & 0b11) << 21))
      }
      TAG_GRAPHEME => {
        CellText((self.0 & !(0b11 << 24)) | ((width as u32 & 0b11) << 24))
      }
      _ => self,
    }
  }

  /// The text as a string; scalars are written into `buf`.
  pub fn as_str<'a>(
    self,
    buf: &'a mut [u8; 4],
    graphemes: &'a GraphemeTable,
  ) -> &'a str {
    match self.kind() {
      TextKind::Empty | TextKind::Continuation => "",
      TextKind::Scalar(c) => c.encode_utf8(buf),
      TextKind::Grapheme(index) => graphemes.get(index),
      TextKind::Image { .. } => PLACEHOLDER_STR,
    }
  }
}

/// The kitty image placeholder character, as a cell's visible text.
pub const PLACEHOLDER: char = '\u{10EEEE}';
const PLACEHOLDER_STR: &str = "\u{10EEEE}";

/// Multi-scalar cell contents of one grid, interned. Append-only.
#[derive(Clone, Debug, Default)]
pub struct GraphemeTable {
  items: Vec<CompactString>,
  index: HashMap<CompactString, u32>,
}

impl CellText {
  /// The cell for `text` (nothing, one scalar, or a cluster) at `width`.
  /// A cluster is interned into `graphemes`, looked up first so a table
  /// shared with a frame is copied only for a new one.
  pub fn of(
    text: &str,
    width: u8,
    graphemes: &mut std::sync::Arc<GraphemeTable>,
  ) -> CellText {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
      (None, _) => CellText::EMPTY,
      (Some(c), None) => CellText::scalar(c, width),
      (Some(_), Some(_)) => {
        let index = match graphemes.lookup(text) {
          Some(index) => Some(index),
          None => std::sync::Arc::make_mut(graphemes).intern(text),
        };
        match index {
          Some(index) => CellText::grapheme(index, width),
          None => CellText::scalar('\u{FFFD}', 1),
        }
      }
    }
  }
}

impl GraphemeTable {
  /// Interns `text`; `None` when the table is full.
  pub fn intern(&mut self, text: &str) -> Option<u32> {
    if let Some(index) = self.index.get(text) {
      return Some(*index);
    }
    if self.items.len() >= MAX_GRAPHEMES {
      return None;
    }
    let index = self.items.len() as u32;
    let text = CompactString::from(text);
    self.items.push(text.clone());
    self.index.insert(text, index);
    Some(index)
  }

  pub fn lookup(&self, text: &str) -> Option<u32> {
    self.index.get(text).copied()
  }

  pub fn get(&self, index: u32) -> &str {
    self
      .items
      .get(index as usize)
      .map_or("\u{FFFD}", CompactString::as_str)
  }

  pub fn len(&self) -> usize {
    self.items.len()
  }

  pub fn is_empty(&self) -> bool {
    self.items.is_empty()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn encodings_round_trip() {
    let a = CellText::scalar('a', 1);
    assert_eq!(a.kind(), TextKind::Scalar('a'));
    assert_eq!(a.width(), 1);
    let wide = CellText::scalar('\u{6d4b}', 2);
    assert_eq!(wide.kind(), TextKind::Scalar('\u{6d4b}'));
    assert!(wide.is_wide());
    assert_eq!(wide.with_width(1).width(), 1);
    let max = CellText::scalar('\u{10FFFF}', 2);
    assert_eq!(max.kind(), TextKind::Scalar('\u{10FFFF}'));

    let g = CellText::grapheme(0xABCDEF, 2);
    assert_eq!(g.kind(), TextKind::Grapheme(0xABCDEF));
    assert_eq!(g.width(), 2);

    assert_eq!(CellText::CONTINUATION.kind(), TextKind::Continuation);
    assert_eq!(CellText::CONTINUATION.width(), 0);
    assert_eq!(CellText::EMPTY.kind(), TextKind::Empty);

    let img = CellText::image(1023, 300, 511);
    assert_eq!(
      img.kind(),
      TextKind::Image {
        slot: 1023,
        row: 300,
        col: 511
      }
    );
    assert_eq!(img.width(), 1);
    assert_eq!(std::mem::size_of::<Cell>(), 8);
  }

  #[test]
  fn graphemes_intern() {
    let mut table = GraphemeTable::default();
    let a = table.intern("e\u{301}").unwrap();
    assert_eq!(table.intern("e\u{301}"), Some(a));
    assert_eq!(table.get(a), "e\u{301}");
    assert_eq!(table.get(99), "\u{FFFD}");
    let mut buf = [0u8; 4];
    assert_eq!(
      CellText::grapheme(a, 1).as_str(&mut buf, &table),
      "e\u{301}"
    );
    assert_eq!(CellText::scalar('x', 1).as_str(&mut buf, &table), "x");
  }
}
