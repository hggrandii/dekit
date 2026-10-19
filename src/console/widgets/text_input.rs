use tui_input::{Input, InputRequest};
use unicode_width::UnicodeWidthStr;

use crate::console::theme::BlockStyle;
use crate::term::{
  Grid, Rgb,
  grid::{Pos, Rect},
  key::{Key, KeyCode, KeyMods},
  style::Style,
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
  attrs: Style,
) -> Pos {
  let (value, cursor) = visible(input, area.width);
  grid.fill_area(area, ' ', attrs);
  grid.draw_text(area, value, attrs);
  Pos {
    col: area.x + cursor as u16,
    row: area.y,
  }
}

/// Draws a text field: the input, or `placeholder` while it is empty, on
/// a line across `area` in `line`. A `suggestion`, the value the field
/// stands for while what is typed leads up to it, shows dim after what is
/// typed, as `web/` then `api`. Returns the cursor position, which is at
/// the start while nothing is typed.
pub fn render_text_field(
  input: &Input,
  area: Rect,
  grid: &mut Grid,
  placeholder: &str,
  suggestion: Option<&str>,
  style: &BlockStyle,
  line: Rgb,
) -> Pos {
  let (value, cursor, attrs) = if input.value().is_empty() {
    (suggestion.unwrap_or(placeholder), 0, style.placeholder)
  } else {
    let (value, cursor) = visible(input, area.width);
    (value, cursor, style.text)
  };
  // Blanks are in the text's color too: a terminal without underline
  // colors (tmux, unless told the outer terminal has them) draws the
  // line in the text's, and so in one color across.
  let attrs = attrs.set_underlined(true).ul_color(line.into());
  grid.fill_area(area, ' ', attrs);
  let drawn = grid.draw_text(area, value, attrs);
  let typed = input.value();
  if !typed.is_empty()
    && value == typed
    && let Some(rest) = suggestion.and_then(|s| s.strip_prefix(typed))
  {
    let mut after = area;
    after.take_left(drawn.width);
    let ghost = style.placeholder.set_underlined(true).ul_color(line.into());
    grid.draw_text(after, rest, ghost);
  }
  Pos {
    col: area.x + cursor as u16,
    row: area.y,
  }
}

/// What of the value fits `width` columns with the cursor in view, and
/// the cursor's column in it. A cursor after the text takes a column too.
fn visible(input: &Input, width: u16) -> (&str, usize) {
  let value = input.value();
  let scroll = input.visual_scroll(usize::from(width.saturating_sub(1)));
  // The bytes of the graphemes scrolled out of view.
  let start = unicode_segmentation::UnicodeSegmentation::graphemes(value, true)
    .scan(0, |cols, grapheme| {
      (*cols < scroll).then(|| {
        *cols += grapheme.width();
        grapheme.len()
      })
    })
    .sum::<usize>();
  (&value[start..], input.visual_cursor() - scroll)
}

/// Types pasted text into an input on one line: its line breaks as
/// spaces, a trailing one left out. Returns whether the value changed.
pub fn paste_line(input: &mut Input, text: &str) -> bool {
  let mut changed = false;
  for c in text.trim_end_matches(['\r', '\n']).chars() {
    let c = match c {
      '\r' => continue,
      '\n' => ' ',
      c => c,
    };
    changed |= input
      .handle(InputRequest::InsertChar(c))
      .is_some_and(|change| change.value);
  }
  changed
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::console::theme::Theme;
  use crate::term::common::Size;

  /// The cursor stays in the field, by columns: a wide character takes
  /// two.
  #[test]
  fn the_cursor_stays_in_the_field() {
    let at_end = |value: &str| {
      let input = Input::new(value.into());
      let (shown, cursor) = visible(&input, 10);
      (shown.to_string(), cursor)
    };
    assert_eq!(at_end("abcdefghi"), ("abcdefghi".into(), 9));
    assert_eq!(at_end("abcdefghij"), ("bcdefghij".into(), 9));
    assert_eq!(at_end("日本語日本"), ("本語日本".into(), 8));
    let mut input = Input::new("日本語日本".into());
    input.handle(InputRequest::GoToStart);
    assert_eq!(visible(&input, 10), ("日本語日本", 0));
  }

  #[test]
  fn a_field_line_is_one_color_without_underline_colors() {
    let theme = Theme::dark();
    let style = theme.block.modal();
    let line = theme.block.field_line;
    let mut grid = Grid::new(
      Size {
        width: 10,
        height: 1,
      },
      0,
    );
    let fgs = |grid: &mut Grid| {
      (0..10)
        .map(|col| {
          let cell = *grid.cell_mut(Pos { col, row: 0 }).unwrap();
          let shown = grid.style_of(cell.style);
          assert!(shown.underlined());
          assert_eq!(shown.ul, line.into());
          shown.fg
        })
        .collect::<Vec<_>>()
    };
    let area = Rect::new(0, 0, 10, 1);
    let mut input = Input::new(String::new());
    render_text_field(&input, area, &mut grid, "hint", None, &style, line);
    assert_eq!(fgs(&mut grid), vec![style.placeholder.fg; 10]);
    input = input.with_value("abc".into());
    render_text_field(&input, area, &mut grid, "hint", None, &style, line);
    assert_eq!(fgs(&mut grid), vec![style.text.fg; 10]);
  }

  #[test]
  fn a_suggestion_that_goes_on_from_the_value_shows_the_rest() {
    let theme = Theme::dark();
    let style = theme.block.modal();
    let mut grid = Grid::new(
      Size {
        width: 10,
        height: 1,
      },
      0,
    );
    let input = Input::new("web/".into());
    let line = theme.block.field_line;
    let area = Rect::new(0, 0, 10, 1);
    let field = |grid: &mut Grid, suggestion| {
      render_text_field(&input, area, grid, "10s", suggestion, &style, line);
    };
    field(&mut grid, Some("web/api"));
    assert_eq!(grid.row_text(0), "web/api");
    let mut at = |col| {
      let cell = *grid.cell_mut(Pos { col, row: 0 }).unwrap();
      grid.style_of(cell.style).fg
    };
    assert_eq!((at(3), at(4)), (style.text.fg, style.placeholder.fg));
    field(&mut grid, Some("db/api"));
    assert_eq!(grid.row_text(0), "web/");
    // A placeholder is not a suggestion.
    field(&mut grid, None);
    assert_eq!(grid.row_text(0), "web/");
  }
}
