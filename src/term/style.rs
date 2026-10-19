//! What a cell looks like: colors, attributes, and the hyperlink it is
//! part of. Styles are interned per grid; a cell carries a `StyleId`.

use std::{collections::HashMap, sync::Arc};

use super::color::Color;

/// Underline shapes of SGR `4:n`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum Underline {
  #[default]
  None = 0,
  Single = 1,
  Double = 2,
  Curly = 3,
  Dotted = 4,
  Dashed = 5,
}

impl Underline {
  pub fn from_u8(n: u8) -> Underline {
    match n {
      1 => Underline::Single,
      2 => Underline::Double,
      3 => Underline::Curly,
      4 => Underline::Dotted,
      5 => Underline::Dashed,
      _ => Underline::None,
    }
  }
}

/// A hyperlink in a grid's `LinkTable`; 0 is no link.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct LinkId(pub u32);

impl LinkId {
  pub const NONE: LinkId = LinkId(0);

  pub fn is_none(self) -> bool {
    self.0 == 0
  }
}

const BOLD: u16 = 1 << 0;
const ITALIC: u16 = 1 << 1;
/// Any underline; the shape is in `UL_SHIFT`.
const UNDERLINE: u16 = 1 << 2;
const INVERSE: u16 = 1 << 3;
// Above the byte the first binaries read: a snapshot with any of these
// in use does not fit it, so such a binary refuses the snapshot rather
// than lose them (`upgrade::snapshot`).
const DIM: u16 = 1 << 8;
const BLINK: u16 = 1 << 9;
const HIDDEN: u16 = 1 << 10;
const STRIKE: u16 = 1 << 11;
const OVERLINE: u16 = 1 << 12;
const UL_SHIFT: u16 = 13;
const UL_MASK: u16 = 0b111 << UL_SHIFT;

/// The bits of the legacy `mode` byte in snapshots: bold, italic,
/// underline, inverse.
pub const LEGACY_MODE_MASK: u16 = BOLD | ITALIC | UNDERLINE | INVERSE;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Style {
  pub fg: Color,
  pub bg: Color,
  /// Underline color; `Default` is the foreground.
  pub ul: Color,
  pub link: LinkId,
  /// Attribute bits; read and set them through the methods.
  pub attrs: u16,
}

impl Style {
  pub const DEFAULT: Style = Style {
    fg: Color::Default,
    bg: Color::Default,
    ul: Color::Default,
    link: LinkId::NONE,
    attrs: 0,
  };

  pub fn fg(mut self, color: Color) -> Self {
    self.fg = color;
    self
  }

  pub fn bg(mut self, color: Color) -> Self {
    self.bg = color;
    self
  }

  pub fn ul_color(mut self, color: Color) -> Self {
    self.ul = color;
    self
  }

  pub fn with_link(mut self, link: LinkId) -> Self {
    self.link = link;
    self
  }

  fn flag(&self, bit: u16) -> bool {
    self.attrs & bit != 0
  }

  fn set_flag(mut self, bit: u16, on: bool) -> Self {
    if on {
      self.attrs |= bit;
    } else {
      self.attrs &= !bit;
    }
    self
  }

  pub fn bold(&self) -> bool {
    self.flag(BOLD)
  }

  pub fn set_bold(self, on: bool) -> Self {
    self.set_flag(BOLD, on)
  }

  pub fn dim(&self) -> bool {
    self.flag(DIM)
  }

  pub fn set_dim(self, on: bool) -> Self {
    self.set_flag(DIM, on)
  }

  pub fn italic(&self) -> bool {
    self.flag(ITALIC)
  }

  pub fn set_italic(self, on: bool) -> Self {
    self.set_flag(ITALIC, on)
  }

  pub fn underline(&self) -> Underline {
    if self.flag(UNDERLINE) {
      match Underline::from_u8(((self.attrs & UL_MASK) >> UL_SHIFT) as u8) {
        Underline::None => Underline::Single,
        shape => shape,
      }
    } else {
      Underline::None
    }
  }

  pub fn underlined(&self) -> bool {
    self.flag(UNDERLINE)
  }

  pub fn set_underline(self, shape: Underline) -> Self {
    let mut style = self;
    style.attrs &= !UL_MASK;
    match shape {
      Underline::None => style.set_flag(UNDERLINE, false),
      shape => {
        style.attrs |= (shape as u16) << UL_SHIFT;
        style.set_flag(UNDERLINE, true)
      }
    }
  }

  /// Plain single underline on or off, for UI code.
  pub fn set_underlined(self, on: bool) -> Self {
    self.set_underline(if on {
      Underline::Single
    } else {
      Underline::None
    })
  }

  pub fn inverse(&self) -> bool {
    self.flag(INVERSE)
  }

  pub fn set_inverse(self, on: bool) -> Self {
    self.set_flag(INVERSE, on)
  }

  pub fn blink(&self) -> bool {
    self.flag(BLINK)
  }

  pub fn set_blink(self, on: bool) -> Self {
    self.set_flag(BLINK, on)
  }

  pub fn hidden(&self) -> bool {
    self.flag(HIDDEN)
  }

  pub fn set_hidden(self, on: bool) -> Self {
    self.set_flag(HIDDEN, on)
  }

  pub fn strike(&self) -> bool {
    self.flag(STRIKE)
  }

  pub fn set_strike(self, on: bool) -> Self {
    self.set_flag(STRIKE, on)
  }

  pub fn overline(&self) -> bool {
    self.flag(OVERLINE)
  }

