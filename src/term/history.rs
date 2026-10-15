//! Scrollback: an append-only store of logical lines.
//!
//! A row that scrolls off the screen is committed here. Rows a program
//! wrapped are joined into one logical line, so a line is stored once
//! whatever the width was, and is wrapped again for whatever width it is
//! viewed at. A line is plain text plus run-length style runs: compact,
//! searchable as text, and serializable as is, so a store can page lines
//! to disk later behind the same interface.

use std::{collections::VecDeque, sync::Arc};

use super::{
  cell::{Cell, CellText, GraphemeTable},
  frame::Tables,
  row::Row,
  style::{Style, StyleId, StyleTable},
  unicode,
};

/// Lays cells out in rows of `width`: for each cell, in order, the row
/// and column it starts at. A cell that does not fit at the end of a row
/// starts the next one; the right half of a wide cell (width 0) is the
/// column after its left half. This is the one wrapping rule, for live
/// rows and history alike.
pub fn wrap<I: Iterator<Item = u16>>(widths: I, width: u16) -> Wrap<I> {
  Wrap {
    widths,
    width: width.max(1),
    row: 0,
    col: 0,
  }
}

pub struct Wrap<I> {
  widths: I,
  width: u16,
  row: usize,
  col: u16,
}

impl<I> Wrap<I> {
  /// The position after the last cell laid out so far.
  pub fn end(&self) -> (usize, u16) {
    (self.row, self.col)
  }
}

impl<I: Iterator<Item = u16>> Iterator for Wrap<I> {
  type Item = (usize, u16);

  fn next(&mut self) -> Option<(usize, u16)> {
    let w = self.widths.next()?;
    if w == 0 {
      return Some((self.row, self.col.saturating_sub(1)));
    }
    if self.col + w > self.width && self.col > 0 {
      self.row += 1;
      self.col = 0;
    }
    let pos = (self.row, self.col);
    self.col += w;
    Some(pos)
  }
}

/// One logical line. `text` holds each cell's text in order; a blank
/// cell is a space, the right half of a wide cell has no text. `cells`
/// says how many bytes of `text` each cell has, in runs; `styles` the
/// style of each cell, in runs.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Line {
  pub text: String,
  pub cells: Vec<(u8, u16)>,
  pub styles: Vec<(Style, u16)>,
}

/// A logical line stops growing here and the rest starts a new one.
pub const MAX_LINE_CELLS: usize = 1 << 16;
/// A cell's text longer than this is cut.
const MAX_CELL_BYTES: usize = 255;

impl Line {
  pub fn cell_count(&self) -> usize {
    self.cells.iter().map(|(_, n)| usize::from(*n)).sum()
  }

  fn push_run<T: PartialEq>(runs: &mut Vec<(T, u16)>, value: T) {
    push_run(runs, value)
  }

