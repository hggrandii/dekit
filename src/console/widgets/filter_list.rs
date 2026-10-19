//! A filter typed over a list it narrows, as the pickers have it.

use tui_input::Input;

use crate::console::theme::{BlockStyle, BlockTheme};
use crate::console::widgets::{
  list::{ListState, paint_row},
  text_input::{paste_line, render_filter_row, to_input_request},
};
use crate::term::{
  Grid,
  grid::{Pos, Rect},
  key::Key,
  mouse::MouseEvent,
};

pub struct FilterList {
  input: Input,
  pub list: ListState,
  /// The owner's items the filter keeps, by index, in order.
  shown: Vec<usize>,
}

/// What input did to a filter list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterInput {
  /// The cursor moved.
  Moved,
  /// The filter changed: the owner shows what it keeps now.
  Edited,
  /// An item was clicked.
  Picked(usize),
  Ignored,
}

impl Default for FilterList {
  fn default() -> Self {
    FilterList {
      input: Input::default(),
      list: ListState::new(0),
      shown: Vec::new(),
    }
  }
}

impl FilterList {
  pub fn value(&self) -> &str {
    self.input.value()
  }

  pub fn set_value(&mut self, value: String) {
    self.input = Input::new(value);
  }

  /// Shows these items, the first one selected.
  pub fn show(&mut self, shown: Vec<usize>) {
    self.shown = shown;
    self.list.reset(self.shown.len());
  }

  pub fn shown(&self) -> &[usize] {
    &self.shown
  }

  /// The item under the cursor.
  pub fn selected(&self) -> Option<usize> {
    self.list.selected().map(|i| self.shown[i])
  }

  /// The list's keys move; the rest edit the filter. Enter is the
  /// owner's.
  pub fn handle_key(&mut self, key: &Key) -> FilterInput {
    if self.list.handle_key(key) {
      return FilterInput::Moved;
    }
    match to_input_request(key).and_then(|req| self.input.handle(req)) {
      Some(change) if change.value => FilterInput::Edited,
      Some(_) | None => FilterInput::Ignored,
    }
  }

  pub fn handle_paste(&mut self, text: &str) -> FilterInput {
    if paste_line(&mut self.input, text) {
      FilterInput::Edited
    } else {
      FilterInput::Ignored
    }
  }

  /// The pointer moves the cursor and a click picks.
  pub fn handle_mouse(&mut self, mouse: &MouseEvent) -> FilterInput {
    let before = self.list.selected();
    match self.list.handle_mouse(mouse) {
      Some(i) => FilterInput::Picked(self.shown[i]),
      None if self.list.selected() != before => FilterInput::Moved,
      None => FilterInput::Ignored,
    }
  }

  /// The filter row, `prompt` first and a counter last; returns where
  /// the cursor goes.
  pub fn render_input(
    &self,
    grid: &mut Grid,
    row: Rect,
    prompt: &str,
    style: &BlockStyle,
  ) -> Pos {
    let counter = self.list.selected().map(|i| (i, self.shown.len()));
    render_filter_row(grid, row, prompt, &self.input, counter, style)
  }

  /// Lays the rows in view out in `area`, the selected one lit and
  /// barred: each item, where it goes, and the style to draw it in.
  pub fn rows(
    &mut self,
    grid: &mut Grid,
    area: Rect,
    style: BlockStyle,
    theme: &BlockTheme,
  ) -> Vec<(usize, Rect, BlockStyle)> {
    let rows: Vec<_> = self.list.rows(area).collect();
    rows
      .into_iter()
      .map(|row| {
        let style = paint_row(grid, row.area, row.selected, style, theme);
        if row.selected {
          grid.draw_text(row.area, "\u{258e}", style.key);
        }
        (self.shown[row.index], row.area, style)
      })
      .collect()
  }
}
