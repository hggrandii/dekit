//! The single place that writes terminal escape sequences.

use base64::Engine;

use crate::term::{
  color::{Color, Rgb},
  common::CursorStyle,
  key::{Key, KeyCode, KeyEventKind, KeyEventState, KeyMods},
  modes::MouseProtocolEncoding,
  mouse::{MouseButton, MouseEvent, MouseEventKind},
  style::{Link, Style, Underline},
  vt::keys,
};

pub const SGR_RESET: &str = "\x1b[0m";
pub const SAVE_CURSOR: &str = "\x1b7";
pub const RESTORE_CURSOR: &str = "\x1b8";
pub const CLEAR_ALL: &str = "\x1b[2J";
pub const DA1_QUERY: &str = "\x1b[c";
// Kitty keyboard protocol is not queried on Windows (issue #215).
#[cfg_attr(windows, allow(dead_code))]
pub const KITTY_QUERY: &str = "\x1b[?u";
pub const KITTY_POP: &str = "\x1b[<1u";

/// DEC private modes used by dekit itself (`CSI ? {n} h/l`).
#[derive(Clone, Copy, Debug)]
pub enum DecMode {
  ShowCursor = 25,
  MousePressRelease = 1000,
  MouseButtonMotion = 1002,
  MouseAnyMotion = 1003,
  FocusEvents = 1004,
  MouseRxvt = 1015,
  MouseSgr = 1006,
  AltScreen = 1049,
  BracketedPaste = 2004,
  SyncOutput = 2026,
  Win32Input = 9001,
}

fn num(out: &mut Vec<u8>, n: i64) {
  let mut buf = itoa::Buffer::new();
  out.extend_from_slice(buf.format(n).as_bytes());
}

pub fn dec_set(out: &mut Vec<u8>, mode: DecMode) {
  out.extend_from_slice(b"\x1b[?");
  num(out, mode as i64);
  out.push(b'h');
}

pub fn dec_reset(out: &mut Vec<u8>, mode: DecMode) {
  out.extend_from_slice(b"\x1b[?");
  num(out, mode as i64);
  out.push(b'l');
}

/// Kitty keyboard protocol: push the given flags.
pub fn kitty_push(out: &mut Vec<u8>, flags: u8) {
  out.extend_from_slice(b"\x1b[>");
  num(out, flags as i64);
  out.push(b'u');
}

/// Reply to a kitty keyboard flags query.
pub fn kitty_flags_reply(out: &mut Vec<u8>, flags: u8) {
  out.extend_from_slice(b"\x1b[?");
  num(out, flags as i64);
  out.push(b'u');
}

/// xterm modifyOtherKeys level (0 disables).
pub fn modify_other_keys(out: &mut Vec<u8>, level: u8) {
  out.extend_from_slice(b"\x1b[>4;");
  num(out, level as i64);
  out.push(b'm');
}

/// Cursor position; takes 0-based coordinates.
pub fn cup(out: &mut Vec<u8>, row: u16, col: u16) {
  out.extend_from_slice(b"\x1b[");
  num(out, row as i64 + 1);
  out.push(b';');
  num(out, col as i64 + 1);
  out.push(b'H');
}

pub fn cursor_style(out: &mut Vec<u8>, style: CursorStyle) {
  out.extend_from_slice(b"\x1b[");
  num(out, style as i64);
  out.extend_from_slice(b" q");
}

// Replies a screen sends to the program that asked.

/// Primary Device Attributes: VT500 with selective erase, ANSI color,
/// clipboard access, and sixel.
pub fn da1_reply(out: &mut Vec<u8>) {
  // No sixel (4): a program then picks a better protocol it also has,
  // such as kitty graphics; sixel sent anyway is still decoded.
  out.extend_from_slice(b"\x1b[?65;6;22;52c");
}

/// Secondary Device Attributes: an xterm-compatible with dekit's
/// version as the firmware number.
pub fn da2_reply(out: &mut Vec<u8>) {
  out.extend_from_slice(b"\x1b[>41;");
  num(out, version_number());
  out.extend_from_slice(b";0c");
}

/// Tertiary Device Attributes: a unit id of zeros.
pub fn da3_reply(out: &mut Vec<u8>) {
  out.extend_from_slice(b"\x1bP!|00000000\x1b\\");
}

/// XTVERSION: `DCS > | dekit x.y.z ST`.
pub fn xtversion_reply(out: &mut Vec<u8>) {
  out.extend_from_slice(b"\x1bP>|dekit ");
  out.extend_from_slice(env!("CARGO_PKG_VERSION").as_bytes());
  out.extend_from_slice(b"\x1b\\");
}

/// The version as xterm's patch number: major*10000 + minor*100 + patch.
fn version_number() -> i64 {
  let mut parts = env!("CARGO_PKG_VERSION")
    .split(['.', '-'])
    .map(|p| p.parse::<i64>().unwrap_or(0));
  let major = parts.next().unwrap_or(0);
  let minor = parts.next().unwrap_or(0);
  let patch = parts.next().unwrap_or(0);
  major * 10000 + minor * 100 + patch
}

/// Cursor Position Report; takes 0-based coordinates.
pub fn cpr(out: &mut Vec<u8>, row: u16, col: u16) {
  out.extend_from_slice(b"\x1b[");
  num(out, row as i64 + 1);
  out.push(b';');
  num(out, col as i64 + 1);
  out.push(b'R');
}

/// DECXCPR: like CPR with a page number.
pub fn decxcpr(out: &mut Vec<u8>, row: u16, col: u16) {
  out.extend_from_slice(b"\x1b[?");
  num(out, row as i64 + 1);
  out.push(b';');
  num(out, col as i64 + 1);
  out.extend_from_slice(b";1R");
}

/// DECRPM: the answer to DECRQM.
pub fn decrqm_reply(out: &mut Vec<u8>, dec: bool, mode: u32, state: u8) {
  out.extend_from_slice(if dec { b"\x1b[?" } else { b"\x1b[" });
  num(out, mode as i64);
  out.push(b';');
  num(out, state as i64);
  out.extend_from_slice(b"$y");
}

/// XTWINOPS report: `CSI code ; a ; b t`.
pub fn window_report(out: &mut Vec<u8>, code: u32, a: u32, b: u32) {
  out.extend_from_slice(b"\x1b[");
  num(out, code as i64);
  out.push(b';');
  num(out, a as i64);
  out.push(b';');
  num(out, b as i64);
  out.push(b't');
}