  fn widths(&self) -> impl Iterator<Item = u16> + '_ {
    self.cells().map(|cell| cell.width())
  }

  /// Appends the cells of a live row, up to `len`.
  pub fn push_cells(
    &mut self,
    cells: &[Cell],
    styles: &StyleTable,
    graphemes: &GraphemeTable,
  ) {
    let mut buf = [0u8; 4];
    for cell in cells {
      if cell.text.is_filler() {
        // Padding before a wrapped wide character, not text.
        continue;
      }
      if cell.is_continuation() {
        Self::push_run(&mut self.cells, 0);
      } else {
        // An image placeholder's picture does not follow it into
        // history; the cell is a blank there.
        let text = if cell.text.is_image() {
          " "
        } else {
          cell.text.as_str(&mut buf, graphemes)
        };
        let text = if text.is_empty() { " " } else { text };
        let text = if text.len() > MAX_CELL_BYTES {
          "\u{FFFD}"
        } else {
          text
        };
        self.text.push_str(text);
        Self::push_run(&mut self.cells, text.len() as u8);
      }
      Self::push_run(&mut self.styles, styles.get(cell.style));
    }
  }

  /// Iterates the cells: each one's text and style.
  pub fn cells(&self) -> LineCells<'_> {
    LineCells {
      line: self,
      byte: 0,
      cell_run: 0,
      cell_in_run: 0,
      style_run: 0,
      style_in_run: 0,
    }
  }

  /// Rows this line takes when wrapped at `width`.
  pub fn rows_at(&self, width: u16) -> usize {
    if self.cell_count() <= usize::from(width.max(1)) {
      return 1;
    }
    wrap(self.widths(), width)
      .last()
      .map_or(1, |(row, _)| row + 1)
  }

  /// The cell offset where row `row` of this line starts when wrapped at
  /// `width`; the end of the line for a row it does not have.
  pub fn offset_of_row(&self, width: u16, row: usize) -> usize {
    if row == 0 {
      return 0;
    }
    wrap(self.widths(), width)
      .position(|(r, _)| r == row)
      .unwrap_or_else(|| self.cell_count())
  }

  /// The row and column cell `offset` lands on when the line is wrapped
  /// at `width`. Past the line's cells, blanks lead to it, as a cursor
  /// left there would wrap.
  pub fn row_col_of(&self, width: u16, offset: usize) -> (usize, u16) {
    let width = width.max(1);
    let mut wrap = wrap(self.widths(), width);
    let mut count = 0;
    for pos in wrap.by_ref() {
      if count == offset {
        return pos;
      }
      count += 1;
    }
    let (row, col) = wrap.end();
    let extra = offset - count;
    let room = usize::from(width.saturating_sub(col));
    if extra < room {
      (row, col + extra as u16)
    } else {
      let extra = extra - room;
      let width = usize::from(width);
      (row + 1 + extra / width, (extra % width) as u16)
    }
  }

  /// The wrapped rows of this line at `width`, as live rows whose styles
  /// and clusters are interned into `tables` (looked up first: a table
  /// shared with a frame is copied only for something new).
  pub fn rows(&self, width: u16, tables: &mut Tables) -> Vec<Row> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut cells: Vec<Cell> = Vec::with_capacity(usize::from(width));
    let mut current = 0usize;
    let mut last_style = (Style::DEFAULT, StyleId::DEFAULT);
    for (cell, (row, _)) in self.cells().zip(wrap(self.widths(), width)) {
      let w = cell.width();
      if w == 0 {
        // Continuation: produced with its wide cell below.
        continue;
      }
      if row != current {
        // A wide cell that did not fit leaves a filler, not text.
        let pad = cells.last().map_or(StyleId::DEFAULT, |c| c.style);
        while cells.len() < usize::from(width) {
          cells.push(Cell::filler(pad));
        }
        let mut done = Row::from_cells(std::mem::take(&mut cells), width);
        done.wrap(true);
        rows.push(done);
        current = row;
      }
      let style = if cell.style == last_style.0 {
        last_style.1
      } else {
        let id = StyleTable::intern_in(&mut tables.styles, cell.style);
        last_style = (cell.style, id);
        id
      };
      let text = cell_text(cell.text, w, &mut tables.graphemes);
      cells.push(Cell::new(text, style));
      if w == 2 {
        cells.push(Cell::new(CellText::CONTINUATION, style));
      }
    }
    rows.push(Row::from_cells(cells, width));
    rows
  }
}

/// Extends the last run or starts a new one.
pub(crate) fn push_run<T: PartialEq>(runs: &mut Vec<(T, u16)>, value: T) {
  match runs.last_mut() {
    Some((last, n)) if *last == value && *n < u16::MAX => *n += 1,
    _ => runs.push((value, 1)),
  }
}

fn cell_text(
  text: &str,
  width: u16,
  graphemes: &mut Arc<GraphemeTable>,
) -> CellText {
  // A blank cell was stored as a space.
  if text == " " {
    return CellText::EMPTY;
  }
  CellText::of(text, width as u8, graphemes)
}

pub struct LineCell<'a> {
  pub text: &'a str,
  pub style: Style,
  /// 0 for the right half of a wide cell.
  width: u8,
}

impl LineCell<'_> {
  pub fn width(&self) -> u16 {
    u16::from(self.width)
  }
}

pub struct LineCells<'a> {
  line: &'a Line,
  byte: usize,
  cell_run: usize,
  cell_in_run: u16,
  style_run: usize,
  style_in_run: u16,
}

impl<'a> Iterator for LineCells<'a> {
  type Item = LineCell<'a>;

  fn next(&mut self) -> Option<LineCell<'a>> {
    let line = self.line;
    while self.cell_run < line.cells.len()
      && self.cell_in_run >= line.cells[self.cell_run].1
    {
      self.cell_run += 1;
      self.cell_in_run = 0;
    }
    let (bytes, _) = *line.cells.get(self.cell_run)?;
    self.cell_in_run += 1;
    while self.style_run < line.styles.len()
      && self.style_in_run >= line.styles[self.style_run].1
    {
      self.style_run += 1;
      self.style_in_run = 0;
    }
    let style = line
      .styles
      .get(self.style_run)
      .map_or(Style::DEFAULT, |(style, _)| *style);
    self.style_in_run += 1;
    let bytes = usize::from(bytes);
    let end = (self.byte + bytes).min(line.text.len());
    let text = line.text.get(self.byte..end).unwrap_or("");
    self.byte = end;
    let width = if bytes == 0 {
      0
    } else if text == " " {
      1
    } else {
      unicode::cluster_width(text).max(1)
    };
    Some(LineCell { text, style, width })
  }
}

