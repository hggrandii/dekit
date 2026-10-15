//! Paints frames onto a terminal as the difference from the last paint,
//! shaped by what that terminal can show.

use std::sync::Arc;

use super::images::ImagePainter;
use crate::protocol::TermCaps;
use crate::term::{
  cell::{Cell, TextKind},
  common::CursorStyle,
  frame::Frame,
  grid::Pos,
  style::{Link, Style, Underline},
  unicode,
  vt::emit,
};

pub struct Painter {
  caps: TermCaps,
  /// Kitty images on the terminal, when it shows them.
  images: Option<ImagePainter>,
  /// The frame as last painted; `None` paints everything.
  prev: Option<Arc<Frame>>,
  /// The SGR state the terminal is in, as emitted (after downgrades).
  brush: Style,
  /// The hyperlink open on the terminal.
  link: Option<Link>,
  /// Where the terminal's cursor is, when known.
  pos: Option<Pos>,
  cursor_shown: Option<bool>,
  cursor_style: Option<CursorStyle>,
}

impl Painter {
  pub fn new(caps: TermCaps) -> Self {
    let images = caps
      .kitty_graphics
      .as_ref()
      .map(|graphics| ImagePainter::new(graphics.animation));
    Painter {
      caps,
      images,
      prev: None,
      brush: Style::DEFAULT,
      link: None,
      pos: None,
      cursor_shown: None,
      cursor_style: None,
    }
  }

  pub fn caps(&self) -> &TermCaps {
    &self.caps
  }

  /// Deletes what dekit put on the terminal besides text: images.
  pub fn cleanup(&mut self, out: &mut Vec<u8>) {
    if let Some(images) = &mut self.images {
      images.cleanup(out);
    }
  }

  /// Puts a terminal in the state a fresh painter assumes: default
  /// attributes, cursor shown with the default shape, no link, and no
  /// images of an earlier painter's.
  pub fn reset(out: &mut Vec<u8>, caps: &TermCaps) {
    out.extend_from_slice(emit::SGR_RESET.as_bytes());
    emit::dec_set(out, emit::DecMode::ShowCursor);
    emit::cursor_style(out, CursorStyle::default());
    emit::osc_link(out, None);
    if caps.kitty_graphics.is_some() {
      emit::kitty_graphics(out, "a=d,d=A,q=2", b"");
    }
  }

  /// Something else painted the terminal (the reveal sweep paints a
  /// frame whole, hides the cursor and resets the attributes): `shown`
  /// is the frame it shows now, or `None` when that is unknown and the
  /// next paint should repaint everything.
  pub fn assume(&mut self, shown: Option<Arc<Frame>>) {
    self.prev = shown;
    self.pos = None;
    self.brush = Style::DEFAULT;
    self.link = None;
    self.cursor_shown = Some(false);
  }

  pub fn paint(&mut self, out: &mut Vec<u8>, frame: &Arc<Frame>) {
    // The sync wrapper goes around whatever gets painted; nothing, and
    // it comes off again.
    let start = out.len();
    if self.caps.sync_output {
      emit::dec_set(out, emit::DecMode::SyncOutput);
    }
    let begun = out.len();
    // A terminal may drop image placements when it is resized; the
    // cells are all repainted then, and so are the placements.
    let resized = self
      .prev
      .as_ref()
      .is_some_and(|prev| prev.size != frame.size);
    self.paint_cells(out, frame);
    if let Some(images) = &mut self.images {
      if resized {
        images.forget_placements(out);
      }
      if images.paint(out, frame) {
        // Placements are positioned with the cursor.
        self.pos = None;
      }
    }
    self.paint_cursor(out, frame);
    if self.caps.sync_output {
      if out.len() == begun {
        out.truncate(start);
      } else {
        emit::dec_reset(out, emit::DecMode::SyncOutput);
      }
    }
    self.prev = Some(frame.clone());
  }