/// In-band resize notification (`?2048`).
pub fn inband_resize(
  out: &mut Vec<u8>,
  rows: u16,
  cols: u16,
  height_px: u32,
  width_px: u32,
) {
  out.extend_from_slice(b"\x1b[48;");
  num(out, rows as i64);
  out.push(b';');
  num(out, cols as i64);
  out.push(b';');
  num(out, height_px as i64);
  out.push(b';');
  num(out, width_px as i64);
  out.push(b't');
}

/// OSC 4/10/11/12 color report: `OSC code ; [idx ;] rgb:rr/gg/bb ST`.
pub fn osc_color_reply(
  out: &mut Vec<u8>,
  code: u32,
  idx: Option<u8>,
  rgb: Rgb,
) {
  out.extend_from_slice(b"\x1b]");
  num(out, code as i64);
  out.push(b';');
  if let Some(idx) = idx {
    num(out, idx as i64);
    out.push(b';');
  }
  out.extend_from_slice(
    format!(
      "rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}",
      rgb.0, rgb.0, rgb.1, rgb.1, rgb.2, rgb.2
    )
    .as_bytes(),
  );
  out.extend_from_slice(b"\x1b\\");
}

/// XTGETTCAP reply: `DCS 1 + r name=value ST`, or `DCS 0 + r ST` for a
/// capability we do not have. `name` is still hex-encoded.
pub fn xtgettcap_reply(
  out: &mut Vec<u8>,
  valid: bool,
  name_hex: &[u8],
  value: Option<&str>,
) {
  if valid {
    out.extend_from_slice(b"\x1bP1+r");
    out.extend_from_slice(name_hex);
    if let Some(value) = value {
      out.push(b'=');
      for b in value.as_bytes() {
        out.extend_from_slice(format!("{b:02X}").as_bytes());
      }
    }
  } else {
    out.extend_from_slice(b"\x1bP0+r");
  }
  out.extend_from_slice(b"\x1b\\");
}

/// DECRPSS: `DCS 1 $ r payload ST`, or `DCS 0 $ r ST` when invalid.
pub fn decrqss_reply(out: &mut Vec<u8>, valid: bool, payload: &[u8]) {
  if valid {
    out.extend_from_slice(b"\x1bP1$r");
    out.extend_from_slice(payload);
  } else {
    out.extend_from_slice(b"\x1bP0$r");
  }
  out.extend_from_slice(b"\x1b\\");
}

// Styles

fn color_params(out: &mut Vec<u8>, base: u8, color: Color) {
  // Foreground 30..37/90..97, extended 38, default 39; background the
  // same from 40; underline color only has the extended form 58 and
  // the default 59.
  let (extended, default) = if base == 58 {
    (58, 59)
  } else {
    (base + 8, base + 9)
  };
  match color {
    Color::Default => num(out, default as i64),
    Color::Idx(idx) if idx < 8 && base != 58 => num(out, (base + idx) as i64),
    Color::Idx(idx) if idx < 16 && base != 58 => {
      num(out, (base + 60 + idx - 8) as i64)
    }
    Color::Idx(idx) => {
      num(out, extended as i64);
      if base == 58 {
        out.extend_from_slice(b":5:");
      } else {
        out.extend_from_slice(b";5;");
      }
      num(out, idx as i64);
    }
    Color::Rgb(r, g, b) => {
      num(out, extended as i64);
      if base == 58 {
        out.extend_from_slice(b":2::");
        num(out, r as i64);
        out.push(b':');
        num(out, g as i64);
        out.push(b':');
        num(out, b as i64);
      } else {
        out.extend_from_slice(b";2;");
        num(out, r as i64);
        out.push(b';');
        num(out, g as i64);
        out.push(b';');
        num(out, b as i64);
      }
    }
  }
}

fn underline_params(out: &mut Vec<u8>, shape: Underline) {
  match shape {
    Underline::None => num(out, 24),
    Underline::Single => num(out, 4),
    shape => {
      out.extend_from_slice(b"4:");
      num(out, shape as i64);
    }
  }
}

/// The SGR transition from one style to another. Writes nothing when
/// they are equal. Links are not SGR: see `osc_link`.
pub fn sgr(out: &mut Vec<u8>, from: Style, to: Style) {
  let from = from.with_link(Default::default());
  let to = to.with_link(Default::default());
  if from == to {
    return;
  }
  if to == Style::DEFAULT {
    out.extend_from_slice(SGR_RESET.as_bytes());
    return;
  }
  out.extend_from_slice(b"\x1b[");
  let mut first = true;
  let mut sep = |out: &mut Vec<u8>| {
    if first {
      first = false;
    } else {
      out.push(b';');
    }
  };
  // 22 clears both bold and dim; what stays on is set again.
  let intensity_off = (from.bold() && !to.bold()) || (from.dim() && !to.dim());
  if intensity_off {
    sep(out);
    num(out, 22);
  }
  if to.bold() && (intensity_off || !from.bold()) {
    sep(out);
    num(out, 1);
  }
  if to.dim() && (intensity_off || !from.dim()) {
    sep(out);
    num(out, 2);
  }
  if from.italic() != to.italic() {
    sep(out);
    num(out, if to.italic() { 3 } else { 23 });
  }
  if from.underline() != to.underline() {
    sep(out);
    underline_params(out, to.underline());
  }
  if from.blink() != to.blink() {
    sep(out);
    num(out, if to.blink() { 5 } else { 25 });
  }
  if from.inverse() != to.inverse() {
    sep(out);
    num(out, if to.inverse() { 7 } else { 27 });
  }
  if from.hidden() != to.hidden() {
    sep(out);
    num(out, if to.hidden() { 8 } else { 28 });
  }
  if from.strike() != to.strike() {
    sep(out);
    num(out, if to.strike() { 9 } else { 29 });
  }
  if from.overline() != to.overline() {
    sep(out);
    num(out, if to.overline() { 53 } else { 55 });
  }
  if from.fg != to.fg {
    sep(out);
    color_params(out, 30, to.fg);
  }
  if from.bg != to.bg {
    sep(out);
    color_params(out, 40, to.bg);
  }
  if from.ul != to.ul {
    sep(out);
    color_params(out, 58, to.ul);
  }
  out.push(b'm');
}