/// Where committed lines go. The in-memory store keeps the last
/// `capacity` lines; a store that pages to disk implements the same.
pub trait LineStore: Send + Sync {
  fn len(&self) -> usize;
  fn is_empty(&self) -> bool {
    self.len() == 0
  }
  fn get(&self, index: usize) -> Option<Arc<Line>>;
  /// Appends a line; returns the line dropped at the front to make room,
  /// if one was.
  fn push(&mut self, line: Arc<Line>) -> Option<Arc<Line>>;
  /// Removes and returns the last line.
  fn pop(&mut self) -> Option<Arc<Line>>;
  fn clear(&mut self);
  fn capacity(&self) -> usize;
}

#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
  lines: VecDeque<Arc<Line>>,
  capacity: usize,
}

impl MemoryStore {
  pub fn new(capacity: usize) -> Self {
    MemoryStore {
      lines: VecDeque::new(),
      capacity,
    }
  }
}

impl LineStore for MemoryStore {
  fn len(&self) -> usize {
    self.lines.len()
  }

  fn get(&self, index: usize) -> Option<Arc<Line>> {
    self.lines.get(index).cloned()
  }

  fn push(&mut self, line: Arc<Line>) -> Option<Arc<Line>> {
    if self.capacity == 0 {
      return None;
    }
    self.lines.push_back(line);
    if self.lines.len() > self.capacity {
      self.lines.pop_front()
    } else {
      None
    }
  }

  fn pop(&mut self) -> Option<Arc<Line>> {
    self.lines.pop_back()
  }

  fn clear(&mut self) {
    self.lines.clear();
  }

  fn capacity(&self) -> usize {
    self.capacity
  }
}

/// The scrollback of one grid.
#[derive(Clone, Debug)]
pub struct History {
  store: MemoryStore,
  /// The last line continues on the screen's first row (the row that
  /// scrolled off was wrapped).
  open: bool,
  /// `(width, rows)`: how many rows the lines take at the width last
  /// asked about, kept up to date as lines come and go.
  rows: Option<(u16, usize)>,
}

impl History {
  pub fn new(capacity: usize) -> Self {
    History {
      store: MemoryStore::new(capacity),
      open: false,
      rows: None,
    }
  }

  pub fn len(&self) -> usize {
    self.store.len()
  }

  pub fn is_empty(&self) -> bool {
    self.store.len() == 0
  }

  pub fn capacity(&self) -> usize {
    self.store.capacity()
  }

  pub fn get(&self, index: usize) -> Option<Arc<Line>> {
    self.store.get(index)
  }

  pub fn is_open(&self) -> bool {
    self.open
  }

  /// The last line, which the screen's first row continues when `open`.
  pub fn last(&self) -> Option<Arc<Line>> {
    self
      .store
      .len()
      .checked_sub(1)
      .and_then(|i| self.store.get(i))
  }

  /// Commits a row that scrolled off: up to its last visible cell, or
  /// whole when the program wrapped it, since its trailing blanks are
  /// text positions the next row continues from.
  pub fn commit(&mut self, row: &Row, tables: &Tables) {
    if self.store.capacity() == 0 {
      return;
    }
    let wrapped = row.wrapped();
    let len = if wrapped {
      row.cells.len()
    } else {
      let styles = &tables.styles;
      usize::from(row.used_len(|id| styles.get(id).is_visible_blank()))
    };
    let cells = &row.cells[..len];
    // An open line grows: taken out (moved, not copied, since nothing
    // else holds it) and put back.
    let appending = self.open && self.store.len() > 0;
    let mut line = if appending {
      let last = self.store.pop().expect("has lines");
      self.rows_removed(&last);
      Arc::unwrap_or_clone(last)
    } else {
      Line::default()
    };
    line.push_cells(cells, &tables.styles, &tables.graphemes);
    let cell_count = line.cell_count();
    self.rows_added(&line);
    if let Some(dropped) = self.store.push(Arc::new(line)) {
      self.rows_removed(&dropped);
    }
    self.open = wrapped && cell_count < MAX_LINE_CELLS;
  }

