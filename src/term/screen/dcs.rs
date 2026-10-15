//! Device control strings: terminfo queries (XTGETTCAP), setting
//! reports (DECRQSS), and sixel images.

use crate::term::vt::emit;

use super::{Screen, VtEvent};

impl Screen {
  pub(super) fn dcs(&mut self, data: &[u8]) {
    // The body starts with the parameters and intermediates of the DCS
    // introducer, then the final byte, then the payload.
    if let Some(rest) = data.strip_prefix(b"+q") {
      self.xtgettcap(rest);
    } else if let Some(rest) = data.strip_prefix(b"$q") {
      self.decrqss(rest);
    } else if is_sixel(data) {
      self.sixel(data);
    } else {
      log::debug!(
        "Ignored DCS {:?}",
        String::from_utf8_lossy(&data[..data.len().min(16)])
      );
    }
  }

  /// `DCS + q <hex names ;...> ST`: answers from what dekit itself does,
  /// never from the terminal the client sits in.
  fn xtgettcap(&mut self, names: &[u8]) {
    let mut out = Vec::new();
    for name in names.split(|b| *b == b';') {
      let Some(decoded) = unhex(name) else {
        continue;
      };
      let Ok(name_str) = std::str::from_utf8(&decoded) else {
        continue;
      };
      match termcap(name_str) {
        Some(value) => emit::xtgettcap_reply(&mut out, true, name, Some(value)),
        None => emit::xtgettcap_reply(&mut out, false, name, None),
      }
    }
    if !out.is_empty() {
      self.events.push(VtEvent::Reply(out));
    }
  }

  /// `DCS $ q <Pt> ST`: reports a setting as the sequence that sets it.
  fn decrqss(&mut self, what: &[u8]) {
    let mut payload = Vec::new();
    let valid = match what {
      b"m" => {
        emit::sgr_full(&mut payload, self.style);
        true
      }
      b"r" => {
        let (top, bottom) = self.grid().scroll_region();
        payload
          .extend_from_slice(format!("{};{}r", top + 1, bottom + 1).as_bytes());
        true
      }
      b"s" => {
        let (left, right) = self.grid().margins();
        payload
          .extend_from_slice(format!("{};{}s", left + 1, right + 1).as_bytes());
        true
      }
      b" q" => {
        payload.extend_from_slice(
          format!("{} q", self.cursor_style as u8).as_bytes(),
        );
        true
      }
      // DECSCL: a VT400 with 8-bit controls.
      b"\"p" => {
        payload.extend_from_slice(b"64;1\"p");
        true
      }
      // DECSCA
      b"\"q" => {
        payload.extend_from_slice(b"0\"q");
        true
      }
      _ => false,
    };
    let mut out = Vec::new();
    emit::decrqss_reply(&mut out, valid, &payload);
    self.events.push(VtEvent::Reply(out));
  }

  /// A sixel image is decoded into the image store and placed at the
  /// cursor, which moves below it.
  fn sixel(&mut self, data: &[u8]) {
    let Some(image) = crate::term::image::sixel::decode(data) else {
      return;
    };
    let data = crate::term::image::ImageData {
      format: crate::term::image::Format::Rgba,
      zlib: false,
      width: image.width,
      height: image.height,
      bytes: bytes::Bytes::from(image.pixels),
    };
    let id = self.images.allocate_id();
    let stored = self.images.insert(id, 0, data);
    let command = crate::term::image::kitty::Command {
      action: b'T',
      quiet: 2,
      id,
      ..crate::term::image::kitty::Command::default()
    };
    // A sixel image lives in its cells: writing over them erases it.
    self.place_image(&stored, &command, true);
    // Sixel leaves the cursor on the line after the image.
    let style = self.style_id();
    let (left, _) = self.grid().margins();
    self.grid_mut().row_inc_scroll(1, style);
    self.grid_mut().col_set(left);
  }
}

fn is_sixel(data: &[u8]) -> bool {
  // `DCS Pa ; Pb ; Ph q <sixel data> ST`: digits and semicolons, then q.
  let mut i = 0;
  while i < data.len() && (data[i].is_ascii_digit() || data[i] == b';') {
    i += 1;
  }
  data.get(i) == Some(&b'q')
}

fn unhex(hex: &[u8]) -> Option<Vec<u8>> {
  if !hex.len().is_multiple_of(2) {
    return None;
  }
  hex
    .chunks(2)
    .map(|pair| {
      let s = std::str::from_utf8(pair).ok()?;
      u8::from_str_radix(s, 16).ok()
    })
    .collect()
}

/// The terminfo capabilities dekit's screens have. Values are what the
/// `xterm-256color` entry says where that entry is right about dekit,
/// and what dekit does where it is not.
fn termcap(name: &str) -> Option<&'static str> {
  Some(match name {
    "TN" | "name" => "xterm-256color",
    "Co" | "colors" => "256",
    "RGB" => "8/8/8",
    "Tc" => "",
    "setrgbf" => "\x1b[38:2::%p1%d:%p2%d:%p3%dm",
    "setrgbb" => "\x1b[48:2::%p1%d:%p2%d:%p3%dm",
    "setaf" => {
      "\x1b[%?%p1%{8}%<%t3%p1%d%e%p1%{16}%<%t9%p1%{8}%-%d%e38;5;%p1%d%;m"
    }
    "setab" => {
      "\x1b[%?%p1%{8}%<%t4%p1%d%e%p1%{16}%<%t10%p1%{8}%-%d%e48;5;%p1%d%;m"
    }
    "Smulx" => "\x1b[4:%p1%dm",
    "Setulc" => {
      "\x1b[58:2::%p1%{65536}%/%d:%p1%{256}%/%{255}%&%d:%p1%{255}%&%dm"
    }
    "Su" => "",
    "Sync" => "\x1b[?2026%?%p1%{1}%-%tl%eh%;",
    "Ms" => "\x1b]52;%p1%s;%p2%s\x1b\\",
    "Se" => "\x1b[ q",
    "Ss" => "\x1b[%p1%d q",
    "Cs" => "\x1b]12;%p1%s\x1b\\",
    "Cr" => "\x1b]112\x1b\\",
    "smcup" => "\x1b[?1049h\x1b[22;0;0t",
    "rmcup" => "\x1b[?1049l\x1b[23;0;0t",
    "smkx" => "\x1b[?1h\x1b=",
    "rmkx" => "\x1b[?1l\x1b>",
    "kbs" => "\x7f",
    "kdch1" => "\x1b[3~",
    "khome" => "\x1bOH",
    "kend" => "\x1bOF",
    "kcuu1" => "\x1bOA",
    "kcud1" => "\x1bOB",
    "kcuf1" => "\x1bOC",
    "kcub1" => "\x1bOD",
    "cols" => "80",
    "lines" => "24",
    "XF" => "",
    "fsl" | "tsl" | "dsl" => "",
    "bel" => "\x07",
    "u8" => "\x1b[?%[;0123456789]c",
    "u7" => "\x1b[6n",
    "u6" => "\x1b[%i%d;%dR",
    _ => return None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hex_and_sixel_detection() {
    assert_eq!(unhex(b"5463").unwrap(), b"Tc");
    assert!(unhex(b"546").is_none());
    assert!(is_sixel(b"0;1;0q#0;2;0;0;0"));
    assert!(is_sixel(b"q~~"));
    assert!(!is_sixel(b"+q5463"));
  }
}