/// The SGR parameters that set `style` from scratch, without the CSI
/// framing: `0;1;31`. For DECRQSS.
pub fn sgr_full(out: &mut Vec<u8>, style: Style) {
  out.push(b'0');
  let mut seq = Vec::new();
  sgr(&mut seq, Style::DEFAULT, style);
  if let Some(params) = seq
    .strip_prefix(b"\x1b[")
    .and_then(|s| s.strip_suffix(b"m"))
    && !params.is_empty()
    && params != b"0"
  {
    out.push(b';');
    out.extend_from_slice(params);
  }
  out.push(b'm');
}

/// OSC 8: opens a hyperlink, or closes the current one with `None`.
pub fn osc_link(out: &mut Vec<u8>, link: Option<&Link>) {
  out.extend_from_slice(b"\x1b]8;");
  if let Some(link) = link {
    if let Some(id) = &link.id {
      out.extend_from_slice(b"id=");
      out.extend_from_slice(id.as_bytes());
    }
    out.push(b';');
    out.extend_from_slice(link.uri.as_bytes());
  } else {
    out.push(b';');
  }
  out.extend_from_slice(b"\x1b\\");
}

/// Pasted text for a pty, wrapped in the bracketed paste markers when the
/// receiving program enabled them. Control characters that could end the
/// paste early or run commands are dropped, as kitty does.
pub fn paste(out: &mut Vec<u8>, text: &str, bracketed: bool) {
  if bracketed {
    out.extend_from_slice(b"\x1b[200~");
  }
  for c in text.chars() {
    match c {
      '\t' | '\n' | '\r' => out.push(c as u8),
      c if (c as u32) < 0x20
        || c == '\x7f'
        || ('\u{80}'..'\u{a0}').contains(&c) => {}
      c => {
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
      }
    }
  }
  if bracketed {
    out.extend_from_slice(b"\x1b[201~");
  }
}

/// OSC 2: set the window title.
pub fn osc_title(out: &mut Vec<u8>, title: &str) {
  out.extend_from_slice(b"\x1b]2;");
  out.extend_from_slice(title.as_bytes());
  out.push(0x07);
}

/// OSC 52: copy text to the clipboard through the outer terminal.
pub fn osc52_copy(out: &mut Vec<u8>, text: &str) {
  out.extend_from_slice(b"\x1b]52;;");
  let encoded = base64::engine::general_purpose::STANDARD.encode(text);
  out.extend_from_slice(encoded.as_bytes());
  out.push(0x07);
}

/// Focus in (`CSI I`) or out (`CSI O`) for a program that asked.
pub fn focus(out: &mut Vec<u8>, gained: bool) {
  out.extend_from_slice(if gained { b"\x1b[I" } else { b"\x1b[O" });
}

/// A mouse report in the encoding the receiving program enabled.
/// `cell_px` is used for pixel reports.
pub fn mouse(
  out: &mut Vec<u8>,
  event: &MouseEvent,
  encoding: MouseProtocolEncoding,
  cell_px: (u16, u16),
) {
  let mods = mouse_mods_bits(event.mods);
  match encoding {
    // SGR: CSI < Cb ; Cx ; Cy M/m
    MouseProtocolEncoding::Sgr | MouseProtocolEncoding::SgrPixels => {
      let base: u8 = match event.kind {
        MouseEventKind::Down(btn) | MouseEventKind::Up(btn) => {
          mouse_button_bits(btn)
        }
        MouseEventKind::Drag(btn) => 32 + mouse_button_bits(btn),
        MouseEventKind::Moved => 35,
        MouseEventKind::ScrollUp => 64,
        MouseEventKind::ScrollDown => 65,
        MouseEventKind::ScrollLeft => 66,
        MouseEventKind::ScrollRight => 67,
      };
      let (x, y) = if encoding == MouseProtocolEncoding::SgrPixels {
        // The center of the cell, in pixels.
        (
          event.x as i64 * cell_px.0 as i64 + cell_px.0 as i64 / 2,
          event.y as i64 * cell_px.1 as i64 + cell_px.1 as i64 / 2,
        )
      } else {
        (event.x as i64 + 1, event.y as i64 + 1)
      };
      out.extend_from_slice(b"\x1b[<");
      num(out, (base | mods) as i64);
      out.push(b';');
      num(out, x);
      out.push(b';');
      num(out, y);
      out.push(match event.kind {
        MouseEventKind::Up(_) => b'm',
        MouseEventKind::Down(_)
        | MouseEventKind::Drag(_)
        | MouseEventKind::Moved
        | MouseEventKind::ScrollDown
        | MouseEventKind::ScrollUp
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => b'M',
      });
    }
    // urxvt: CSI Cb ; Cx ; Cy M, with Cb offset by 32; a release loses
    // its button.
    MouseProtocolEncoding::Urxvt => {
      let base = legacy_button_byte(event.kind);
      out.extend_from_slice(b"\x1b[");
      num(out, (base | mods) as i64 + 32);
      out.push(b';');
      num(out, event.x as i64 + 1);
      out.push(b';');
      num(out, event.y as i64 + 1);
      out.push(b'M');
    }
    // Legacy byte encodings: CSI M Cb Cx Cy; a release loses its button.
    MouseProtocolEncoding::Default | MouseProtocolEncoding::Utf8 => {
      let base = legacy_button_byte(event.kind);
      out.extend_from_slice(b"\x1b[M");
      out.push((base | mods) + 32);
      let x = event.x.max(0) as u32 + 33;
      let y = event.y.max(0) as u32 + 33;
      match encoding {
        MouseProtocolEncoding::Default => {
          out.push(x.min(255) as u8);
          out.push(y.min(255) as u8);
        }
        _ => {
          mouse_coord_utf8(out, x);
          mouse_coord_utf8(out, y);
        }
      }
    }
  }
}

fn legacy_button_byte(kind: MouseEventKind) -> u8 {
  match kind {
    MouseEventKind::Down(btn) => mouse_button_bits(btn),
    MouseEventKind::Up(_) => 3,
    MouseEventKind::Drag(btn) => 32 + mouse_button_bits(btn),
    MouseEventKind::Moved => 35,
    MouseEventKind::ScrollUp => 64,
    MouseEventKind::ScrollDown => 65,
    MouseEventKind::ScrollLeft => 66,
    MouseEventKind::ScrollRight => 67,
  }
}

fn mouse_button_bits(btn: MouseButton) -> u8 {
  match btn {
    MouseButton::Left => 0,
    MouseButton::Middle => 1,
    MouseButton::Right => 2,
  }
}