  fn paint_cells(&mut self, out: &mut Vec<u8>, frame: &Arc<Frame>) {
    let size = frame.size;
    let prev = match &self.prev {
      Some(prev) if prev.size == size => Some(prev.clone()),
      _ => None,
    };
    // Cells compare by id only within one grid's tables.
    let same_tables = prev.as_ref().is_some_and(|prev| {
      Arc::ptr_eq(&prev.tables.styles, &frame.tables.styles)
        && Arc::ptr_eq(&prev.tables.graphemes, &frame.tables.graphemes)
        && Arc::ptr_eq(&prev.tables.links, &frame.tables.links)
    });
    let mut buf = [0u8; 4];
    for y in 0..size.height {
      let Some(row) = frame.rows.get(usize::from(y)) else {
        break;
      };
      let prev_row =
        prev.as_ref().and_then(|prev| prev.rows.get(usize::from(y)));
      if same_tables
        && let Some(prev_row) = prev_row
        && Arc::ptr_eq(prev_row, row)
      {
        continue;
      }
      let mut x = 0u16;
      while x < size.width {
        let Some(cell) = row.get(x) else {
          break;
        };
        if cell.is_continuation() {
          x += 1;
          continue;
        }
        let changed = match (prev_row, prev.as_ref()) {
          (Some(prev_row), Some(prev)) => match prev_row.get(x) {
            Some(old) => {
              // A wide cell is repainted when either half differs.
              self.cell_changed(prev, old, frame, cell, same_tables)
                || (cell.is_wide()
                  && match (prev_row.get(x + 1), row.get(x + 1)) {
                    (Some(old), Some(new)) => {
                      self.cell_changed(prev, old, frame, new, same_tables)
                    }
                    (old, new) => old.is_some() != new.is_some(),
                  })
            }
            None => true,
          },
          _ => true,
        };
        if changed {
          self.put(out, frame, Pos { row: y, col: x }, cell, &mut buf);
        }
        x += cell.width().max(1);
      }
    }
    if self.link.is_some() {
      emit::osc_link(out, None);
      self.link = None;
    }
  }

  /// Whether `cell` shows something other than `old` did. Cells compare
  /// by id only within one grid's tables; otherwise by what they show.
  fn cell_changed(
    &self,
    prev: &Frame,
    old: Cell,
    frame: &Frame,
    cell: Cell,
    same_tables: bool,
  ) -> bool {
    if same_tables {
      return old != cell;
    }
    let (mut old_buf, mut buf) = ([0u8; 4], [0u8; 4]);
    self.resolved(prev, old, &mut old_buf)
      != self.resolved(frame, cell, &mut buf)
  }

