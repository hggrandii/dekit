//! ANSI and DEC private modes: set, reset, and report (DECRQM).

use crate::term::{
  modes::{MouseProtocolEncoding, MouseProtocolMode},
  vt::{Params, emit},
};

use super::{Screen, VtEvent};

/// The modes a screen keeps that are not the grid's (origin mode lives
/// there). Defaults are a terminal's power-on state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Modes {
  pub app_keypad: bool,
  pub app_cursor: bool,
  pub hide_cursor: bool,
  pub alt_screen: bool,
  pub bracketed_paste: bool,
  pub autowrap: bool,
  pub insert: bool,
  /// LNM: line feed also returns the carriage.
  pub newline: bool,
  pub focus_report: bool,
  pub alt_scroll: bool,
  /// `?2026`: a synchronized update is in progress.
  pub sync: bool,
  /// `?2027`: the program asked for grapheme clustering (always on).
  pub grapheme_clusters: bool,
  /// `?2048`: the program wants resize reports.
  pub inband_resize: bool,
  pub reverse_video: bool,
  pub cursor_blink: bool,
  /// `?69`: left/right margins may be set (`CSI s` is DECSLRM).
  pub margins: bool,
  /// `?1049`-style saves: the cursor was saved on entering the alt screen.
  pub alt_saved_cursor: bool,
  /// `?1034`: eightBitInput, meta sends ESC. Kept for reports.
  pub meta_sends_esc: bool,
  /// `?2004` and the mouse modes are in `Screen` proper.
  pub keyboard_action_locked: bool,
}

impl Default for Modes {
  fn default() -> Self {
    Modes {
      app_keypad: false,
      app_cursor: false,
      hide_cursor: false,
      alt_screen: false,
      bracketed_paste: false,
      autowrap: true,
      insert: false,
      newline: false,
      focus_report: false,
      alt_scroll: true,
      sync: false,
      grapheme_clusters: false,
      inband_resize: false,
      reverse_video: false,
      cursor_blink: true,
      margins: false,
      alt_saved_cursor: false,
      meta_sends_esc: true,
      keyboard_action_locked: false,
    }
  }
}

/// How many of `Modes::flags` the first snapshot writer knew; they sit in
/// the low bits of its byte.
const BYTE_FLAGS: usize = 5;

impl Modes {
  /// The modes a snapshot stores, in bit order: the first `BYTE_FLAGS`
  /// at bits 0..5 as the first writer stored them, the rest from bit 8,
  /// so any of them in use makes the value exceed a byte and an older
  /// reader, whose field is one, refuses the snapshot rather than lose
  /// it. New modes go at the end. `insert` has its own snapshot field.
  fn flags(&mut self) -> [&mut bool; 18] {
    [
      &mut self.app_keypad,
      &mut self.app_cursor,
      &mut self.hide_cursor,
      &mut self.alt_screen,
      &mut self.bracketed_paste,
      &mut self.autowrap,
      &mut self.newline,
      &mut self.focus_report,
      &mut self.alt_scroll,
      &mut self.sync,
      &mut self.grapheme_clusters,
      &mut self.inband_resize,
      &mut self.reverse_video,
      &mut self.cursor_blink,
      &mut self.margins,
      &mut self.alt_saved_cursor,
      &mut self.meta_sends_esc,
      &mut self.keyboard_action_locked,
    ]
  }

  /// The snapshot bit of `flags()[index]`.
  fn bit(index: usize) -> u32 {
    1 << if index < BYTE_FLAGS { index } else { index + 3 }
  }

  /// Packs the modes into bits for snapshots. A bit is set when the mode
  /// differs from its power-on state, so a mode a snapshot lacks (an
  /// older writer's) reads as power-on; the first writer's modes are off
  /// at power-on, so their bits still mean set.
  pub fn to_bits(&self) -> u32 {
    let (mut modes, mut power_on) = (*self, Modes::default());
    let mut bits = 0;
    for (i, (on, default)) in
      modes.flags().into_iter().zip(power_on.flags()).enumerate()
    {
      if *on != *default {
        bits |= Modes::bit(i);
      }
    }
    bits
  }