fn mouse_mods_bits(mods: KeyMods) -> u8 {
  let mut bits = 0;
  if mods.contains(KeyMods::SHIFT) {
    bits |= 4;
  }
  if mods.contains(KeyMods::ALT) {
    bits |= 8;
  }
  if mods.contains(KeyMods::CONTROL) {
    bits |= 16;
  }
  bits
}

/// The 1005 extension encodes coordinates as UTF-8 (up to 2 bytes).
fn mouse_coord_utf8(out: &mut Vec<u8>, value: u32) {
  let value = value.min(2047);
  if value < 128 {
    out.push(value as u8);
  } else {
    out.push(0xC0 | (value >> 6) as u8);
    out.push(0x80 | (value & 0x3F) as u8);
  }
}

/// Terminal modes that change how a key is encoded for the pty.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyEncodeModes {
  /// The kitty keyboard protocol flags the program pushed; 0 is the
  /// legacy encoding.
  pub kitty_flags: u8,
  pub application_cursor_keys: bool,
  pub newline_mode: bool,
}

/// Writes the byte sequence for this key as the program asked for it:
/// the kitty keyboard protocol with the flags it pushed, else the
/// xterm-compatible legacy encoding. Keys with no encoding write nothing.
pub fn key(out: &mut Vec<u8>, key: &Key, modes: KeyEncodeModes) {
  #[cfg(windows)]
  if key_win32(out, key) {
    return;
  }
  if modes.kitty_flags != 0 {
    key_kitty(out, key, modes.kitty_flags);
  } else if key.kind != KeyEventKind::Release {
    key_legacy(out, key, modes);
  }
}

/// How a key is written in the kitty protocol's functional key table.
enum Functional {
  /// `CSI 1 ; mods X`, `CSI X` unmodified.
  Letter(u8),
  /// `CSI n ; mods ~`, `CSI n ~` unmodified.
  Tilde(u16),
  /// `CSI n ; mods u`.
  Code(u32),
}

fn functional(code: KeyCode, keypad: bool) -> Option<Functional> {
  if let Some(cp) = keys::functional_codepoint(code, keypad) {
    return Some(Functional::Code(cp));
  }
  Some(match code {
    KeyCode::Esc => Functional::Code(27),
    KeyCode::Enter => Functional::Code(13),
    KeyCode::Tab => Functional::Code(9),
    KeyCode::Backspace => Functional::Code(127),
    KeyCode::Up => Functional::Letter(b'A'),
    KeyCode::Down => Functional::Letter(b'B'),
    KeyCode::Right => Functional::Letter(b'C'),
    KeyCode::Left => Functional::Letter(b'D'),
    KeyCode::Home => Functional::Letter(b'H'),
    KeyCode::End => Functional::Letter(b'F'),
    KeyCode::F(1) => Functional::Letter(b'P'),
    KeyCode::F(2) => Functional::Letter(b'Q'),
    KeyCode::F(4) => Functional::Letter(b'S'),
    KeyCode::Insert => Functional::Tilde(2),
    KeyCode::Delete => Functional::Tilde(3),
    KeyCode::PageUp => Functional::Tilde(5),
    KeyCode::PageDown => Functional::Tilde(6),
    KeyCode::F(n) if (3..=12).contains(&n) => {
      Functional::Tilde(keys::key_tilde(code)?)
    }
    _ => return None,
  })
}

/// A kitty graphics command: `ESC _ G control [; payload] ESC \\`.
pub fn kitty_graphics(out: &mut Vec<u8>, control: &str, payload: &[u8]) {
  out.extend_from_slice(b"\x1b_G");
  out.extend_from_slice(control.as_bytes());
  if !payload.is_empty() {
    out.push(b';');
    out.extend_from_slice(payload);
  }
  out.extend_from_slice(b"\x1b\\");
}

/// The `mods[:event]` field.
fn kitty_mods_field(
  out: &mut Vec<u8>,
  mods: u32,
  kind: KeyEventKind,
  flags: u8,
) {
  let event = match kind {
    KeyEventKind::Press => 0,
    KeyEventKind::Repeat
      if flags & crate::term::modes::KITTY_EVENT_TYPES != 0 =>
    {
      2
    }
    KeyEventKind::Release
      if flags & crate::term::modes::KITTY_EVENT_TYPES != 0 =>
    {
      3
    }
    KeyEventKind::Repeat | KeyEventKind::Release => 0,
  };
  num(out, mods as i64 + 1);
  if event != 0 {
    out.push(b':');
    num(out, event);
  }
}

