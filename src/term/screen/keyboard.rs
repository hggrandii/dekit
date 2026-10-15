//! The kitty keyboard protocol's flag stacks, one per screen.

use crate::term::{modes::KITTY_ALL_FLAGS, vt::Params};

use super::Screen;

/// Flags each screen keeps; the top entry is active.
#[derive(Clone, Debug, Default)]
pub struct KittyStack {
  flags: Vec<u8>,
}

const MAX_DEPTH: usize = 16;

impl KittyStack {
  pub fn active(&self) -> u8 {
    self.flags.last().copied().unwrap_or(0)
  }

  pub fn push(&mut self, flags: u8) {
    if self.flags.len() >= MAX_DEPTH {
      self.flags.remove(0);
    }
    self.flags.push(flags & KITTY_ALL_FLAGS);
  }

  pub fn pop(&mut self, n: usize) {
    let len = self.flags.len().saturating_sub(n.max(1));
    self.flags.truncate(len);
  }

  /// `CSI = flags ; mode u`: 1 sets, 2 adds, 3 removes.
  pub fn set(&mut self, flags: u8, mode: u32) {
    let flags = flags & KITTY_ALL_FLAGS;
    if self.flags.is_empty() {
      self.flags.push(0);
    }
    let top = self.flags.last_mut().expect("just pushed");
    match mode {
      2 => *top |= flags,
      3 => *top &= !flags,
      _ => *top = flags,
    }
  }

  pub fn entries(&self) -> &[u8] {
    &self.flags
  }

  pub fn from_entries(entries: &[u8]) -> KittyStack {
    KittyStack {
      flags: entries.iter().map(|f| f & KITTY_ALL_FLAGS).collect(),
    }
  }
}

impl Screen {
  /// `CSI ? u`, `CSI > flags u`, `CSI < n u`, `CSI = flags ; mode u`.
  pub(super) fn kitty_keyboard(&mut self, p: &Params) {
    match p.prefix {
      b'?' => {
        let flags = self.kitty.active();
        let mut out = Vec::new();
        crate::term::vt::emit::kitty_flags_reply(&mut out, flags);
        self.events.push(super::VtEvent::Reply(out));
      }
      b'>' => self.kitty.push(p.get(0, 0).min(255) as u8),
      b'<' => self.kitty.pop(p.get(0, 1).max(1) as usize),
      b'=' => self.kitty.set(p.get(0, 0).min(255) as u8, p.get(1, 1)),
      _ => (),
    }
  }

  /// Active kitty keyboard protocol flags (0 = legacy encoding).
  pub fn kitty_flags(&self) -> u8 {
    self.kitty.active()
  }
}
