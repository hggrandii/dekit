//! A dialog over a list of changes: one row per change, each taken or
//! left, and what the selected one changes in a pane under them. The
//! reload and write-back dialogs are this, each over its own plan.

use unicode_width::UnicodeWidthStr;

use crate::console::{
  keymap::Keymap,
  theme::{BlockStyle, Theme},
  widgets::{
    border::{draw_hints, draw_on_border, top_hints_row},
    list::{ListState, paint_row},
  },
};
use crate::term::{
  Grid,
  grid::Rect,
  key::{Key, KeyCode, KeyMods},
  mouse::MouseEvent,
};

use super::modal::{Modal, ModalResult};

/// What a checklist dialog lists.
pub trait Checklist: Send {
  const TITLE: &'static str;
  /// What the dialog says when there is nothing to list.
  const NOTHING: &'static str;
  /// Rows of the pane under the list.
  const DETAIL: u16;

  fn len(&self) -> usize;
  fn row(&self, index: usize) -> CheckRow;
  /// Takes or leaves a row.
  fn toggle(&mut self, index: usize);
  /// A key of its own, looked at before the list's.
  fn key(&mut self, key: &Key) -> Option<ModalResult>;
  /// The keys it takes, as `(key, what it does)`, for the bottom border.
  fn hints(&self) -> Vec<(&'static str, &'static str)>;
  /// What the dialog draws in the pane for a row.
  fn detail(
    &self,
    index: usize,
    grid: &mut Grid,
    area: Rect,
    style: BlockStyle,
  );

  /// For the top border.
  fn summary(&self) -> Option<String> {
    None
  }

  /// A line under the rows.
  fn footer(&self) -> Option<String> {
    None
  }
}

/// How a row reads: `[x] ~ path  what  (note)`.
/// Field keys as a row lists them.
pub fn listed<'a>(keys: impl Iterator<Item = &'a str>) -> String {
  keys.collect::<Vec<_>>().join(", ")
}

/// How wide a detail's column of keys is.
pub fn key_column<'a>(keys: impl Iterator<Item = &'a str>) -> u16 {
  keys.map(UnicodeWidthStr::width).max().unwrap_or(0).min(18) as u16
}

pub struct CheckRow {
  pub check: Check,
  /// What taking it does, as a diff marks it.
  pub mark: &'static str,
  pub path: String,
  pub what: String,
  /// Why it is not taken unless asked.
  pub note: Option<String>,
}

pub enum Check {
  Off,
  On,
  /// Taken in part.
  Partly,
}

pub struct ChecklistModal<C> {
  pub items: C,
  list: ListState,
}

impl<C: Checklist> ChecklistModal<C> {
  pub fn new(items: C) -> Self {
    let mut list = ListState::new(1);
    list.reset(items.len());
    ChecklistModal { items, list }
  }

  fn is_empty(&self) -> bool {
    self.items.len() == 0
  }
}

impl<C: Checklist> Modal for ChecklistModal<C> {
  fn handle_key(&mut self, key: &Key) -> ModalResult {
    if self.is_empty() {
      return match key.code {
        KeyCode::Enter | KeyCode::Esc => ModalResult::Close,
        _ => ModalResult::Keep,
      };
    }
    if let Some(result) = self.items.key(key) {
      return result;
    }
    match (key.code, key.mods) {
      (KeyCode::Esc, KeyMods::NONE) => return ModalResult::Close,
      (KeyCode::Char(' '), KeyMods::NONE) => {
        if let Some(index) = self.list.selected() {
          self.items.toggle(index);
        }
      }
      _ => {
        self.list.handle_vi_key(key);
      }
    }
    ModalResult::Keep
  }

  fn handle_mouse(&mut self, mouse: &MouseEvent) -> ModalResult {
    if let Some(index) = self.list.handle_mouse(mouse)
      && let Some(row) = self.list.row_area(index)
      // The box at the row's start takes or leaves it.
      && (mouse.x as u16) < row.x + 4
    {
      self.items.toggle(index);
    }
    ModalResult::Keep
  }

  fn size(&self, frame: Rect) -> (u16, u16) {
    if self.is_empty() {
      let width = C::NOTHING.width() as u16 + 6;
      return (width.min(frame.width), 5);
    }
    let rows = (self.items.len() as u16).min(12);
    let footer = u16::from(self.items.footer().is_some());
    let height = 2 + rows + footer + 1 + C::DETAIL;
    (84.min(frame.width), height.min(frame.height))
  }

  fn render(&mut self, grid: &mut Grid, _keymap: &Keymap, theme: &Theme) {
    let area = self.area(grid.area());
    let style = theme.block.modal();
    let mut block = style.draw(grid, area);
    block.title(&format!(" {} ", C::TITLE), style.title);
    let mut inner = block.inner().inner((0, 1));
    if self.is_empty() {
      inner.take_top(1);
      grid.draw_text(inner, C::NOTHING, style.text);
      return;
    }
    if let Some(summary) = self.items.summary() {
      let mut top = top_hints_row(area);
      let pieces = [(summary.as_str(), style.dim)];
      draw_on_border(grid, &mut top, &pieces, style.border);
    }

    let footer = self.items.footer();
    let mut detail = inner;
    let list_area = detail.take_top(
      inner
        .height
        .saturating_sub(C::DETAIL + 1)
        .saturating_sub(u16::from(footer.is_some())),
    );
    let rows: Vec<CheckRow> =
      (0..self.items.len()).map(|i| self.items.row(i)).collect();
    let path_w = rows
      .iter()
      .map(|r| r.path.width())
      .max()
      .unwrap_or(0)
      .clamp(6, 28) as u16;
    for list_row in self.list.rows(list_area) {
      let row = &rows[list_row.index];
      let style =
        paint_row(grid, list_row.area, list_row.selected, style, &theme.block);
      let mut rest = list_row.area;
      let check = match row.check {
        Check::Off => "[ ]",
        Check::On => "[x]",
        Check::Partly => "[~]",
      };
      grid.draw_text(rest.take_left(4), check, style.key);
      grid.draw_text(rest.take_left(2), row.mark, style.key);
      grid.draw_text(rest.take_left(path_w), &row.path, style.text);
      rest.take_left(2);
      let shown = grid.draw_text(rest, &row.what, style.dim);
      rest.take_left(shown.width);
      if let Some(note) = &row.note {
        rest.take_left(2);
        grid.draw_text(rest, &format!("({note})"), style.error);
      }
    }
    if let Some(footer) = footer {
      grid.draw_text(detail.take_top(1), &footer, style.dim);
    }
    let sep = detail.take_top(1);
    grid.separator(area, sep.y, style.border);
    if let Some(index) = self.list.selected() {
      self.items.detail(index, grid, detail, style);
    }

    draw_hints(grid, area, &self.items.hints(), &style);
  }
}