fn key_kitty(out: &mut Vec<u8>, key: &Key, flags: u8) {
  use crate::term::modes::{
    KITTY_ALL_KEYS, KITTY_ALTERNATE_KEYS, KITTY_EVENT_TYPES, KITTY_TEXT,
  };
  let report_events = flags & KITTY_EVENT_TYPES != 0;
  if key.kind == KeyEventKind::Release && !report_events {
    return;
  }
  let all_keys = flags & KITTY_ALL_KEYS != 0;
  let keypad = key.state.contains(KeyEventState::KEYPAD);
  // Repeats and releases are only told apart when asked.
  let eventful = report_events && key.kind != KeyEventKind::Press;

  // The key as the protocol names it: unshifted, with the shifted and
  // base-layout forms as alternates.
  let (code, shifted, base): (char, Option<char>, Option<char>) = match key.code
  {
    KeyCode::Char(c) => match key.unshifted {
      Some(unshifted) if unshifted != c => (unshifted, Some(c), key.base),
      _ => {
        let lower = c.to_lowercase().next().unwrap_or(c);
        let upper = c.to_uppercase().next().unwrap_or(c);
        if lower != c && c.is_alphabetic() {
          (lower, Some(c), key.base)
        } else if key.mods.contains(KeyMods::SHIFT) && upper != c {
          (c, Some(upper), key.base)
        } else {
          (c, None, key.base)
        }
      }
    },
    _ => ('\0', None, None),
  };
  let mut mods = key.mods;
  if shifted.is_some() {
    mods |= KeyMods::SHIFT;
  }
  let mods_value = keys::kitty_mods(mods, key.state, all_keys);
  let plain_mods = mods & !KeyMods::SHIFT;

  if let KeyCode::Char(_) = key.code {
    let text_key = plain_mods.is_empty()
      || (!mods.intersects(
        KeyMods::CONTROL
          | KeyMods::ALT
          | KeyMods::SUPER
          | KeyMods::HYPER
          | KeyMods::META,
      ));
    let kp_code = keypad
      .then(|| keys::functional_codepoint(key.code, true))
      .flatten();
    let legacy_text = text_key && !all_keys && !eventful && kp_code.is_none();
    if legacy_text {
      // Plain text stays text; the program never asked for more.
      let c = shifted.unwrap_or(code);
      if mods.contains(KeyMods::ALT) {
        out.push(0x1b);
      }
      push_char(out, c);
      return;
    }
    out.extend_from_slice(b"\x1b[");
    match kp_code {
      Some(cp) if all_keys => num(out, cp as i64),
      _ => {
        num(out, code as i64);
        if flags & KITTY_ALTERNATE_KEYS != 0
          && (shifted.is_some() || base.is_some())
        {
          out.push(b':');
          if let Some(shifted) = shifted {
            num(out, shifted as i64);
          }
          if let Some(base) = base {
            out.push(b':');
            num(out, base as i64);
          }
        }
      }
    }
    let text = if flags & KITTY_TEXT != 0 && key.kind != KeyEventKind::Release {
      match &key.text {
        Some(text) => Some(text.clone()),
        None if text_key => Some(shifted.unwrap_or(code).to_string()),
        None => None,
      }
    } else {
      None
    };
    let text =
      text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control));
    if text.is_some() || mods_value != 0 || eventful {
      out.push(b';');
      kitty_mods_field(out, mods_value, key.kind, flags);
    }
    if let Some(text) = text {
      out.push(b';');
      let mut first = true;
      for c in text.chars() {
        if !first {
          out.push(b':');
        }
        first = false;
        num(out, c as i64);
      }
    }
    out.push(b'u');
    return;
  }

  let Some(form) = functional(key.code, keypad) else {
    return;
  };
  match form {
    Functional::Code(cp) => {
      // Enter, Tab and Backspace keep their legacy bytes until the
      // program asks for every key as an escape code, or modifies them.
      let legacy = match key.code {
        KeyCode::Enter => Some(b'\r'),
        KeyCode::Tab => Some(b'\t'),
        KeyCode::Backspace => Some(0x7f),
        _ => None,
      };
      if let Some(byte) = legacy
        && !all_keys
        && mods_value == 0
        && !eventful
        && !keypad
      {
        out.push(byte);
        return;
      }
      // Lock and modifier keys exist only when every key is reported.
      let only_with_all = matches!(
        key.code,
        KeyCode::CapsLock
          | KeyCode::ScrollLock
          | KeyCode::NumLock
          | KeyCode::PrintScreen
          | KeyCode::Pause
          | KeyCode::Menu
          | KeyCode::Modifier(_)
      );
      if only_with_all && !all_keys {
        return;
      }
      out.extend_from_slice(b"\x1b[");
      num(out, cp as i64);
      if mods_value != 0 || eventful {
        out.push(b';');
        kitty_mods_field(out, mods_value, key.kind, flags);
      }
      out.push(b'u');
    }
    Functional::Letter(letter) => {
      out.extend_from_slice(b"\x1b[");
      if mods_value != 0 || eventful {
        out.push(b'1');
        out.push(b';');
        kitty_mods_field(out, mods_value, key.kind, flags);
      }
      out.push(letter);
    }
    Functional::Tilde(n) => {
      out.extend_from_slice(b"\x1b[");
      num(out, n as i64);
      if mods_value != 0 || eventful {
        out.push(b';');
        kitty_mods_field(out, mods_value, key.kind, flags);
      }
      out.push(b'~');
    }
  }
}

/// The xterm-compatible encoding, for programs that asked for nothing
/// else.
fn key_legacy(out: &mut Vec<u8>, key: &Key, modes: KeyEncodeModes) {
  use KeyCode::*;

  let code = key.code;
  let mods = key.mods;

  // Normalize the modifier state for Char's that are uppercase; remove
  // the SHIFT modifier so that reduce ambiguity below
  let mods = match code {
    Char(c)
      if (c.is_ascii_punctuation() || c.is_ascii_uppercase())
        && mods.contains(KeyMods::SHIFT) =>
    {
      mods.difference(KeyMods::SHIFT)
    }
    _ => mods,
  };

  // Normalize Backspace and Delete
  let code = match code {
    Char('\x7f') => KeyCode::Backspace,
    Char('\x08') => KeyCode::Delete,
    c => c,
  };

  match code {
    Char(c) if c.is_ascii_uppercase() && mods.contains(KeyMods::CONTROL) => {
      legacy_modified_char(out, c, mods);
    }

    Char(c)
      if mods.contains(KeyMods::CONTROL) && keys::ctrl_char(c).is_some() =>
    {
      let c = keys::ctrl_char(c).unwrap();
      if mods.contains(KeyMods::ALT) {
        out.push(0x1b);
      }
      push_char(out, c);
    }

    // When alt is pressed, send escape first to indicate to the peer that
    // ALT is pressed.  We do this only for ascii alnum characters because
    // eg: on macOS generates altgr style glyphs and keeps the ALT key
    // in the modifier set.  This confuses eg: zsh which then just displays
    // <fffffffff> as the input, so we want to avoid that.
    Char(c)
      if (c.is_ascii_alphanumeric() || c.is_ascii_punctuation())
        && mods.contains(KeyMods::ALT) =>
    {
      out.push(0x1b);
      push_char(out, c);
    }

    Enter | Esc | Backspace => {
      let c = match code {
        Enter => '\r',
        Esc => '\x1b',
        // Backspace sends the default VERASE which is confusingly
        // the DEL ascii codepoint
        Backspace => '\x7f',
        _ => unreachable!(),
      };
      if mods.contains(KeyMods::SHIFT) || mods.contains(KeyMods::CONTROL) {
        legacy_modified_char(out, c, mods);
      } else {
        if mods.contains(KeyMods::ALT) {
          out.push(0x1b);
        }
        push_char(out, c);
        if modes.newline_mode && code == Enter {
          out.push(b'\n');
        }
      }
    }

    Tab => {
      if mods.contains(KeyMods::ALT) {
        out.push(0x1b);
      }
      let mods = mods & !KeyMods::ALT;
      if mods == KeyMods::CONTROL {
        out.extend_from_slice(b"\x1b[9;5u");
      } else if mods == KeyMods::CONTROL | KeyMods::SHIFT {
        out.extend_from_slice(b"\x1b[1;5Z");
      } else if mods == KeyMods::SHIFT {
        out.extend_from_slice(b"\x1b[Z");
      } else {
        out.push(b'\t');
      }
    }

    Char(c) => {
      if mods.is_empty() {
        push_char(out, c);
      } else {
        legacy_modified_char(out, c, mods);
      }
    }

    Home | End | Up | Down | Right | Left => {
      let c = keys::cursor_key_final(code).unwrap();
      if mods.intersects(KeyMods::ALT | KeyMods::SHIFT | KeyMods::CONTROL) {
        out.extend_from_slice(b"\x1b[1;");
        num(out, keys::mods_mask(mods) as i64 + 1);
        out.push(c);
      } else if modes.application_cursor_keys {
        // Use SS3 in application mode.
        // Strict reading of DECCKM suggests that application_cursor_keys
        // only applies when DECANM and DECKPAM are active, but that seems
        // to break unmodified cursor keys in vim
        out.extend_from_slice(b"\x1bO");
        out.push(c);
      } else {
        out.extend_from_slice(b"\x1b[");
        out.push(c);
      }
    }

    F(n) if mods.is_empty() && (1..5).contains(&n) => {
      // F1-F4 are encoded using SS3 if there are no modifiers
      out.extend_from_slice(b"\x1bO");
      out.push(b'P' + n - 1);
    }

    PageUp | PageDown | Insert | Delete | F(_) => {
      // Higher numbered F-keys plus modified keys are encoded using
      // `CSI n ~`.
      let Some(tilde) = keys::key_tilde(code) else {
        return;
      };
      out.extend_from_slice(b"\x1b[");
      num(out, tilde as i64);
      let mask = keys::mods_mask(mods);
      if mask != 0 {
        out.push(b';');
        num(out, mask as i64 + 1);
      }
      out.push(b'~');
    }

    Null | CapsLock | ScrollLock | NumLock | PrintScreen | Pause | Menu
    | KeypadBegin | Media(_) | Modifier(_) => (),
  }
}

