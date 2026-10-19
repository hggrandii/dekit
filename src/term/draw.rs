//! Drawing into a grid as a UI surface: text, fills, borders.

use unicode_segmentation::UnicodeSegmentation;

use super::{
  cell::{Cell, CellText},
  grid::{Grid, Pos, Rect},
  line_symbols::{VERTICAL_LEFT, VERTICAL_RIGHT},
  style::Style,
  unicode,
};

pub enum BorderType {
  Thick,
  Plain,
  Rounded,
}

impl BorderType {
  pub fn chars(&self) -> BorderChars {
    match self {
      BorderType::Thick => BorderChars {
        tl: '┏',
        tr: '┓',
        br: '┛',
        bl: '┗',
        hor: '━',
        ver: '┃',
      },
      BorderType::Plain => BorderChars {
        tl: '┌',
        tr: '┐',
        br: '┘',
        bl: '└',
        hor: '─',
        ver: '│',
      },
      BorderType::Rounded => BorderChars {
        tl: '╭',
        tr: '╮',
        br: '╯',
        bl: '╰',
        hor: '─',
        ver: '│',
      },
    }
  }
}

pub struct BorderChars {
  pub tl: char,
  pub tr: char,
  pub br: char,
  pub bl: char,
  pub hor: char,
  pub ver: char,
}

impl Grid {
  /// A horizontal line across a block drawn at `area`, at row `y`,
  /// joined to its border on both sides.
  pub fn separator(&mut self, area: Rect, y: u16, style: Style) {
    let mut row = Rect {
      y,
      height: 1,
      ..area
    };
    self.draw_text(row.take_left(1), VERTICAL_RIGHT, style);
    self.draw_text(row.take_right(1), VERTICAL_LEFT, style);
    self.fill_area(row, '─', style);
  }

  /// Puts one character at `pos`.
  pub fn put_char(&mut self, pos: Pos, c: char, style: Style) {
    let width = unicode::scalar_width(c).unwrap_or(1).max(1);
    let style = self.intern_style(style);
    self.write_cell(pos, CellText::scalar(c, width), style);
  }

  /// Copies a cell from another grid's frame: the text re-interned, the
  /// style as given.
  pub fn put_cell(&mut self, pos: Pos, text: &str, width: u16, style: Style) {
    let style = self.intern_style(style);
    let text = self.cell_text(text, width as u8);
    self.write_cell(pos, text, style);
  }

  pub fn draw_block(&mut self, area: Rect, chars: &BorderChars, style: Style) {
    if area.width < 2 || area.height < 2 {
      return;
    }
    for y in [area.y, area.y + area.height - 1] {
      for x in area.x + 1..area.x + area.width - 1 {
        self.put_char(Pos { col: x, row: y }, chars.hor, style);
      }
    }
    for x in [area.x, area.x + area.width - 1] {
      for y in area.y + 1..area.y + area.height - 1 {
        self.put_char(Pos { col: x, row: y }, chars.ver, style);
      }
    }
    self.put_char(
      Pos {
        col: area.x,
        row: area.y,
      },
      chars.tl,
      style,
    );
    self.put_char(
      Pos {
        col: area.x + area.width - 1,
        row: area.y,
      },
      chars.tr,
      style,
    );
    self.put_char(
      Pos {
        col: area.x + area.width - 1,
        row: area.y + area.height - 1,
      },
      chars.br,
      style,
    );
    self.put_char(
      Pos {
        col: area.x,
        row: area.y + area.height - 1,
      },
      chars.bl,
      style,
    );
  }

  /// Draws `text` on one row, clipped to `area`; returns what it covered.
  pub fn draw_text(&mut self, area: Rect, text: &str, style: Style) -> Rect {
    let mut x = area.x;
    if area.y >= self.size().height || area.width == 0 {
      return Rect {
        x: area.x,
        y: area.y,
        width: 0,
        height: 1,
      };
    }
    let style_id = self.intern_style(style);
    for g in text.graphemes(true) {
      let w = unicode::cluster_width(g).max(1);
      if x + u16::from(w) - area.x > area.width {
        break;
      }
      let cell_text = self.cell_text(g, w);
      self.write_cell(
        Pos {
          col: x,
          row: area.y,
        },
        cell_text,
        style_id,
      );
      x += u16::from(w);
    }
    Rect {
      x: area.x,
      y: area.y,
      width: x - area.x,
      height: 1,
    }
  }

  pub fn fill_area(&mut self, area: Rect, c: char, style: Style) {
    let style_id = self.intern_style(style);
    let text = if c == ' ' {
      CellText::EMPTY
    } else {
      CellText::scalar(c, unicode::scalar_width(c).unwrap_or(1).max(1))
    };
    for row in area.y..area.bottom().min(self.size().height) {
      for col in area.x..area.right().min(self.size().width) {
        self.write_cell(Pos { col, row }, text, style_id);
      }
    }
  }

  /// Draws a border; the returned block adds title pieces left to right.
  pub fn block(
    &mut self,
    area: Rect,
    border: BorderType,
    style: Style,
  ) -> Block<'_> {
    self.draw_block(area, &border.chars(), style);
    Block {
      grid: self,
      area,
      title: area.inner((0, 1)).take_top(1),
      style,
    }
  }

  /// The plain text of row `row`, blanks as spaces, trailing blanks
  /// trimmed. For tests and UI readback.
  pub fn row_text(&self, row: u16) -> String {
    let mut out = String::new();
    if let Some(row) = self.visible_row(row) {
      row.push_text(0..row.cols(), &self.tables().graphemes, &mut out);
    }
    out.truncate(out.trim_end().len());
    out
  }

  /// The style of the cell at `pos`.
  pub fn style_at(&self, pos: Pos) -> Option<Style> {
    self
      .visible_cell(pos)
      .map(|cell: Cell| self.style_of(cell.style))
  }
}

pub struct Block<'a> {
  grid: &'a mut Grid,
  area: Rect,
  title: Rect,
  style: Style,
}

impl Block<'_> {
  pub fn title(&mut self, text: &str, style: Style) -> &mut Self {
    let area = self.title.take_left(unicode::str_width(text));
    self.grid.draw_text(area, text, style);
    self
  }

  /// Puts `text` at the right end of the top border, left of what is
  /// there already; nothing when it does not fit. Returns where it went.
  pub fn title_right(&mut self, text: &str, style: Style) -> Option<Rect> {
    let width = unicode::str_width(text);
    if width > self.title.width {
      return None;
    }
    let area = self.title.take_right(width);
    self.grid.draw_text(area, text, style);
    Some(area)
  }

  /// Skips `width` border cells before the next title piece.
  pub fn gap(&mut self, width: u16) -> &mut Self {
    self.title.take_left(width);
    self
  }

  pub fn inner(&self) -> Rect {
    self.area.inner(1)
  }

  /// A horizontal line across the block at row `y`, joined to the
  /// border on both sides.
  pub fn separator(&mut self, y: u16) -> &mut Self {
    self.grid.separator(self.area, y, self.style);
    self
  }
}