  pub fn from_bits(bits: u32) -> Modes {
    let mut modes = Modes::default();
    for (i, flag) in modes.flags().into_iter().enumerate() {
      *flag ^= bits & Modes::bit(i) != 0;
    }
    modes
  }
}

/// DECRQM answers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeState {
  NotRecognized = 0,
  Set = 1,
  Reset = 2,
  PermanentlySet = 3,
  PermanentlyReset = 4,
}

fn state(on: bool) -> ModeState {
  if on { ModeState::Set } else { ModeState::Reset }
}

impl Screen {
  /// `CSI [?] Pm h` / `l`.
  pub(super) fn set_modes(&mut self, p: &Params) {
    let set = p.final_ == b'h';
    for i in 0..p.len() {
      if p.is_sub(i) {
        continue;
      }
      let Some(mode) = p.get_opt(i) else {
        continue;
      };
      if p.prefix == b'?' {
        self.set_dec_mode(mode, set);
      } else {
        self.set_ansi_mode(mode, set);
      }
    }
  }

  fn set_ansi_mode(&mut self, mode: u32, set: bool) {
    match mode {
      // KAM
      2 => self.modes.keyboard_action_locked = set,
      // IRM
      4 => self.modes.insert = set,
      // LNM
      20 => self.modes.newline = set,
      // DECRLM: right to left, not supported.
      34 => (),
      _ => {
        log::debug!("Ignored ANSI mode {mode} {}", if set { "h" } else { "l" })
      }
    }
  }

  fn set_dec_mode(&mut self, mode: u32, set: bool) {
    match mode {
      // DECCKM
      1 => self.modes.app_cursor = set,
      // DECANM: VT52 mode off is all we are.
      2 => (),
      // DECCOLM: 80/132 columns; the width is the client's.
      3 => (),
      // DECSCLM smooth scroll
      4 => (),
      // DECSCNM
      5 => self.modes.reverse_video = set,
      // DECOM
      6 => self.grid_mut().set_origin_mode(set),
      // DECAWM
      7 => self.modes.autowrap = set,
      // DECARM auto repeat
      8 => (),
      // X10 mouse
      9 => self.set_mouse_mode(MouseProtocolMode::Press, set),
      // Cursor blink
      12 => self.modes.cursor_blink = set,
      // DECTCEM
      25 => self.modes.hide_cursor = !set,
      // Numeric keypad (DECNKM)
      66 => self.modes.app_keypad = set,
      // DECBKM backarrow sends BS: keys are encoded by the client.
      67 => (),
      // DECLRMM
      69 => {
        self.modes.margins = set;
        if !set {
          let width = self.grid().size().width;
          self.grid_mut().set_margins(0, width - 1);
        }
      }
      47 | 1047 => {
        if set {
          self.enter_alternate_grid(false);
        } else {
          self.exit_alternate_grid();
        }
      }
      1048 => {
        if set {
          self.save_cursor();
        } else {
          self.restore_cursor();
        }
      }
      1049 => {
        if set {
          self.save_cursor();
          self.enter_alternate_grid(true);
        } else {
          self.exit_alternate_grid();
          self.restore_cursor();
        }
      }
      1000 => self.set_mouse_mode(MouseProtocolMode::PressRelease, set),
      1001 => (), // highlight tracking: never
      1002 => self.set_mouse_mode(MouseProtocolMode::ButtonMotion, set),
      1003 => self.set_mouse_mode(MouseProtocolMode::AnyMotion, set),
      1004 => self.modes.focus_report = set,
      1005 => self.set_mouse_encoding(MouseProtocolEncoding::Utf8, set),
      1006 => self.set_mouse_encoding(MouseProtocolEncoding::Sgr, set),
      1007 => self.modes.alt_scroll = set,
      1015 => self.set_mouse_encoding(MouseProtocolEncoding::Urxvt, set),
      1016 => self.set_mouse_encoding(MouseProtocolEncoding::SgrPixels, set),
      1034 => self.modes.meta_sends_esc = set,
      2004 => self.modes.bracketed_paste = set,
      2026 => self.modes.sync = set,
      2027 => self.modes.grapheme_clusters = set,
      2048 => {
        self.modes.inband_resize = set;
        if set {
          self.report_size_inband();
        }
      }
      _ => {
        log::debug!("Ignored DEC mode ?{mode} {}", if set { "h" } else { "l" })
      }
    }
  }