  fn rows_added(&mut self, line: &Line) {
    if let Some((width, rows)) = &mut self.rows {
      *rows += line.rows_at(*width);
    }
  }

  fn rows_removed(&mut self, line: &Line) {
    if let Some((width, rows)) = &mut self.rows {
      *rows = rows.saturating_sub(line.rows_at(*width));
    }
  }

  /// Takes the open line back out of history, to be reflowed with the
  /// screen rows that continue it.
  pub fn pop_open(&mut self) -> Option<Arc<Line>> {
    if !self.open {
      return None;
    }
    self.pop_last()
  }

  /// Takes the last line out of history, to come back onto the screen.
  pub fn pop_last(&mut self) -> Option<Arc<Line>> {
    self.open = false;
    let line = self.store.pop()?;
    self.rows_removed(&line);
    Some(line)
  }

  pub fn clear(&mut self) {
    self.store.clear();
    self.open = false;
    if let Some((_, rows)) = &mut self.rows {
      *rows = 0;
    }
  }

  /// The width the grid shows this history at: its row count is kept up
  /// to date for it from here on.
  pub fn set_width(&mut self, width: u16) {
    if self.rows.is_some_and(|(w, _)| w == width) {
      return;
    }
    self.rows = Some((width, self.count_rows(width)));
  }

  /// Total rows the history takes at `width`: kept up to date for the
  /// grid's width, counted for any other.
  pub fn rows_at(&self, width: u16) -> usize {
    match self.rows {
      Some((w, rows)) if w == width => rows,
      _ => self.count_rows(width),
    }
  }

  fn count_rows(&self, width: u16) -> usize {
    (0..self.store.len())
      .filter_map(|i| self.store.get(i))
      .map(|line| line.rows_at(width))
      .sum()
  }

  /// The line, and the row in it, that is `rows_above` rows above the
  /// screen's first row at `width`; `None` when history has fewer rows
  /// (or `rows_above` is 0, a live row).
  pub fn locate(
    &self,
    rows_above: usize,
    width: u16,
  ) -> Option<(usize, usize)> {
    if rows_above == 0 {
      return None;
    }
    let mut covered = 0usize;
    let mut i = self.store.len();
    while i > 0 {
      i -= 1;
      covered += self.store.get(i).map_or(1, |line| line.rows_at(width));
      if covered >= rows_above {
        return Some((i, covered - rows_above));
      }
    }
    None
  }

  /// How many history rows lie below row `row` of line `index` at
  /// `width`: the line's own rows after it and every later line's.
  pub fn rows_below(&self, index: usize, row: usize, width: u16) -> usize {
    let own = self.store.get(index).map_or(1, |line| line.rows_at(width));
    let after: usize = (index + 1..self.store.len())
      .filter_map(|i| self.store.get(i))
      .map(|line| line.rows_at(width))
      .sum();
    after + own.saturating_sub(row + 1)
  }

  /// The `count` rows ending `offset` rows above the screen's first row,
  /// wrapped at `width`: the rows a view scrolled back by `offset` shows
  /// above the live rows it still shows. Fewer when history runs out at
  /// the top.
  pub fn rows_above(
    &self,
    offset: usize,
    count: usize,
    width: u16,
    tables: &mut Tables,
  ) -> Vec<Row> {
    // Walk back over lines until `offset` rows are covered, then build
    // forward.
    let mut covered = 0usize;
    let mut first = self.store.len();
    while first > 0 && covered < offset {
      first -= 1;
      covered += self.store.get(first).map_or(1, |line| line.rows_at(width));
    }
    let skip = covered.saturating_sub(offset);
    let mut rows = Vec::with_capacity(count);
    let mut skipped = 0;
    'lines: for i in first..self.store.len() {
      let Some(line) = self.store.get(i) else {
        continue;
      };
      for row in line.rows(width, tables) {
        if skipped < skip {
          skipped += 1;
          continue;
        }
        rows.push(row);
        if rows.len() == count {
          break 'lines;
        }
      }
    }
    rows
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::term::color::Color;

  fn tables() -> Tables {
    Tables::default()
  }

  fn row_of(text: &str, tables: &mut Tables) -> Row {
    let line = Line {
      text: text.to_string(),
      cells: text.chars().map(|c| (c.len_utf8() as u8, 1)).collect(),
      styles: vec![(Style::DEFAULT, text.chars().count() as u16)],
    };
    line.rows(text.chars().count() as u16, tables).remove(0)
  }

