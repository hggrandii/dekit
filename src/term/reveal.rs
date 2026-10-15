//! A diagonal sweep that paints a screen over whatever a terminal shows,
//! band by band, instead of all at once. Played after a live upgrade so
//! the new kernel's first paint is seen arriving rather than snapping in.
//!
//! The band is a blue tint moving across the screen: it paints the new
//! screen tinted, strongest at its leading edge, and behind it the new
//! screen is painted as is. Ahead of it the terminal keeps whatever it
//! showed.

use super::{
  Color, Palette, Rgb, cell::Cell, frame::Frame, style::Style, vt::emit,
};

/// Frames of terminal bytes; write each, then wait `FRAME_INTERVAL`.
/// Once the last frame is written every cell of `new` is on the
/// terminal (hyperlinks included when `links`), the cursor hidden, the
/// attributes reset and no link open. The caller paints again as usual
/// afterwards, after telling its painter the sweep showed `new`, to place
/// the cursor (and any change made meanwhile).
pub fn frames(new: &Frame, links: bool) -> Vec<Vec<u8>> {
  let size = new.size;
  if size.width == 0 || size.height == 0 {
    return Vec::new();
  }
  let steps = FRAMES.max(1);
  let last = depth(size.width - 1, size.height - 1) + BAND;
  let mut out = Vec::with_capacity(steps as usize);
  let mut painted: Vec<Paint> =
    vec![Paint::Old; size.width as usize * size.height as usize];
  let mut brush = Style::DEFAULT;
  let mut pos = None;
  let mut link: Option<&crate::term::style::Link> = None;
  let mut buf = [0u8; 4];
  let cell_of =
    |x: u16, y: u16| -> Cell { new.cell(y, x).unwrap_or(Cell::EMPTY) };
  for step in 1..=steps {
    let front = (last as u32 * step as u32).div_ceil(steps as u32) as u16;
    let mut frame = Vec::new();
    if step == 1 {
      emit::dec_reset(&mut frame, emit::DecMode::ShowCursor);
    }
    for y in 0..size.height {
      for x in 0..size.width {
        let paint = match depth(x, y) {
          d if d + BAND <= front => Paint::New,
          d if d < front => Paint::Band(level(front - d)),
          _ => Paint::Old,
        };
        let index = y as usize * size.width as usize + x as usize;
        if painted[index] == paint {
          continue;
        }
        painted[index] = paint;
        let tint = match paint {
          Paint::Old => unreachable!("an untouched cell never changes"),
          Paint::New => None,
          Paint::Band(level) => Some(TINTS[level as usize]),
        };
        let cell = cell_of(x, y);
        // The right half of a wide glyph is never drawn on its own; it
        // follows the glyph.
        if cell.is_continuation() {
          continue;
        }
        let cell_style = new.style(cell);
        let style = cell_style.with_link(Default::default());
        let style = match tint {
          Some(tint) => tinted(style, tint),
          None => style,
        };
        emit::sgr(&mut frame, brush, style);
        brush = style;
        if links {
          let wanted = new.link(&cell_style);
          if wanted != link {
            emit::osc_link(&mut frame, wanted);
            link = wanted;
          }
        }
        if pos != Some((x, y)) {
          emit::cup(&mut frame, y, x);
        }
        let text = new.text(cell, &mut buf);
        let text = if text.is_empty() { " " } else { text };
        frame.extend_from_slice(text.as_bytes());
        // Past the last column the cursor is pending a wrap; the next
        // write there must be addressed, or it scrolls the terminal.
        let end = x + cell.width().max(1);
        pos = (end < size.width).then_some((end, y));
      }
    }
    if step == steps {
      frame.extend_from_slice(emit::SGR_RESET.as_bytes());
      if link.is_some() {
        emit::osc_link(&mut frame, None);
        link = None;
      }
    }
    out.push(frame);
  }
  out
}

/// Time between frames: 120 a second, so the sweep lasts a second.
pub const FRAME_INTERVAL: std::time::Duration =
  std::time::Duration::from_micros(8_333);
const FRAMES: u16 = 120;
/// Depth of the band, in `depth` units.
const BAND: u16 = 16;
/// How much of the wave color (out of 255) each level of the band takes,
/// from its leading edge back.
const TINTS: [u8; 4] = [150, 110, 80, 40];

