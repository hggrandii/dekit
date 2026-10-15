//! One row of the live screen.

use super::{
  cell::{Cell, GraphemeTable},
  style::StyleId,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
  pub cells: Vec<Cell>,
  wrapped: bool,
}

impl Row {
  pub fn new(cols: u16) -> Self {
    Row {
      cells: vec![Cell::EMPTY; usize::from(cols)],
      wrapped: false,
    }
  }

  pub fn blank(cols: u16, style: StyleId) -> Self {
    Row {
      cells: vec![Cell::blank(style); usize::from(cols)],
      wrapped: false,
    }
  }

  pub fn cols(&self) -> u16 {
    self.cells.len() as u16
  }

  pub fn clear(&mut self, style: StyleId) {
    for cell in &mut self.cells {
      *cell = Cell::blank(style);
    }
    self.wrapped = false;
  }

  pub fn get(&self, col: u16) -> Option<Cell> {
    self.cells.get(usize::from(col)).copied()
  }

  pub fn get_mut(&mut self, col: u16) -> Option<&mut Cell> {
    self.cells.get_mut(usize::from(col))
  }

  /// Puts `cell` at `col`, keeping wide cells whole: a wide cell it
  /// overwrites loses its other half too.
  pub fn set(&mut self, col: u16, cell: Cell) {
    let i = usize::from(col);
    if i >= self.cells.len() {
      return;
    }
    self.clear_wide(col, self.cells[i].style);
    self.cells[i] = cell;
  }

  pub fn insert(&mut self, col: u16, cell: Cell) {
    let i = usize::from(col);
    if i > self.cells.len() {
      return;
    }
    self.cells.insert(i, cell);
    self.wrapped = false;
  }

  pub fn remove(&mut self, col: u16) {
    let i = usize::from(col);
    if i >= self.cells.len() {
      return;
    }
    self.clear_wide(col, self.cells[i].style);
    self.cells.remove(i);
    self.wrapped = false;
  }

  pub fn erase(&mut self, col: u16, style: StyleId) {
    let i = usize::from(col);
    if i >= self.cells.len() {
      return;
    }
    let wide = self.cells[i].is_wide();
    self.clear_wide(col, style);
    self.cells[i] = Cell::blank(style);
    if usize::from(col) + if wide { 2 } else { 1 } == self.cells.len() {
      self.wrapped = false;
    }
  }

  /// ICH within `col..=right`: `count` blanks enter at `col` and as many
  /// cells leave at `right`.
  pub fn insert_blanks(
    &mut self,
    col: u16,
    count: u16,
    right: u16,
    style: StyleId,
  ) {
    let (col, right) = (usize::from(col), usize::from(right));
    if col > right || right >= self.cells.len() {
      return;
    }
    let count = usize::from(count).min(right + 1 - col);
    if count == 0 {
      return;
    }
    // A wide cell split by the insertion point loses both halves.
    if self.cells[col].is_continuation() {
      self.clear_wide(col as u16, style);
      self.cells[col] = Cell::blank(style);
    }
    self.cells[col..=right].rotate_right(count);
    for cell in &mut self.cells[col..col + count] {
      *cell = Cell::blank(style);
    }
    // A wide cell now cut at the right end loses its half too.
    if self.cells[right].is_wide() {
      self.cells[right] = Cell::blank(style);
    }
    self.wrapped = false;
  }

  /// DCH within `col..=right`: `count` cells leave at `col` and as many
  /// blanks enter at `right`.
  pub fn delete_cells(
    &mut self,
    col: u16,
    count: u16,
    right: u16,
    style: StyleId,
  ) {
    let (col, right) = (usize::from(col), usize::from(right));
    if col > right || right >= self.cells.len() {
      return;
    }
    let count = usize::from(count).min(right + 1 - col);
    if count == 0 {
      return;
    }
    // Wide cells cut at either end of the deleted range lose their other
    // half.
    self.clear_wide(col as u16, style);
    self.clear_wide((col + count - 1) as u16, style);
    self.cells[col..=right].rotate_left(count);
    for cell in &mut self.cells[right + 1 - count..=right] {
      *cell = Cell::blank(style);
    }
    self.wrapped = false;
  }

  pub fn wrap(&mut self, wrap: bool) {
    self.wrapped = wrap;
  }

  pub fn wrapped(&self) -> bool {
    self.wrapped
  }

  /// Blanks the other half of the wide cell at `col`, if it is one half.
  pub fn clear_wide(&mut self, col: u16, style: StyleId) {
    let i = usize::from(col);
    let Some(cell) = self.cells.get(i) else {
      return;
    };
    let other = if cell.is_wide() {
      i + 1
    } else if cell.is_continuation() && i > 0 {
      i - 1
    } else {
      return;
    };
    if let Some(other) = self.cells.get_mut(other) {
      *other = Cell::blank(if style == StyleId::DEFAULT {
        other.style
      } else {
        style
      });
    }
  }

  /// Cells up to the last one that shows something: text, or a blank
  /// with a visible style. Blanks past it are the row's padding.
  pub fn used_len(&self, visible: impl Fn(StyleId) -> bool) -> u16 {
    let mut len = self.cells.len();
    while len > 0 {
      let cell = self.cells[len - 1];
      if cell.has_contents() || visible(cell.style) {
        break;
      }
      len -= 1;
    }
    len as u16
  }

  /// A row of `cols` blanks whose cells are the given ones, left-aligned.
  /// A wide cell whose right half falls off the end stays: on a screen
  /// too narrow for it nothing else can be done, and it must survive the
  /// next reflow.
  pub fn from_cells(mut cells: Vec<Cell>, cols: u16) -> Self {
    cells.resize(usize::from(cols), Cell::EMPTY);
    Row {
      cells,
      wrapped: false,
    }
  }

  /// Appends the text of columns `cols` to `out`: blanks as spaces, the
  /// right halves of wide cells and wrap fillers skipped.
  pub fn push_text(
    &self,
    cols: std::ops::Range<u16>,
    graphemes: &GraphemeTable,
    out: &mut String,
  ) {
    let mut buf = [0u8; 4];
    for col in cols {
      let Some(cell) = self.get(col) else {
        break;
      };
      if cell.is_continuation() || cell.text.is_filler() {
        continue;
      }
      let text = cell.text.as_str(&mut buf, graphemes);
      out.push_str(if text.is_empty() { " " } else { text });
    }
  }
}