  #[test]
  fn lines_round_trip_through_rows() {
    let mut tables = tables();
    let red =
      Arc::make_mut(&mut tables.styles).intern(Style::default().fg(Color::RED));
    let mut row = Row::new(8);
    row.set(0, Cell::new(CellText::scalar('a', 1), StyleId::DEFAULT));
    row.set(1, Cell::new(CellText::scalar('\u{6d4b}', 2), red));
    row.set(2, Cell::new(CellText::CONTINUATION, red));
    let g = Arc::make_mut(&mut tables.graphemes)
      .intern("e\u{301}")
      .unwrap();
    row.set(3, Cell::new(CellText::grapheme(g, 1), StyleId::DEFAULT));
    row.set(5, Cell::new(CellText::scalar('z', 1), red));

    let mut history = History::new(10);
    history.commit(&row, &tables);
    let line = history.get(0).unwrap();
    assert_eq!(line.text, "a\u{6d4b}e\u{301} z");
    assert_eq!(line.cell_count(), 6);
    let cells: Vec<(&str, u16)> =
      line.cells().map(|c| (c.text, c.width())).collect();
    assert_eq!(
      cells,
      vec![
        ("a", 1),
        ("\u{6d4b}", 2),
        ("", 0),
        ("e\u{301}", 1),
        (" ", 1),
        ("z", 1)
      ]
    );
    let rows = line.rows(8, &mut tables);
    assert_eq!(rows.len(), 1);
    assert_eq!(&rows[0].cells[..6], &row.cells[..6]);
    assert_eq!(rows[0].cells[6], Cell::EMPTY);
  }