/// Cells are about twice as tall as wide: `x + 2y` sweeps at 45°.
fn depth(x: u16, y: u16) -> u16 {
  x + 2 * y
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Paint {
  /// Still whatever the terminal showed before.
  Old,
  /// In the band, at one of its `level`s.
  Band(u8),
  /// The cell as `new` has it.
  New,
}

/// The band in `TINTS.len()` levels by `age`, the steps a cell is behind
/// the leading edge (1 = the edge): a cell is repainted once per level
/// while the band passes rather than every step.
fn level(age: u16) -> u8 {
  let quarter = BAND / 4;
  match age {
    a if a <= quarter => 0,
    a if a <= 2 * quarter => 1,
    a if a <= 3 * quarter => 2,
    _ => 3,
  }
}

const WAVE: Rgb = Rgb(0x2f, 0x81, 0xf7);
const WAVE_TEXT: Rgb = Rgb(0xea, 0xf3, 0xff);

fn tinted(style: Style, amount: u8) -> Style {
  let palette = Palette::dark();
  let (fg, bg) = if style.inverse() {
    (palette.bg(style.bg), palette.fg(style.fg))
  } else {
    (palette.fg(style.fg), palette.bg(style.bg))
  };
  style
    .fg(Color::from(fg.blend(WAVE_TEXT, amount)))
    .bg(Color::from(bg.blend(WAVE, amount)))
    .set_inverse(false)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::term::{Screen, Size};

  fn screen_text(screen: &Screen) -> Vec<String> {
    let frame = screen.frame();
    (0..frame.size.height)
      .map(|row| frame.row_text(row, 0, frame.size.width))
      .collect()
  }

  #[test]
  fn every_cell_arrives_by_the_last_frame() {
    let size = Size {
      width: 12,
      height: 4,
    };
    let mut wanted = Screen::new(size, 0);
    wanted
      .process(b"\x1b[1;1Hhello world!\x1b[2;3H\x1b[31mred\x1b[0m\x1b[4;12H#");
    let mut shown = Screen::new(size, 0);
    shown.process(b"\x1b[1;1Hold old old old old old old");

    let frames = frames(&wanted.frame(), true);
    assert_eq!(frames.len(), 120);
    assert!(frames[0].starts_with(b"\x1b[?25l"));
    assert!(frames.last().unwrap().ends_with(b"\x1b[0m"));

    // The band starts at the top left; the far corner is still old.
    let early: Vec<u8> = frames[..8].concat();
    let early = String::from_utf8_lossy(&early);
    assert!(early.contains('h'), "{early:?}");
    assert!(!early.contains('#'), "{early:?}");

    let mut seen_tinted = false;
    for (i, frame) in frames.iter().enumerate() {
      shown.process(frame);
      let top_left = shown.cell(0, 0).unwrap();
      let style = shown.frame().style(top_left);
      // Tinted in the band, then as is.
      if i >= 1 && style != Style::DEFAULT {
        let mut buf = [0u8; 4];
        assert_eq!(shown.frame().text(top_left, &mut buf), "h");
        seen_tinted = true;
      }
    }
    assert!(seen_tinted);
    assert_eq!(screen_text(&shown), screen_text(&wanted));
    let shown_frame = shown.frame();
    let wanted_frame = wanted.frame();
    assert_eq!(
      shown_frame.style(shown_frame.cell(1, 2).unwrap()).fg,
      wanted_frame.style(wanted_frame.cell(1, 2).unwrap()).fg
    );
    assert_eq!(
      shown_frame.style(shown_frame.cell(0, 0).unwrap()),
      Style::DEFAULT
    );
    assert!(shown.hide_cursor());
  }

  /// Letters that never occur in an escape sequence, so counting them
  /// across the frames counts the cell's repaints.
  #[test]
  fn a_cell_is_repainted_once_per_level() {
    let size = Size {
      width: 6,
      height: 2,
    };
    let mut wanted = Screen::new(size, 0);
    wanted.process(b"abcdef\r\nwxyzab");
    let all = frames(&wanted.frame(), true).concat();
    let count = |c: u8| all.iter().filter(|b| **b == c).count();
    // Once per level of the band, then as is.
    for c in b"cdefwxyz" {
      assert_eq!(count(*c), TINTS.len() + 1, "{}", *c as char);
    }
  }

  #[test]
  fn the_sweep_paints_links_when_the_terminal_has_them() {
    let mut wanted = Screen::new(
      Size {
        width: 8,
        height: 1,
      },
      0,
    );
    wanted.process(b"\x1b]8;;http://x\x1b\\ab\x1b]8;;\x1b\\c");
    let with = frames(&wanted.frame(), true).concat();
    let text = String::from_utf8_lossy(&with);
    assert!(text.contains("\x1b]8;;http://x\x1b\\"), "{text:?}");
    // Every link opened is closed again: the painter assumes none is
    // open when the sweep ends.
    let opens = text.matches("]8;;http://x").count();
    let closes = text.matches("]8;;\x1b\\").count();
    assert!(
      opens > 0 && opens == closes,
      "{opens} opens, {closes} closes"
    );
    let without = frames(&wanted.frame(), false).concat();
    assert!(!String::from_utf8_lossy(&without).contains("\x1b]8;"));
  }

  #[test]
  fn an_empty_screen_has_no_frames() {
    let empty = Frame::empty(Size {
      width: 0,
      height: 0,
    });
    // A grid is never smaller than one cell; the frame says so too.
    assert_eq!(empty.size.width, 1);
    assert_eq!(frames(&empty, true).len(), 120);
  }
}
