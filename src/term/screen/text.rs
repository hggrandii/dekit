//! Printable text: how scalars become cells.

use crate::term::{
  Color,
  cell::{CellText, PLACEHOLDER, TextKind},
  grid::Pos,
  image::placeholder,
  unicode::{self, Attach, Prev},
};

use super::Screen;

impl Screen {
  /// A run of printable text from the scanner.
  pub(super) fn text_run(&mut self, text: &str) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
      let c = text[i..].chars().next().expect("non-empty");
      i += c.len_utf8();
      self.put_char(c);

      // Only after plain ASCII: anything else may absorb what follows
      // (a ZWJ, an emoji taking a variation selector).
      if !c.is_ascii_graphic() && c != ' '
        || self.modes.insert
        || self.gl_set() != super::charset::CharSet::Ascii
      {
        continue;
      }
      // Blit the following run of plain ASCII cells within the current
      // row; wrapping and wide chars go through the per-char path.
      let ascii_len = bytes[i..]
        .iter()
        .take_while(|b| (0x20..=0x7E).contains(*b))
        .count();
      if ascii_len == 0 {
        continue;
      }
      let grid = self.active_grid();
      let pos = grid.pos();
      let (_, right) = grid.margins();
      if pos.col > right {
        // Pending wrap; the per-char path handles it.
        continue;
      }
      let room = usize::from(right + 1 - pos.col);
      let n = ascii_len.min(room);
      let style = self.style_id();
      let grid = self.grid_mut();
      grid.write_ascii_row(pos, &bytes[i..i + n], style);
      grid.col_inc(n as u16);
      self.last_char = Some(bytes[i + n - 1] as char);
      self.placeholder_marks = 0;
      i += n;
    }
  }

  /// One scalar, through the active charset.
  pub(super) fn put_char(&mut self, c: char) {
    let set = match self.single_shift.take() {
      Some(g) => self.charsets[usize::from(g)],
      None => self.gl_set(),
    };
    let c = if (c as u32) < 0x80 {
      set.translate(c)
    } else {
      c
    };
    if unicode::scalar_width(c).is_none() {
      return;
    }
    if c == PLACEHOLDER {
      self.write_placeholder();
      return;
    }
    // A placeholder's diacritics say which part of the image it shows.
    if let Some(mark) = placeholder::mark_index(c)
      && let Some(pos) = self.prev_pos()
      && self.mark_placeholder(pos, mark)
    {
      return;
    }
    let prev = self.prev_cell();
    match unicode::attach(c, prev.1) {
      Attach::New(width) => self.write_new(c, width),
      Attach::Join => {
        if let Some(pos) = prev.0 {
          self.join(pos, c, None);
        }
      }
      Attach::JoinWide => {
        if let Some(pos) = prev.0 {
          self.join(pos, c, Some(2));
        }
      }
    }
  }

  /// A kitty image placeholder (U+10EEEE): the cell shows part of the
  /// image whose id is in the foreground color (and placement id in the
  /// underline color). Which part, the diacritics after it say; without
  /// them it continues the cell to its left.
  fn write_placeholder(&mut self) {
    let image_id = color_id(self.style.fg);
    let placement_id = color_id(self.style.ul);
    let style = self.style_id();
    let autowrap = self.modes.autowrap;
    let grid = self.grid_mut();
    let (_, right) = grid.margins();
    if grid.pos().col > right {
      if autowrap {
        grid.col_wrap(1, style);
      } else {
        grid.col_stay();
      }
    }
    let pos = grid.pos();
    let Some(slot) = grid.placeholder_slot(image_id, placement_id) else {
      return;
    };
    // Continue the placeholder to the left, if it is one of the same
    // image: next column, same row.
    let (row, col) = match pos.col.checked_sub(1).and_then(|c| {
      grid.drawing_cell(Pos {
        row: pos.row,
        col: c,
      })
    }) {
      Some(cell) => match cell.text.kind() {
        TextKind::Image { slot: s, row, col } if s == slot => {
          (row, col.saturating_add(1))
        }
        _ => (0, 0),
      },
      None => (0, 0),
    };
    grid.write_cell(pos, CellText::image(slot, row, col), style);
    grid.col_inc(1);
    self.placeholder_marks = 0;
    self.last_char = None;
  }

  /// Applies a row/column/id diacritic to the placeholder at `pos`;
  /// false when that cell is not a placeholder.
  fn mark_placeholder(&mut self, pos: Pos, mark: u16) -> bool {
    let marks = self.placeholder_marks;
    let grid = self.grid_mut();
    let Some(cell) = grid.drawing_cell(pos) else {
      return false;
    };
    let TextKind::Image { slot, row, col } = cell.text.kind() else {
      return false;
    };
    let text = match marks {
      0 => CellText::image(slot, mark, col),
      1 => CellText::image(slot, row, mark),
      2 => {
        // The third diacritic is the image id's high byte.
        let Some((image_id, placement_id)) = grid.placeholder_ids(slot) else {
          return true;
        };
        let image_id = (image_id & 0x00FF_FFFF) | ((mark as u32 & 0xFF) << 24);
        match grid.placeholder_slot(image_id, placement_id) {
          Some(slot) => CellText::image(slot, row, col),
          None => return true,
        }
      }
      _ => return true,
    };
    grid.write_cell(pos, text, cell.style);
    self.placeholder_marks += 1;
    true
  }

  /// Where the cell before the cursor is: on this row, or at the end of
  /// the previous row when it wrapped here.
  fn prev_pos(&self) -> Option<Pos> {
    let grid = self.active_grid();
    let pos = grid.pos();
    let (left, _) = grid.margins();
    let width = grid.size().width;
    if pos.col > left && pos.col <= width {
      Some(Pos {
        row: pos.row,
        col: pos.col - 1,
      })
    } else if pos.row > 0
      && pos.col == left
      && grid
        .drawing_row(pos.row - 1)
        .is_some_and(|row| row.wrapped())
    {
      Some(Pos {
        row: pos.row - 1,
        col: width - 1,
      })
    } else {
      None
    }
  }

  /// The cell before the cursor, if there is one to attach to.
  fn prev_cell(&self) -> (Option<Pos>, Prev) {
    let grid = self.active_grid();
    let Some(mut cell_pos) = self.prev_pos() else {
      return (None, Prev::default());
    };
    let Some(mut cell) = grid.drawing_cell(cell_pos) else {
      return (None, Prev::default());
    };
    if cell.is_continuation() && cell_pos.col > 0 {
      cell_pos.col -= 1;
      cell = match grid.drawing_cell(cell_pos) {
        Some(cell) => cell,
        None => return (None, Prev::default()),
      };
    }
    if !cell.has_contents() || cell.text.is_image() {
      return (None, Prev::default());
    }
    let mut buf = [0u8; 4];
    let text = grid.text_of(cell, &mut buf);
    let mut chars = text.chars();
    let first = chars.next().unwrap_or(' ');
    let last = text.chars().next_back().unwrap_or(' ');
    let lone_ri =
      unicode::is_regional_indicator(first) && chars.next().is_none();
    (
      Some(cell_pos),
      Prev {
        exists: true,
        after_zwj: last == unicode::ZWJ,
        lone_ri,
        emoji_base: unicode::is_emoji_base(first),
        width: cell.width() as u8,
      },
    )
  }

  /// Appends `c` to the cell at `pos`, widening it when asked.
  fn join(&mut self, pos: Pos, c: char, widen: Option<u8>) {
    let grid = self.grid_mut();
    let Some(cell) = grid.drawing_cell(pos) else {
      return;
    };
    let mut buf = [0u8; 4];
    let mut text = String::from(grid.text_of(cell, &mut buf));
    if text.len() + c.len_utf8() > unicode::MAX_CLUSTER_BYTES {
      // A cluster this long is not text any more; the mark is dropped.
      return;
    }
    text.push(c);
    let width = widen.unwrap_or(cell.width().max(1) as u8);
    let new_text = grid.cell_text(&text, width);
    let style = cell.style;
    let size = grid.size();
    if width == 2 && cell.width() < 2 {
      // The cell grows to the right; a cursor sitting there moves on.
      if pos.col + 1 >= size.width {
        // No room: keep it narrow.
        let narrow = new_text.with_width(1);
        grid.write_cell(pos, narrow, style);
        return;
      }
      let cursor = grid.pos();
      grid.write_cell(pos, new_text, style);
      if cursor.row == pos.row && cursor.col == pos.col + 1 {
        grid.col_inc(1);
      }
    } else {
      grid.write_cell(pos, new_text, style);
    }
  }

  /// A new cell at the cursor: wrap, insert, write, advance.
  fn write_new(&mut self, c: char, width: u8) {
    let style = self.style_id();
    let autowrap = self.modes.autowrap;
    let insert = self.modes.insert;
    let grid = self.grid_mut();
    let pos = grid.pos();
    let (_, right) = grid.margins();
    let w = u16::from(width);
    if pos.col + w > right + 1 {
      if autowrap {
        // The line continues on the next row: text filled this one (the
        // cursor is past the right margin) or a wide character does not
        // fit in what is left of it.
        grid.col_wrap(w, style);
      } else {
        grid.col_stay();
        if w == 2 && grid.pos().col + 1 > right {
          // A wide char on the last column does not fit: skip it, as
          // xterm does.
          return;
        }
      }
    }
    if insert {
      grid.insert_cells(w, style);
    }
    let pos = grid.pos();
    let text = CellText::scalar(c, width);
    grid.write_cell(pos, text, style);
    grid.col_inc(w);
    self.last_char = Some(c);
    self.placeholder_marks = 0;
  }

  /// REP: repeats the last printed character.
  pub(super) fn repeat_last(&mut self, count: u16) {
    let Some(c) = self.last_char else {
      return;
    };
    for _ in 0..count.max(1).min(self.active_grid().size().width) {
      let width = unicode::scalar_width(c).unwrap_or(1).max(1);
      self.write_new(c, width);
    }
  }
}

/// The 24-bit number a color carries for the placeholder protocol: an
/// indexed color is the index, an RGB color its three bytes.
fn color_id(color: Color) -> u32 {
  match color {
    Color::Default => 0,
    Color::Idx(i) => u32::from(i),
    Color::Rgb(r, g, b) => {
      (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
    }
  }
}