  #[test]
  fn wrapped_rows_join_and_rewrap() {
    let mut tables = tables();
    let mut history = History::new(10);
    let mut first = row_of("abcd", &mut tables);
    first.wrap(true);
    history.commit(&first, &tables);
    assert!(history.is_open());
    let second = row_of("ef  ", &mut tables);
    history.commit(&second, &tables);
    assert!(!history.is_open());
    assert_eq!(history.len(), 1);
    let line = history.get(0).unwrap();
    assert_eq!(line.text, "abcdef");
    assert_eq!(line.rows_at(4), 2);
    assert_eq!(line.rows_at(3), 2);
    assert_eq!(line.rows_at(6), 1);
    assert_eq!(line.rows_at(2), 3);
    let rows = line.rows(3, &mut tables);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].wrapped());
    assert!(!rows[1].wrapped());
  }

  #[test]
  fn blank_run_in_a_wrapped_row_is_kept() {
    let mut tables = tables();
    let mut history = History::new(10);
    // "ab" then two blanks at the end of a full wrapped row, continued
    // by "cd": the blanks are real text positions.
    let mut first = row_of("ab  ", &mut tables);
    first.wrap(true);
    history.commit(&first, &tables);
    let second = row_of("cd", &mut tables);
    history.commit(&second, &tables);
    assert_eq!(history.get(0).unwrap().text, "ab  cd");
  }

  #[test]
  fn rows_above_walks_back_over_wrapped_lines() {
    let mut tables = tables();
    let mut history = History::new(10);
    for text in ["one", "twotwo", "three"] {
      let row = row_of(text, &mut tables);
      history.commit(&row, &tables);
    }
    // At width 3: one=1 row, twotwo=2 rows, three=2 rows: 5 rows.
    assert_eq!(history.rows_at(3), 5);
    fn text(rows: &[Row], graphemes: &GraphemeTable) -> Vec<String> {
      let mut buf = [0u8; 4];
      rows
        .iter()
        .map(|row| {
          row
            .cells
            .iter()
            .map(|c| {
              let s = c.text.as_str(&mut buf, graphemes);
              if s.is_empty() {
                " ".to_string()
              } else {
                s.to_string()
              }
            })
            .collect::<String>()
        })
        .collect()
    }
    let rows = history.rows_above(2, 2, 3, &mut tables);
    assert_eq!(text(&rows, &tables.graphemes), vec!["thr", "ee "]);
    let rows = history.rows_above(4, 3, 3, &mut tables);
    assert_eq!(text(&rows, &tables.graphemes), vec!["two", "two", "thr"]);
    // More than there is: starts at the top.
    let rows = history.rows_above(9, 2, 3, &mut tables);
    assert_eq!(text(&rows, &tables.graphemes), vec!["one", "two"]);
  }

  #[test]
  fn positions_within_lines() {
    // a b 测 (wide) c d: six cells.
    let line = Line {
      text: "ab\u{6d4b}cd".to_string(),
      cells: vec![(1, 2), (3, 1), (0, 1), (1, 2)],
      styles: vec![(Style::DEFAULT, 6)],
    };
    // At width 3: "ab", "测c", "d".
    assert_eq!(line.rows_at(3), 3);
    assert_eq!(line.offset_of_row(3, 0), 0);
    assert_eq!(line.offset_of_row(3, 1), 2);
    assert_eq!(line.offset_of_row(3, 2), 5);
    assert_eq!(line.offset_of_row(3, 7), 6);
    assert_eq!(line.row_col_of(3, 0), (0, 0));
    assert_eq!(line.row_col_of(3, 2), (1, 0));
    assert_eq!(line.row_col_of(3, 3), (1, 1));
    assert_eq!(line.row_col_of(3, 4), (1, 2));
    assert_eq!(line.row_col_of(3, 5), (2, 0));
    // Past the end, blanks lead on and wrap.
    assert_eq!(line.row_col_of(3, 6), (2, 1));
    assert_eq!(line.row_col_of(3, 8), (3, 0));
    assert_eq!(line.row_col_of(3, 9), (3, 1));
  }

  #[test]
  fn locating_rows_in_history() {
    let mut tables = tables();
    let mut history = History::new(10);
    for text in ["one", "twotwo", "three"] {
      let row = row_of(text, &mut tables);
      history.commit(&row, &tables);
    }
    // At width 3: one=1 row, twotwo=2 rows, three=2 rows.
    assert_eq!(history.locate(0, 3), None);
    assert_eq!(history.locate(1, 3), Some((2, 1)));
    assert_eq!(history.locate(2, 3), Some((2, 0)));
    assert_eq!(history.locate(3, 3), Some((1, 1)));
    assert_eq!(history.locate(5, 3), Some((0, 0)));
    assert_eq!(history.locate(6, 3), None);
    assert_eq!(history.rows_below(2, 1, 3), 0);
    assert_eq!(history.rows_below(2, 0, 3), 1);
    assert_eq!(history.rows_below(1, 0, 3), 3);
    assert_eq!(history.rows_below(0, 0, 3), 4);
  }

  #[test]
  fn placeholder_cells_commit_as_blanks() {
    let mut tables = tables();
    let mut history = History::new(10);
    let mut row = Row::new(4);
    row.set(0, Cell::new(CellText::scalar('a', 1), StyleId::DEFAULT));
    row.set(1, Cell::new(CellText::image(0, 0, 0), StyleId::DEFAULT));
    history.commit(&row, &tables);
    assert_eq!(history.get(0).unwrap().text, "a ");
    let rows = history.get(0).unwrap().rows(4, &mut tables);
    assert_eq!(rows[0].cells[1], Cell::EMPTY);
  }

  #[test]
  fn row_counts_follow_the_lines() {
    let mut tables = tables();
    let mut history = History::new(3);
    history.set_width(3);
    for text in ["one", "twotwo", "three"] {
      let row = row_of(text, &mut tables);
      history.commit(&row, &tables);
    }
    assert_eq!(history.rows_at(3), 5);
    // A line committed, one dropped at the front (capacity 3).
    let row = row_of("fourfour", &mut tables);
    history.commit(&row, &tables);
    assert_eq!(history.rows_at(3), 7);
    // A new open line drops "twotwo" (2 rows) and adds one; the row
    // that continues it makes it two rows.
    let mut open = row_of("abc", &mut tables);
    open.wrap(true);
    history.commit(&open, &tables);
    assert_eq!(history.rows_at(3), 6);
    history.commit(&row_of("d", &mut tables), &tables);
    assert_eq!(history.rows_at(3), 7);
    // Another width is counted, not tracked: three, fourfour, abcd.
    assert_eq!(history.rows_at(4), 2 + 2 + 1);
    history.clear();
    assert_eq!(history.rows_at(3), 0);
  }

  #[test]
  fn capacity_drops_the_oldest() {
    let mut tables = tables();
    let mut history = History::new(2);
    for text in ["a", "b", "c"] {
      let row = row_of(text, &mut tables);
      history.commit(&row, &tables);
    }
    assert_eq!(history.len(), 2);
    assert_eq!(history.get(0).unwrap().text, "b");
    let none = History::new(0);
    assert_eq!(none.capacity(), 0);
  }
}
