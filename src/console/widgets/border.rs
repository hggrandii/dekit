use unicode_width::UnicodeWidthStr;

use crate::console::theme::BlockStyle;
use crate::term::{Grid, grid::Rect, style::Style};

/// Draws `pieces` at the right end of a border row with a blank cell on
/// each side, and returns where each piece went.
pub fn draw_on_border(
  grid: &mut Grid,
  row: &mut Rect,
  pieces: &[(&str, Style)],
  blank: Style,
) -> Vec<Rect> {
  let width = pieces.iter().map(|(text, _)| text.width()).sum::<usize>() + 2;
  let mut strip = row.take_right((width as u16).min(row.width));
  grid.fill_area(strip, ' ', blank);
  strip.take_left(1);
  pieces
    .iter()
    .map(|(text, attrs)| {
      let cells = strip.take_left(text.width() as u16);
      grid.draw_text(cells, text, *attrs);
      cells
    })
    .collect()
}

/// Key hints at the right end of `area`'s bottom border: each key, and
/// what it does.
pub fn draw_hints(
  grid: &mut Grid,
  area: Rect,
  hints: &[(&str, &str)],
  style: &BlockStyle,
) {
  let whats: Vec<String> = hints
    .iter()
    .enumerate()
    .map(|(i, (_, what))| match i + 1 < hints.len() {
      true => format!(" {what}  "),
      false => format!(" {what}"),
    })
    .collect();
  let pieces: Vec<_> = hints
    .iter()
    .zip(&whats)
    .flat_map(|((key, _), what)| {
      [(*key, style.key), (what.as_str(), style.dim)]
    })
    .collect();
  draw_on_border(grid, &mut bottom_hints_row(area), &pieces, style.border);
}

/// The bottom border row of `area`, without its corners and one cell
/// before the right one: where key hints go.
pub fn bottom_hints_row(area: Rect) -> Rect {
  border_row(area, area.bottom().saturating_sub(1))
}

/// The top border row of `area`, cut as `bottom_hints_row` cuts the
/// bottom one.
pub fn top_hints_row(area: Rect) -> Rect {
  border_row(area, area.y)
}

fn border_row(area: Rect, y: u16) -> Rect {
  let mut row = Rect {
    x: area.x + 1,
    y,
    width: area.width.saturating_sub(2),
    height: 1,
  };
  row.take_right(1);
  row
}