  /// What a cell shows, independent of which tables it is interned in.
  fn resolved<'a>(
    &self,
    frame: &'a Frame,
    cell: Cell,
    buf: &'a mut [u8; 4],
  ) -> (&'a str, Style, Option<&'a Link>) {
    let style = frame.style(cell);
    let link = frame.link(&style);
    (frame.text(cell, buf), style, link)
  }

  fn put(
    &mut self,
    out: &mut Vec<u8>,
    frame: &Frame,
    pos: Pos,
    cell: Cell,
    buf: &mut [u8; 4],
  ) {
    let style = frame.style(cell);
    let shown = self.shape(style);
    if shown != self.brush {
      emit::sgr(out, self.brush, shown);
      self.brush = shown;
    }
    if self.caps.hyperlinks {
      let link = frame.link(&style);
      if link != self.link.as_ref() {
        emit::osc_link(out, link);
        self.link = link.cloned();
      }
    }
    if self.pos != Some(pos) {
      emit::cup(out, pos.row, pos.col);
    }
    let text = frame.text(cell, buf);
    let width = cell.width().max(1);
    if text.is_empty() || cell.text.is_image() {
      // A blank, or an image placeholder cell: nothing to show in text
      // until images are painted.
      for _ in 0..width {
        out.push(b' ');
      }
    } else {
      out.extend_from_slice(text.as_bytes());
    }
    let end = pos.col + width;
    // Past the last column the cursor is pending a wrap; the next write
    // there must be addressed, or it scrolls the terminal. Text the
    // terminal may measure differently loses the position too.
    self.pos = if end >= frame.size.width || self.uncertain_width(text, cell) {
      None
    } else {
      Some(Pos {
        row: pos.row,
        col: end,
      })
    };
  }

  /// Whether the terminal might give `text` another width than we do.
  fn uncertain_width(&self, text: &str, cell: Cell) -> bool {
    match cell.text.kind() {
      TextKind::Empty | TextKind::Continuation | TextKind::Image { .. } => {
        false
      }
      TextKind::Scalar(c) => {
        // Plain narrow text and East Asian wide characters are agreed
        // on everywhere; emoji presentation is not.
        (c as u32) >= 0x2300 && unicode::is_emoji_base(c)
      }
      TextKind::Grapheme(_) => {
        let widths = self.caps.widths;
        let mut chars = text.chars();
        let first = chars.next().unwrap_or(' ');
        if text.contains(unicode::ZWJ) {
          !widths.zwj_joins
        } else if text.contains(unicode::VS16) {
          !widths.vs16_wide
        } else if unicode::is_regional_indicator(first) {
          !widths.flags_wide
        } else {
          // Combining marks: a terminal may print them on their own.
          true
        }
      }
    }
  }

  /// The style as the terminal will be told it: colors it has, shapes it
  /// knows. Links are not part of SGR.
  fn shape(&self, style: Style) -> Style {
    let depth = self.caps.colors;
    let mut shown = Style::DEFAULT
      .fg(style.fg.downgrade(depth))
      .bg(style.bg.downgrade(depth))
      .from_attrs_bits(style.attrs_bits());
    if self.caps.underline_styles {
      shown.ul = style.ul.downgrade(depth);
    } else {
      shown.ul = crate::term::Color::Default;
      if style.underline() != Underline::None {
        shown = shown.set_underline(Underline::Single);
      }
    }
    if !self.caps.strikethrough {
      shown = shown.set_strike(false);
    }
    shown
  }

  fn paint_cursor(&mut self, out: &mut Vec<u8>, frame: &Frame) {
    let shown = frame.cursor.is_some();
    if self.cursor_shown != Some(shown) {
      if shown {
        emit::dec_set(out, emit::DecMode::ShowCursor);
      } else {
        emit::dec_reset(out, emit::DecMode::ShowCursor);
      }
      self.cursor_shown = Some(shown);
    }
    if let Some(pos) = frame.cursor {
      if self.pos != Some(pos) {
        emit::cup(out, pos.row, pos.col);
      }
      self.pos = Some(pos);
    }
    if self.cursor_style != Some(frame.cursor_style) {
      emit::cursor_style(out, frame.cursor_style);
      self.cursor_style = Some(frame.cursor_style);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::term::{Color, Grid, Screen, Size, color::ColorDepth, grid::Rect};

  fn frame_of(text: &str) -> Arc<Frame> {
    let mut screen = Screen::new(
      Size {
        width: 10,
        height: 2,
      },
      0,
    );
    screen.process(text.as_bytes());
    Arc::new(screen.frame())
  }

  fn paint(painter: &mut Painter, frame: &Arc<Frame>) -> Vec<u8> {
    let mut out = Vec::new();
    painter.paint(&mut out, frame);
    out
  }

  #[test]
  fn reset_clears_an_earlier_painters_images_when_shown() {
    let mut out = Vec::new();
    Painter::reset(&mut out, &TermCaps::full());
    assert!(out.ends_with(b"\x1b_Ga=d,d=A,q=2\x1b\\"));
    let mut plain = Vec::new();
    Painter::reset(&mut plain, &TermCaps::default());
    assert!(!plain.contains(&b'_'));
  }

  #[test]
  fn first_paint_then_diff() {
    let mut painter = Painter::new(TermCaps::full());
    let out = paint(&mut painter, &frame_of("12\x1b[34m34\x1b[m5"));
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("\x1b[1;1H12\x1b[34m34\x1b[0m5"), "{s:?}");
    // The same frame again paints nothing but the sync wrapper.
    let frame = frame_of("12\x1b[34m34\x1b[m5");
    let out = paint(&mut painter, &frame);
    assert_eq!(out, b"");
    // One changed cell.
    let out = paint(&mut painter, &frame_of("1_\x1b[34m34\x1b[m5"));
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("\x1b[1;2H_"), "{s:?}");
    assert!(!s.contains("34"), "{s:?}");
  }

  #[test]
  fn sync_wrapping_and_cursor() {
    let mut painter = Painter::new(TermCaps::full());
    let out = paint(&mut painter, &frame_of("abc"));
    assert!(out.starts_with(b"\x1b[?2026h"));
    assert!(out.ends_with(b"\x1b[?2026l"));
    let s = String::from_utf8_lossy(&out);
    // Cursor after the text, shown, default style.
    assert!(s.contains("\x1b[?25h"), "{s:?}");
    assert!(s.contains("\x1b[0 q"), "{s:?}");
    let out = paint(&mut painter, &frame_of("abc\x1b[?25l"));
    assert_eq!(out, b"\x1b[?2026h\x1b[?25l\x1b[?2026l");
  }

  #[test]
  fn wide_char_continuation_not_overwritten() {
    let mut painter = Painter::new(TermCaps::full());
    let out = paint(&mut painter, &frame_of("A\u{6d4b}B"));
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("A\u{6d4b}B"), "{s:?}");
    let out = paint(&mut painter, &frame_of("A\u{6d4b}B"));
    assert_eq!(out, b"");
  }

  #[test]
  fn a_wide_cells_changed_right_half_repaints_across_tables() {
    use crate::term::{cell::CellText, row::Row, style::StyleId};
    let frame = |right: Cell| {
      let mut grid = Grid::new(
        Size {
          width: 4,
          height: 1,
        },
        0,
      );
      let cells = vec![
        Cell::new(CellText::scalar('\u{6d4b}', 2), StyleId::DEFAULT),
        right,
        Cell::new(CellText::scalar('b', 1), StyleId::DEFAULT),
      ];
      grid.restore(vec![Arc::new(Row::from_cells(cells, 4))], Vec::new());
      Arc::new(Frame::of_grid(&grid))
    };
    let mut painter = Painter::new(TermCaps {
      sync_output: false,
      ..TermCaps::full()
    });
    let continuation = Cell::new(CellText::CONTINUATION, StyleId::DEFAULT);
    paint(&mut painter, &frame(continuation));
    // Each grid has tables of its own, so cells compare by what they
    // show; the left half shows the same, the right half does not.
    let out = paint(&mut painter, &frame(continuation));
    assert_eq!(out, b"");
    let replaced = Cell::new(CellText::scalar('X', 1), StyleId::DEFAULT);
    let out = paint(&mut painter, &frame(replaced));
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("\x1b[1;1H\u{6d4b}"), "{s:?}");
  }

  #[test]
  fn colors_downgrade_and_links() {
    let caps = TermCaps {
      colors: ColorDepth::Ansi256,
      hyperlinks: true,
      sync_output: false,
      ..TermCaps::full()
    };
    let mut painter = Painter::new(caps);
    let out = paint(
      &mut painter,
      &frame_of("\x1b[38;2;255;0;0m\x1b]8;;http://x\x1b\\r\x1b]8;;\x1b\\n"),
    );
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("\x1b[38;5;196m"), "{s:?}");
    assert!(s.contains("\x1b]8;;http://x\x1b\\"), "{s:?}");
    assert!(s.contains("r\x1b]8;;\x1b\\n"), "{s:?}");
  }

  #[test]
  fn uncertain_widths_force_addressing() {
    let caps = TermCaps {
      sync_output: false,
      ..TermCaps::default()
    };
    let mut painter = Painter::new(caps);
    let out = paint(&mut painter, &frame_of("\u{2764}\u{FE0F}ab"));
    let s = String::from_utf8_lossy(&out);
    // After the emoji the next cell is addressed explicitly.
    assert!(s.contains("\u{2764}\u{FE0F}\x1b[1;3Hab"), "{s:?}");
  }

  /// A terminal painted across a resize cycle shows the frame it was
  /// last given, cell for cell.
  #[test]
  fn repaints_match_the_frame_across_resizes() {
    let text = "Reflow: resize now\r\n\r\n01 a long line of text that wraps at the pane width and again at the new width, numbers mark its start\r\n02 a long line of text that wraps at the pane width and again at the new width\r\nwide: \u{6d4b}\u{8bd5}\u{6d4b}\u{8bd5}\u{6d4b}\u{8bd5}\u{6d4b}\u{8bd5}\u{6d4b}\u{8bd5} (never split)\r\n\x1b[31mred\x1b[0m end\r\n";
    let mut source = Screen::new(
      Size {
        width: 60,
        height: 8,
      },
      100,
    );
    source.process(text.as_bytes());
    let mut terminal = Screen::new(
      Size {
        width: 60,
        height: 8,
      },
      0,
    );
    let mut painter = Painter::new(TermCaps {
      sync_output: false,
      ..TermCaps::full()
    });
    let same = |a: &Screen, b: &Screen| {
      let (fa, fb) = (a.frame(), b.frame());
      for row in 0..fa.size.height {
        // The terminal shows a wrap filler as a space the source does
        // not count as text; both end in blanks.
        let ta = fa.row_text(row, 0, fa.size.width);
        let tb = fb.row_text(row, 0, fb.size.width);
        assert_eq!(ta.trim_end(), tb.trim_end(), "row {row}");
      }
      assert_eq!(fa.cursor, fb.cursor);
    };
    for (w, h) in [(60, 8), (25, 8), (60, 8), (40, 5), (60, 8)] {
      source.set_size(h, w);
      terminal.set_size(h, w);
      let mut out = Vec::new();
      painter.paint(&mut out, &Arc::new(source.frame()));
      terminal.process(&out);
      same(&source, &terminal);
    }
  }

  /// After a resize the terminal may have dropped the placements: they
  /// are all put back, the image data is not sent again.
  #[test]
  fn resizes_put_placements_back() {
    let mut grid = Grid::new(
      Size {
        width: 20,
        height: 5,
      },
      0,
    );
    let image = Arc::new(crate::term::image::Image {
      key: 7,
      id: 1,
      number: 0,
      data: crate::term::image::store::ImageData {
        format: crate::term::image::Format::Rgb,
        zlib: false,
        width: 2,
        height: 1,
        bytes: bytes::Bytes::from_static(&[255, 0, 0, 0, 255, 0]),
      },
      commands: Vec::new(),
    });
    grid.add_image(image);
    grid.put_placement(crate::term::image::VisiblePlacement {
      key: 7,
      row: 1,
      col: 2,
      cols: 4,
      rows: 2,
      src: crate::term::image::SrcRect {
        x: 0,
        y: 0,
        w: 2,
        h: 1,
      },
      offset_x: 0,
      offset_y: 0,
      z: 0,
    });
    let mut painter = Painter::new(TermCaps {
      sync_output: false,
      ..TermCaps::full()
    });
    let out = paint(&mut painter, &Arc::new(Frame::of_grid(&grid)));
    let s = String::from_utf8_lossy(&out);
    assert_eq!(s.matches("a=t,").count(), 1);
    assert_eq!(s.matches("a=p,").count(), 1);
    // Same frame: nothing. Then a resize: delete and place, no transmit.
    assert!(paint(&mut painter, &Arc::new(Frame::of_grid(&grid))).is_empty());
    grid.set_size(Size {
      width: 19,
      height: 5,
    });
    let out = paint(&mut painter, &Arc::new(Frame::of_grid(&grid)));
    let s = String::from_utf8_lossy(&out);
    assert_eq!(s.matches("a=t,").count(), 0, "{s:?}");
    assert_eq!(s.matches("a=d,d=i").count(), 1, "{s:?}");
    assert_eq!(s.matches("a=p,").count(), 1, "{s:?}");
  }

  #[test]
  fn ui_grid_frames_paint() {
    let mut grid = Grid::new(
      Size {
        width: 5,
        height: 1,
      },
      0,
    );
    grid.draw_text(
      Rect::new(0, 0, 5, 1),
      "hi",
      Style::default().fg(Color::RED),
    );
    grid.cursor_pos = Some(Pos { row: 0, col: 2 });
    let frame = Arc::new(Frame::of_grid(&grid));
    let mut painter = Painter::new(TermCaps {
      sync_output: false,
      ..TermCaps::full()
    });
    let out = paint(&mut painter, &frame);
    let s = String::from_utf8_lossy(&out);
    assert!(s.starts_with("\x1b[31m\x1b[1;1Hhi"), "{s:?}");
  }
}
