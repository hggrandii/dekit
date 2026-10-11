use tui_input::{Input, InputRequest};
use unicode_width::UnicodeWidthStr;

use crate::console::theme::BlockStyle;
use crate::term::{
  Grid,
  attrs::Attrs,
  grid::{Pos, Rect},
  key::{Key, KeyCode, KeyMods},
};

/// A prompt, the input, and a `selected/total` counter at the right;
/// returns the cursor position.
pub fn render_filter_row(
  grid: &mut Grid,
  mut row: Rect,
  prompt: &str,
  input: &Input,
  counter: Option<(usize, usize)>,
  style: &BlockStyle,
) -> Pos {
  grid.draw_text(row.take_left(prompt.width() as u16), prompt, style.key);
  if let Some((selected, total)) = counter {
    let counter = format!("{}/{}", selected + 1, total);
    grid.draw_text(row.take_right(counter.width() as u16), &counter, style.dim);
  }
  render_text_input(input, row.inner((0, 1, 0, 0)), grid, style.text)
}

/// Draws the input and returns the cursor position.
pub fn render_text_input(
  input: &Input,
  area: Rect,
  grid: &mut Grid,
  attrs: Attrs,
) -> Pos {
  let value = input.value();

  let left_trim = input.cursor().saturating_sub(area.width as usize);
  let (value, cursor) = if left_trim > 0 {
    let start =
      unicode_segmentation::UnicodeSegmentation::grapheme_indices(value, true)
        .nth(left_trim)
        .map_or_else(|| value.len(), |(len, _)| len);
    (&value[start..], input.cursor() - left_trim)
  } else {
    (value, input.cursor())
  };

  grid.fill_area(area, ' ', attrs);
  grid.draw_text(area, value, attrs);

  Pos {
    col: area.x + cursor as u16,
    row: area.y,
  }
}

pub fn to_input_request(key: &Key) -> Option<InputRequest> {
  use InputRequest::*;
  use KeyCode::*;
  match (key.code, key.mods) {
    (Backspace, KeyMods::NONE) | (Char('h'), KeyMods::CONTROL) => {
      Some(DeletePrevChar)
    }
    (Delete, KeyMods::NONE) => Some(DeleteNextChar),
    (Tab, KeyMods::NONE) => None,
    (Left, KeyMods::NONE) | (Char('b'), KeyMods::CONTROL) => Some(GoToPrevChar),
    (Left, KeyMods::CONTROL) | (Char('b'), KeyMods::META) => Some(GoToPrevWord),
    (Right, KeyMods::NONE) | (Char('f'), KeyMods::CONTROL) => {
      Some(GoToNextChar)
    }
    (Right, KeyMods::CONTROL) | (Char('f'), KeyMods::META) => {
      Some(GoToNextWord)
    }
    (Char('u'), KeyMods::CONTROL) => Some(DeleteLine),
    (Char('w'), KeyMods::CONTROL)
    | (Char('d'), KeyMods::META)
    | (Backspace, KeyMods::META)
    | (Backspace, KeyMods::ALT) => Some(DeletePrevWord),
    (Delete, KeyMods::CONTROL) => Some(DeleteNextWord),
    (Char('k'), KeyMods::CONTROL) => Some(DeleteTillEnd),
    (Char('a'), KeyMods::CONTROL) | (Home, KeyMods::NONE) => Some(GoToStart),
    (Char('e'), KeyMods::CONTROL) | (End, KeyMods::NONE) => Some(GoToEnd),
    (Char(c), KeyMods::NONE) | (Char(c), KeyMods::SHIFT) => Some(InsertChar(c)),
    (_, _) => None,
  }
}