  fn set_mouse_mode(&mut self, mode: MouseProtocolMode, set: bool) {
    if set {
      self.mouse_mode = mode;
    } else if self.mouse_mode == mode {
      self.mouse_mode = MouseProtocolMode::None;
    }
  }

  fn set_mouse_encoding(&mut self, encoding: MouseProtocolEncoding, set: bool) {
    if set {
      self.mouse_encoding = encoding;
    } else if self.mouse_encoding == encoding {
      self.mouse_encoding = MouseProtocolEncoding::Default;
    }
  }

  /// DECRQM: `CSI [?] Pd $ p`.
  pub(super) fn request_mode(&mut self, p: &Params) {
    let Some(mode) = p.get_opt(0) else {
      return;
    };
    let dec = p.prefix == b'?';
    let state = if dec {
      self.dec_mode_state(mode)
    } else {
      self.ansi_mode_state(mode)
    };
    let mut out = Vec::new();
    emit::decrqm_reply(&mut out, dec, mode, state as u8);
    self.events.push(VtEvent::Reply(out));
  }

  fn ansi_mode_state(&self, mode: u32) -> ModeState {
    match mode {
      2 => state(self.modes.keyboard_action_locked),
      4 => state(self.modes.insert),
      20 => state(self.modes.newline),
      34 => ModeState::PermanentlyReset,
      _ => ModeState::NotRecognized,
    }
  }

  fn dec_mode_state(&self, mode: u32) -> ModeState {
    let m = &self.modes;
    match mode {
      1 => state(m.app_cursor),
      2 => ModeState::PermanentlySet,
      3 | 4 | 8 => ModeState::PermanentlyReset,
      5 => state(m.reverse_video),
      6 => state(self.grid().origin_mode()),
      7 => state(m.autowrap),
      9 => state(self.mouse_mode == MouseProtocolMode::Press),
      12 => state(m.cursor_blink),
      25 => state(!m.hide_cursor),
      66 => state(m.app_keypad),
      67 => ModeState::PermanentlyReset,
      69 => state(m.margins),
      47 | 1047 | 1049 => state(m.alt_screen),
      1048 => state(m.alt_saved_cursor),
      1000 => state(self.mouse_mode == MouseProtocolMode::PressRelease),
      1001 => ModeState::PermanentlyReset,
      1002 => state(self.mouse_mode == MouseProtocolMode::ButtonMotion),
      1003 => state(self.mouse_mode == MouseProtocolMode::AnyMotion),
      1004 => state(m.focus_report),
      1005 => state(self.mouse_encoding == MouseProtocolEncoding::Utf8),
      1006 => state(self.mouse_encoding == MouseProtocolEncoding::Sgr),
      1007 => state(m.alt_scroll),
      1015 => state(self.mouse_encoding == MouseProtocolEncoding::Urxvt),
      1016 => state(self.mouse_encoding == MouseProtocolEncoding::SgrPixels),
      1034 => state(m.meta_sends_esc),
      2004 => state(m.bracketed_paste),
      2026 => state(m.sync),
      2027 => state(m.grapheme_clusters),
      2048 => state(m.inband_resize),
      _ => ModeState::NotRecognized,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::Modes;

  #[test]
  fn snapshot_bits_keep_the_first_writers_byte() {
    assert_eq!(Modes::default().to_bits(), 0);
    let mut modes = Modes {
      app_keypad: true,
      bracketed_paste: true,
      ..Modes::default()
    };
    assert_eq!(modes.to_bits(), 1 | 16);
    assert_eq!(Modes::from_bits(1 | 16), modes);
    // A byte an older binary wrote reads as its modes at power-on.
    let old = Modes::from_bits(0b11111);
    assert!(old.autowrap && old.alt_scroll && old.cursor_blink);
    // A mode that binary lacked does not fit its byte.
    modes.autowrap = false;
    modes.focus_report = true;
    assert!(modes.to_bits() > 0xFF);
    assert_eq!(Modes::from_bits(modes.to_bits()), modes);
  }
}
