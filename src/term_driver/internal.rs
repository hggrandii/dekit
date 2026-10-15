use crate::term::{key::Key, mouse::MouseEvent};

/// What the input decoder reads off the terminal: the events the driver
/// passes on, and the replies to its own probes.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum InternalTermEvent {
  Key(Key),
  Mouse(MouseEvent),
  Paste(String),
  Resize(u16, u16),
  FocusGained,
  FocusLost,
  /// A cursor position report, 0-based (x, y).
  CursorPos(u16, u16),
  /// DA1, with its parameters (`?` stripped).
  PrimaryDeviceAttributes(Vec<u32>),
  ReplyKittyKeyboard(u8),
  /// `CSI code ; a ; b t`.
  WindowReport(u32, u32, u32),
  /// DECRPM: `CSI ? mode ; state $ y`.
  ModeReport {
    dec: bool,
    mode: u32,
    state: u32,
  },
  /// A kitty graphics reply (`ESC _ G ... ST`), the body after `G`.
  GraphicsReply(Vec<u8>),
  /// A DCS reply (XTVERSION, XTGETTCAP), its body.
  DcsReply(Vec<u8>),
}

#[derive(Debug)]
pub enum KeyboardMode {
  Unknown,
  #[cfg_attr(windows, allow(dead_code))]
  ModifyOtherKeys,
  Kitty,
  #[cfg_attr(not(windows), allow(dead_code))]
  Win32,
}