  pub fn set_overline(self, on: bool) -> Self {
    self.set_flag(OVERLINE, on)
  }

  /// All attribute bits, for snapshots and fast comparisons.
  pub fn attrs_bits(&self) -> u16 {
    self.attrs
  }

  pub fn from_attrs_bits(mut self, bits: u16) -> Self {
    self.attrs = bits;
    self
  }

  /// True when the cell shows something even without text: a
  /// background, an underline, inverse video.
  pub fn is_visible_blank(&self) -> bool {
    self.bg != Color::Default
      || self.inverse()
      || self.underlined()
      || self.strike()
      || self.overline()
  }
}

/// Index of a style in a grid's `StyleTable`; 0 is `Style::DEFAULT`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct StyleId(pub u32);

impl StyleId {
  pub const DEFAULT: StyleId = StyleId(0);
}

/// Interned styles of one grid. Append-only; ids never move.
#[derive(Clone, Debug)]
pub struct StyleTable {
  styles: Vec<Style>,
  index: HashMap<Style, StyleId>,
}

impl Default for StyleTable {
  fn default() -> Self {
    StyleTable::new()
  }
}

impl StyleTable {
  pub fn new() -> Self {
    let mut table = StyleTable {
      styles: Vec::with_capacity(64),
      index: HashMap::with_capacity(64),
    };
    table.intern(Style::DEFAULT);
    table
  }

  pub fn intern(&mut self, style: Style) -> StyleId {
    if let Some(id) = self.index.get(&style) {
      return *id;
    }
    let id = StyleId(self.styles.len() as u32);
    self.styles.push(style);
    self.index.insert(style, id);
    id
  }

  /// A table with every style mapped through `f`, ids kept. Two styles
  /// may map to one; the index then finds the last, which only matters
  /// for interning into the result, and nothing does.
  pub fn map(&self, mut f: impl FnMut(Style) -> Style) -> StyleTable {
    let styles: Vec<Style> = self.styles.iter().map(|s| f(*s)).collect();
    let index = styles
      .iter()
      .enumerate()
      .map(|(i, s)| (*s, StyleId(i as u32)))
      .collect();
    StyleTable { styles, index }
  }

  /// Interns into a table that frames may share: looked up first, so
  /// the table is copied (`Arc::make_mut`) only for a new style.
  pub fn intern_in(table: &mut Arc<StyleTable>, style: Style) -> StyleId {
    match table.lookup(&style) {
      Some(id) => id,
      None => Arc::make_mut(table).intern(style),
    }
  }

  /// Whether `style` is already interned, without interning it.
  pub fn lookup(&self, style: &Style) -> Option<StyleId> {
    self.index.get(style).copied()
  }

  pub fn get(&self, id: StyleId) -> Style {
    self.styles.get(id.0 as usize).copied().unwrap_or_default()
  }

  pub fn len(&self) -> usize {
    self.styles.len()
  }

  pub fn is_empty(&self) -> bool {
    self.styles.is_empty()
  }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Link {
  pub uri: String,
  /// The `id` parameter of OSC 8, grouping cells of one link.
  pub id: Option<String>,
}

/// Hyperlinks of one grid. Append-only; ids never move.
#[derive(Clone, Debug, Default)]
pub struct LinkTable {
  links: Vec<Link>,
  index: HashMap<Link, LinkId>,
}

/// Beyond this many distinct links new ones are dropped.
const MAX_LINKS: usize = 1 << 16;

impl LinkTable {
  pub fn intern(&mut self, link: Link) -> LinkId {
    if let Some(id) = self.index.get(&link) {
      return *id;
    }
    if self.links.len() >= MAX_LINKS {
      return LinkId::NONE;
    }
    let id = LinkId(self.links.len() as u32 + 1);
    self.links.push(link.clone());
    self.index.insert(link, id);
    id
  }

  pub fn get(&self, id: LinkId) -> Option<&Link> {
    if id.is_none() {
      return None;
    }
    self.links.get(id.0 as usize - 1)
  }

  pub fn len(&self) -> usize {
    self.links.len()
  }

  pub fn is_empty(&self) -> bool {
    self.links.is_empty()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn flags_round_trip() {
    let style = Style::default()
      .set_bold(true)
      .set_dim(true)
      .set_underline(Underline::Curly)
      .set_strike(true);
    assert!(style.bold() && style.dim() && style.strike());
    assert_eq!(style.underline(), Underline::Curly);
    assert!(!style.italic());
    let plain = style.set_underline(Underline::None);
    assert_eq!(plain.underline(), Underline::None);
    assert!(!plain.underlined());
    assert_eq!(
      Style::default().set_underlined(true).underline(),
      Underline::Single
    );
  }

  #[test]
  fn tables_intern() {
    let mut styles = StyleTable::new();
    assert_eq!(styles.intern(Style::DEFAULT), StyleId::DEFAULT);
    let red = Style::default().fg(Color::RED);
    let id = styles.intern(red);
    assert_eq!(styles.intern(red), id);
    assert_eq!(styles.get(id), red);
    assert_eq!(styles.get(StyleId(99)), Style::DEFAULT);

    let mut links = LinkTable::default();
    let a = links.intern(Link {
      uri: "http://a".into(),
      id: None,
    });
    let b = links.intern(Link {
      uri: "http://a".into(),
      id: Some("x".into()),
    });
    assert_ne!(a, b);
    assert_eq!(links.get(a).unwrap().uri, "http://a");
    assert!(links.get(LinkId::NONE).is_none());
  }
}
