use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Size {
  pub height: u16,
  pub width: u16,
}

#[derive(
  Debug, Default, Deserialize, Clone, Copy, PartialEq, Eq, Serialize,
)]
pub enum CursorStyle {
  #[default]
  Default = 0,
  BlinkingBlock = 1,
  SteadyBlock = 2,
  BlinkingUnderline = 3,
  SteadyUnderline = 4,
  BlinkingBar = 5,
  SteadyBar = 6,
}

impl CursorStyle {
  /// The DECSCUSR parameter; anything unknown is the default.
  pub fn from_u16(n: u16) -> Self {
    match n {
      1 => CursorStyle::BlinkingBlock,
      2 => CursorStyle::SteadyBlock,
      3 => CursorStyle::BlinkingUnderline,
      4 => CursorStyle::SteadyUnderline,
      5 => CursorStyle::BlinkingBar,
      6 => CursorStyle::SteadyBar,
      _ => CursorStyle::Default,
    }
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Winsize {
  pub x: u16,
  pub y: u16,
  pub x_px: u16,
  pub y_px: u16,
}

/// A terminal's geometry: its size in cells, and the size of one cell
/// in pixels when the terminal said. What an observer asks of a screen,
/// what a resize carries, and what the pty is told.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Geometry {
  pub size: Size,
  pub cell_pixels: Option<(u16, u16)>,
}

impl Geometry {
  /// A size whose cell size is unknown.
  pub fn cells(width: u16, height: u16) -> Geometry {
    Geometry {
      size: Size { width, height },
      cell_pixels: None,
    }
  }

  /// What the pty is told: cells and pixels, with 0 pixels while the
  /// cell size is unknown, as a terminal without one reports.
  pub fn winsize(&self) -> Winsize {
    let (cell_w, cell_h) = self.cell_pixels.unwrap_or((0, 0));
    Winsize {
      x: self.size.width,
      y: self.size.height,
      x_px: self.size.width.saturating_mul(cell_w),
      y_px: self.size.height.saturating_mul(cell_h),
    }
  }
}

#[cfg(unix)]
impl From<Winsize> for libc::winsize {
  fn from(value: Winsize) -> Self {
    libc::winsize {
      ws_row: value.y,
      ws_col: value.x,
      ws_xpixel: value.x_px,
      ws_ypixel: value.y_px,
    }
  }
}

#[cfg(unix)]
impl From<Winsize> for rustix::termios::Winsize {
  fn from(value: Winsize) -> Self {
    rustix::termios::Winsize {
      ws_row: value.y,
      ws_col: value.x,
      ws_xpixel: value.x_px,
      ws_ypixel: value.y_px,
    }
  }
}
