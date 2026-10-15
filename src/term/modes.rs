//! Terminal modes observers need to know about, shared by the screen
//! that keeps them and the frames that carry them.

/// The xterm mouse handling mode currently in use.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum MouseProtocolMode {
  /// Mouse handling is disabled.
  #[default]
  None,
  /// Button presses only (X10, `?9`).
  Press,
  /// Presses and releases (VT200, `?1000`).
  PressRelease,
  /// Presses, releases, and motion while a button is held (`?1002`).
  ButtonMotion,
  /// Everything, motion included (`?1003`).
  AnyMotion,
}

/// The encoding to use for the enabled `MouseProtocolMode`.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum MouseProtocolEncoding {
  /// Default single-printable-byte encoding.
  #[default]
  Default,
  /// UTF-8-based encoding (`?1005`).
  Utf8,
  /// SGR encoding (`?1006`), the one every program should use.
  Sgr,
  /// urxvt's decimal encoding (`?1015`).
  Urxvt,
  /// SGR with pixel coordinates (`?1016`).
  SgrPixels,
}

impl MouseProtocolMode {
  /// The name snapshots use.
  pub fn name(self) -> &'static str {
    match self {
      MouseProtocolMode::None => "none",
      MouseProtocolMode::Press => "press",
      MouseProtocolMode::PressRelease => "press_release",
      MouseProtocolMode::ButtonMotion => "button_motion",
      MouseProtocolMode::AnyMotion => "any_motion",
    }
  }

  pub fn from_name(name: &str) -> MouseProtocolMode {
    match name {
      "press" => MouseProtocolMode::Press,
      "press_release" => MouseProtocolMode::PressRelease,
      "button_motion" => MouseProtocolMode::ButtonMotion,
      "any_motion" => MouseProtocolMode::AnyMotion,
      _ => MouseProtocolMode::None,
    }
  }
}

impl MouseProtocolEncoding {
  pub fn name(self) -> &'static str {
    match self {
      MouseProtocolEncoding::Default => "default",
      MouseProtocolEncoding::Utf8 => "utf8",
      MouseProtocolEncoding::Sgr => "sgr",
      MouseProtocolEncoding::Urxvt => "urxvt",
      MouseProtocolEncoding::SgrPixels => "sgr_pixels",
    }
  }

  pub fn from_name(name: &str) -> MouseProtocolEncoding {
    match name {
      "utf8" => MouseProtocolEncoding::Utf8,
      "sgr" => MouseProtocolEncoding::Sgr,
      "urxvt" => MouseProtocolEncoding::Urxvt,
      "sgr_pixels" => MouseProtocolEncoding::SgrPixels,
      _ => MouseProtocolEncoding::Default,
    }
  }
}

/// Kitty keyboard protocol flags.
pub const KITTY_DISAMBIGUATE: u8 = 1;
pub const KITTY_EVENT_TYPES: u8 = 2;
pub const KITTY_ALTERNATE_KEYS: u8 = 4;
pub const KITTY_ALL_KEYS: u8 = 8;
pub const KITTY_TEXT: u8 = 16;
pub const KITTY_ALL_FLAGS: u8 = 31;

/// What of a screen's modes matter to whoever shows it or feeds it
/// input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ObservedModes {
  pub mouse: MouseProtocolMode,
  pub mouse_encoding: MouseProtocolEncoding,
  pub bracketed_paste: bool,
  pub kitty_flags: u8,
  pub app_cursor: bool,
  pub app_keypad: bool,
  pub focus_reporting: bool,
  pub alt_screen: bool,
  /// `?1007`: the wheel scrolls the program (as arrow keys) in the
  /// alternate screen.
  pub alt_scroll: bool,
  pub hide_cursor: bool,
}