/// A modified character without the kitty protocol: the control
/// character, an ESC prefix for Alt, the plain character otherwise.
fn legacy_modified_char(out: &mut Vec<u8>, c: char, mods: KeyMods) {
  let c = if mods.contains(KeyMods::CONTROL) {
    keys::ctrl_char(c).unwrap_or(c)
  } else {
    c
  };
  // Non-ascii glyphs (macOS Option keys) are sent alone; see the ALT
  // arm in `key_legacy` for why.
  if mods.contains(KeyMods::ALT) && c.is_ascii() {
    out.push(0x1b);
  }
  push_char(out, c);
}

fn push_char(out: &mut Vec<u8>, c: char) {
  let mut buf = [0u8; 4];
  out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// <https://github.com/microsoft/terminal/blob/main/doc/specs/%234999%20-%20Improved%20keyboard%20handling%20in%20Conpty.md>
/// Returns false when the key has no win32 encoding.
#[cfg(windows)]
fn key_win32(out: &mut Vec<u8>, key: &Key) -> bool {
  // <https://docs.microsoft.com/en-us/windows/console/key-event-record-str>
  // defines the dwControlKeyState values
  let mut control_key_state = 0;

  if key.mods().contains(KeyMods::SHIFT) {
    control_key_state |= windows::Win32::System::Console::SHIFT_PRESSED;
  }
  if key.mods().contains(KeyMods::ALT) {
    control_key_state |= windows::Win32::System::Console::LEFT_ALT_PRESSED;
  }
  if key.mods().contains(KeyMods::CONTROL) {
    control_key_state |= windows::Win32::System::Console::LEFT_CTRL_PRESSED;
  }

  let Some(vkey) = virtual_key_code(&key.code()) else {
    return false;
  };
  let uni = match key.code() {
    KeyCode::Char(c) => {
      let c = match c {
        // Delete key is transmitted as 0x0
        '\x7f' => '\x00',
        // Backspace key is transmitted as 0x8, 0x7f or 0x0
        '\x08' => {
          if key.mods().contains(KeyMods::CONTROL) {
            if key.mods().contains(KeyMods::ALT)
              || key.mods().contains(KeyMods::SHIFT)
            {
              '\x00'
            } else {
              '\x7f'
            }
          } else {
            '\x08'
          }
        }
        _ => c,
      };

      let c = if key.mods().contains(KeyMods::CONTROL) {
        // Ensure that we rewrite the unicode value to the ASCII CTRL
        // equivalent value.
        // <https://github.com/microsoft/terminal/issues/13134>
        keys::ctrl_char(c).unwrap_or(c)
      } else {
        c
      };
      c as u32
    }
    KeyCode::Backspace => 0x8,
    KeyCode::Enter => 0xd,
    KeyCode::Tab => 0x9,
    KeyCode::Delete => 0x7f,
    _ => 0,
  };

  let scan_code = 0;
  let key_down = 1;
  let repeat_count = 1;
  out.extend_from_slice(b"\x1b[");
  num(out, vkey.0 as i64);
  out.push(b';');
  num(out, scan_code);
  out.push(b';');
  num(out, uni as i64);
  out.push(b';');
  num(out, key_down);
  out.push(b';');
  num(out, control_key_state as i64);
  out.push(b';');
  num(out, repeat_count);
  out.push(b'_');
  true
}

/// <https://docs.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes>
#[cfg(windows)]
fn virtual_key_code(
  code: &KeyCode,
) -> Option<windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY> {
  use windows::Win32::UI::Input::KeyboardAndMouse::*;

  let code = match code {
    KeyCode::Char(c) => match c {
      '0'..='9' => VIRTUAL_KEY(*c as u16),
      'a'..='z' => VIRTUAL_KEY(c.to_ascii_uppercase() as u16),
      ' ' => VK_SPACE,
      '*' => VK_MULTIPLY,
      '+' => VK_ADD,
      ',' => VK_SEPARATOR,
      '-' => VK_SUBTRACT,
      '.' => VK_DECIMAL,
      '/' => VK_DIVIDE,
      _ => return None,
    },
    KeyCode::Backspace => VK_BACK,
    KeyCode::Enter => VK_RETURN,
    KeyCode::Left => VK_LEFT,
    KeyCode::Right => VK_RIGHT,
    KeyCode::Up => VK_UP,
    KeyCode::Down => VK_DOWN,
    KeyCode::Home => VK_HOME,
    KeyCode::End => VK_END,
    KeyCode::PageUp => VK_PRIOR,
    KeyCode::PageDown => VK_NEXT,
    KeyCode::Tab => VK_TAB,
    KeyCode::Delete => VK_DELETE,
    KeyCode::Insert => VK_INSERT,
    KeyCode::F(n) => match n {
      1..=24 => VIRTUAL_KEY(VK_F1.0 - 1 + *n as u16),
      _ => return None,
    },
    KeyCode::Esc => VK_ESCAPE,
    KeyCode::Null
    | KeyCode::CapsLock
    | KeyCode::ScrollLock
    | KeyCode::NumLock
    | KeyCode::PrintScreen
    | KeyCode::Pause
    | KeyCode::Menu
    | KeyCode::KeypadBegin
    | KeyCode::Media(_)
    | KeyCode::Modifier(_) => VIRTUAL_KEY(0),
  };

  Some(code)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basics() {
    let mut out = Vec::new();
    dec_set(&mut out, DecMode::AltScreen);
    assert_eq!(out, b"\x1b[?1049h");

    out.clear();
    dec_reset(&mut out, DecMode::ShowCursor);
    assert_eq!(out, b"\x1b[?25l");

    out.clear();
    cup(&mut out, 0, 0);
    assert_eq!(out, b"\x1b[1;1H");

    out.clear();
    cursor_style(&mut out, CursorStyle::BlinkingBar);
    assert_eq!(out, b"\x1b[5 q");

    out.clear();
    kitty_push(&mut out, 15);
    assert_eq!(out, b"\x1b[>15u");

    out.clear();
    modify_other_keys(&mut out, 2);
    assert_eq!(out, b"\x1b[>4;2m");

    out.clear();
    decrqm_reply(&mut out, true, 2026, 2);
    assert_eq!(out, b"\x1b[?2026;2$y");

    out.clear();
    osc_color_reply(&mut out, 11, None, Rgb(0x16, 0x1b, 0x22));
    assert_eq!(out, b"\x1b]11;rgb:1616/1b1b/2222\x1b\\");

    out.clear();
    xtgettcap_reply(&mut out, true, b"5463", Some(""));
    assert_eq!(out, b"\x1bP1+r5463=\x1b\\");

    out.clear();
    inband_resize(&mut out, 24, 80, 384, 640);
    assert_eq!(out, b"\x1b[48;24;80;384;640t");
  }

  // On Windows `key` always emits win32-input-mode (see `key_win32`).
  #[cfg(not(windows))]
  #[test]
  fn key_kitty_disambiguate() {
    fn enc(spec: &str, flags: u8) -> Vec<u8> {
      let mut out = Vec::new();
      let modes = KeyEncodeModes {
        kitty_flags: flags,
        ..KeyEncodeModes::default()
      };
      key(&mut out, &Key::parse(spec).unwrap(), modes);
      out
    }
    // Legacy encoding when the program did not ask for the flag.
    assert_eq!(enc("<Esc>", 0), b"\x1b");
    assert_eq!(enc("<C-c>", 0), b"\x03");
    assert_eq!(enc("<M-j>", 0), b"\x1bj");
    assert_eq!(enc("<C-Enter>", 0), b"\r");
    assert_eq!(enc("<Up>", 0), b"\x1b[A");
    assert_eq!(enc("<F5>", 0), b"\x1b[15~");
    // With the flag, anything a peer could mistake for an ESC prefix is
    // CSI u; plain text and unmodified special keys stay legacy.
    assert_eq!(enc("<Esc>", 1), b"\x1b[27u");
    assert_eq!(enc("<M-Esc>", 1), b"\x1b[27;3u");
    assert_eq!(enc("<C-c>", 1), b"\x1b[99;5u");
    assert_eq!(enc("<M-j>", 1), b"\x1b[106;3u");
    assert_eq!(enc("<C-M-a>", 1), b"\x1b[97;7u");
    assert_eq!(enc("<C-Enter>", 1), b"\x1b[13;5u");
    assert_eq!(enc("<M-Enter>", 1), b"\x1b[13;3u");
    assert_eq!(enc("<M-BS>", 1), b"\x1b[127;3u");
    assert_eq!(enc("<M-Tab>", 1), b"\x1b[9;3u");
    assert_eq!(enc("<a>", 1), b"a");
    assert_eq!(enc("<A>", 1), b"A");
    assert_eq!(enc("<Enter>", 1), b"\r");
    assert_eq!(enc("<Up>", 1), b"\x1b[A");
    assert_eq!(enc("<S-Up>", 1), b"\x1b[1;2A");
    assert_eq!(enc("<C-F5>", 1), b"\x1b[15;5~");
    assert_eq!(enc("<F13>", 1), b"\x1b[57376u");
    assert_eq!(enc("<MediaPlay>", 1), b"\x1b[57428u");
    // Lock keys only when everything is reported.
    assert_eq!(enc("<CapsLock>", 1), b"");
    assert_eq!(enc("<CapsLock>", 9), b"\x1b[57358u");
    // All keys as escape codes: text too, unshifted with Shift as a
    // modifier; alternates when asked.
    assert_eq!(enc("<a>", 9), b"\x1b[97u");
    assert_eq!(enc("<A>", 9), b"\x1b[97;2u");
    assert_eq!(enc("<A>", 13), b"\x1b[97:65;2u");
    assert_eq!(enc("<Enter>", 9), b"\x1b[13u");
    assert_eq!(enc("<Up>", 9), b"\x1b[A");
    // Associated text.
    assert_eq!(enc("<a>", 25), b"\x1b[97;1;97u");
    assert_eq!(enc("<A>", 29), b"\x1b[97:65;2;65u");
    assert_eq!(enc("<C-a>", 25), b"\x1b[97;5u");
  }

  #[cfg(not(windows))]
  #[test]
  fn key_kitty_events_and_layout() {
    let modes = KeyEncodeModes {
      kitty_flags: 3,
      ..KeyEncodeModes::default()
    };
    let mut out = Vec::new();
    let release = Key::new_with_kind(
      KeyCode::Char('a'),
      KeyMods::NONE,
      KeyEventKind::Release,
    );
    key(&mut out, &release, modes);
    assert_eq!(out, b"\x1b[97;1:3u");
    out.clear();
    let repeat =
      Key::new_with_kind(KeyCode::Up, KeyMods::NONE, KeyEventKind::Repeat);
    key(&mut out, &repeat, modes);
    assert_eq!(out, b"\x1b[1;1:2A");
    // Without the flag, releases are dropped and repeats are presses.
    out.clear();
    let legacy = KeyEncodeModes {
      kitty_flags: 1,
      ..KeyEncodeModes::default()
    };
    key(&mut out, &release, legacy);
    assert_eq!(out, b"");
    key(&mut out, &repeat, legacy);
    assert_eq!(out, b"\x1b[A");
    // A base-layout key: Cyrillic Ф on the a key, with alternates.
    out.clear();
    let mut layout = Key::new(KeyCode::Char('\u{444}'), KeyMods::CONTROL);
    layout.base = Some('a');
    key(
      &mut out,
      &layout,
      KeyEncodeModes {
        kitty_flags: 5,
        ..KeyEncodeModes::default()
      },
    );
    assert_eq!(out, b"\x1b[1092::97;5u");
    // Keypad keys have their own codes once every key is reported.
    out.clear();
    let mut kp = Key::new(KeyCode::Char('5'), KeyMods::NONE);
    kp.state = KeyEventState::KEYPAD;
    key(
      &mut out,
      &kp,
      KeyEncodeModes {
        kitty_flags: 9,
        ..KeyEncodeModes::default()
      },
    );
    assert_eq!(out, b"\x1b[57404u");
  }

  #[test]
  fn sgr_transition() {
    let mut out = Vec::new();
    sgr(&mut out, Style::DEFAULT, Style::DEFAULT);
    assert_eq!(out, b"");

    let colored = Style::default().fg(Color::Idx(4));
    sgr(&mut out, Style::DEFAULT, colored);
    assert_eq!(out, b"\x1b[34m");

    out.clear();
    let bold = Style::default()
      .fg(Color::Rgb(1, 2, 3))
      .bg(Color::Idx(7))
      .set_bold(true);
    sgr(&mut out, colored, bold);
    assert_eq!(out, b"\x1b[1;38;2;1;2;3;47m");

    out.clear();
    sgr(&mut out, bold, Style::DEFAULT);
    assert_eq!(out, b"\x1b[0m");

    // 22 turns off bold and dim; dim that stays is set again.
    out.clear();
    let both = Style::default().set_bold(true).set_dim(true);
    sgr(&mut out, both, Style::default().set_dim(true));
    assert_eq!(out, b"\x1b[22;2m");

    out.clear();
    let curly = Style::default()
      .set_underline(Underline::Curly)
      .ul_color(Color::Rgb(9, 8, 7));
    sgr(&mut out, Style::DEFAULT, curly);
    assert_eq!(out, b"\x1b[4:3;58:2::9:8:7m");

    out.clear();
    let bright = Style::default().fg(Color::Idx(9)).bg(Color::Idx(200));
    sgr(&mut out, Style::DEFAULT, bright.set_strike(true));
    assert_eq!(out, b"\x1b[9;91;48;5;200m");

    out.clear();
    sgr_full(&mut out, bold);
    assert_eq!(out, b"0;1;38;2;1;2;3;47m");
  }

  #[test]
  fn mouse_encodings() {
    let down = MouseEvent {
      kind: MouseEventKind::Down(MouseButton::Left),
      x: 4,
      y: 9,
      mods: KeyMods::NONE,
    };
    let px = (10, 20);
    let mut out = Vec::new();
    mouse(&mut out, &down, MouseProtocolEncoding::Sgr, px);
    assert_eq!(out, b"\x1b[<0;5;10M");

    out.clear();
    mouse(&mut out, &down, MouseProtocolEncoding::SgrPixels, px);
    assert_eq!(out, b"\x1b[<0;45;190M");

    out.clear();
    mouse(&mut out, &down, MouseProtocolEncoding::Urxvt, px);
    assert_eq!(out, b"\x1b[32;5;10M");

    out.clear();
    mouse(
      &mut out,
      &MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Right),
        x: 0,
        y: 0,
        mods: KeyMods::NONE,
      },
      MouseProtocolEncoding::Sgr,
      px,
    );
    assert_eq!(out, b"\x1b[<2;1;1m");

    out.clear();
    mouse(
      &mut out,
      &MouseEvent {
        kind: MouseEventKind::ScrollUp,
        x: 4,
        y: 9,
        mods: KeyMods::CONTROL,
      },
      MouseProtocolEncoding::Sgr,
      px,
    );
    assert_eq!(out, b"\x1b[<80;5;10M");

    out.clear();
    mouse(&mut out, &down, MouseProtocolEncoding::Default, px);
    assert_eq!(out, b"\x1b[M\x20\x25\x2a");

    // Legacy encoding clamps coordinates it cannot represent.
    out.clear();
    mouse(
      &mut out,
      &MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        x: 500,
        y: 9,
        mods: KeyMods::NONE,
      },
      MouseProtocolEncoding::Default,
      px,
    );
    assert_eq!(out, b"\x1b[M\x20\xff\x2a");

    // The UTF-8 extension covers larger coordinates.
    out.clear();
    mouse(
      &mut out,
      &MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        x: 500,
        y: 9,
        mods: KeyMods::NONE,
      },
      MouseProtocolEncoding::Utf8,
      px,
    );
    assert_eq!(out, b"\x1b[M\x20\xc8\x95\x2a");
  }

  #[test]
  fn paste_wrapping_and_sanitizing() {
    let mut out = Vec::new();
    paste(&mut out, "hi\nthere", false);
    assert_eq!(out, b"hi\nthere");

    out.clear();
    paste(&mut out, "hi", true);
    assert_eq!(out, b"\x1b[200~hi\x1b[201~");

    // An escape in the paste cannot end it early.
    out.clear();
    paste(&mut out, "a\x1b[201~b\x07c", true);
    assert_eq!(out, b"\x1b[200~a[201~bc\x1b[201~");
  }

  #[test]
  fn links() {
    let mut out = Vec::new();
    osc_link(
      &mut out,
      Some(&Link {
        uri: "http://x".into(),
        id: Some("7".into()),
      }),
    );
    assert_eq!(out, b"\x1b]8;id=7;http://x\x1b\\");
    out.clear();
    osc_link(&mut out, None);
    assert_eq!(out, b"\x1b]8;;\x1b\\");
  }
}
