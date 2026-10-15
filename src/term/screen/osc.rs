//! Operating system commands: title, colors, hyperlinks, clipboard,
//! notifications.

use crate::term::{color::Rgb, style::Link, vt::emit};

use super::{Screen, VtEvent};

impl Screen {
  pub(super) fn osc(&mut self, data: &[u8]) {
    let Ok(s) = std::str::from_utf8(data) else {
      return;
    };
    let (ps, pt) = match s.split_once(';') {
      Some((ps, pt)) => (ps, pt),
      None => (s, ""),
    };
    let Ok(code) = ps.parse::<u32>() else {
      return;
    };
    match code {
      0 | 2 => self.set_title(pt),
      // Icon name.
      1 => (),
      4 => self.osc_palette(pt),
      // 5 (special colors), 6/106 (color mode) are not ours.
      7 => self.cwd = Some(pt.to_string()),
      8 => self.osc_hyperlink(pt),
      9 => self.events.push(VtEvent::Notify {
        title: String::new(),
        body: pt.to_string(),
      }),
      10 | 11 | 12 | 17 | 19 => self.osc_dynamic_color(code, pt),
      // Pointer shape: the client's business, nothing to keep.
      22 => (),
      52 => self.osc_clipboard(pt),
      104 => self.reset_palette(pt),
      110 => self.dynamic_fg = None,
      111 => self.dynamic_bg = None,
      112 => self.dynamic_cursor = None,
      // Shell integration marks.
      133 => (),
      777 => self.osc_notify_777(pt),
      // iTerm2 inline images and other proprietary commands.
      _ => log::debug!("Ignored OSC {code}"),
    }
  }

  fn set_title(&mut self, title: &str) {
    if self.title.as_ref() != title {
      self.title = std::sync::Arc::from(title);
    }
  }

  /// OSC 8 ; params ; uri
  fn osc_hyperlink(&mut self, pt: &str) {
    let (params, uri) = match pt.split_once(';') {
      Some(split) => split,
      None => ("", pt),
    };
    if uri.is_empty() {
      self.set_link(None);
      return;
    }
    let id = params
      .split(':')
      .filter_map(|kv| kv.split_once('='))
      .find(|(k, _)| *k == "id")
      .map(|(_, v)| v.to_string());
    self.set_link(Some(Link {
      uri: uri.to_string(),
      id,
    }));
  }

  /// OSC 4 ; idx ; spec [; idx ; spec]...  with `?` as a query.
  fn osc_palette(&mut self, pt: &str) {
    let mut parts = pt.split(';');
    while let (Some(idx), Some(spec)) = (parts.next(), parts.next()) {
      let Ok(idx) = idx.parse::<u8>() else {
        continue;
      };
      if spec == "?" {
        let rgb = self.palette.idx(idx);
        let mut out = Vec::new();
        emit::osc_color_reply(&mut out, 4, Some(idx), rgb);
        self.events.push(VtEvent::Reply(out));
      } else if let Some(rgb) = parse_color_spec(spec)
        && idx < 16
      {
        self.palette.ansi[idx as usize] = rgb;
      }
    }
  }

  fn reset_palette(&mut self, pt: &str) {
    let default = crate::term::Palette::dark();
    if pt.is_empty() {
      self.palette.ansi = default.ansi;
      return;
    }
    for idx in pt.split(';').filter_map(|i| i.parse::<u8>().ok()) {
      if idx < 16 {
        self.palette.ansi[idx as usize] = default.ansi[idx as usize];
      }
    }
  }

  /// OSC 10/11/12: foreground, background, cursor color; `?` queries.
  fn osc_dynamic_color(&mut self, code: u32, pt: &str) {
    // One command may carry specs for consecutive codes: `OSC 10;fg;bg`.
    for (i, spec) in pt.split(';').enumerate() {
      let code = code + i as u32;
      let current = match code {
        10 => self.dynamic_fg.unwrap_or(self.palette.fg),
        11 => self.dynamic_bg.unwrap_or(self.palette.bg),
        12 => self.dynamic_cursor.unwrap_or(self.palette.fg),
        _ => self.palette.fg,
      };
      if spec == "?" {
        let mut out = Vec::new();
        emit::osc_color_reply(&mut out, code, None, current);
        self.events.push(VtEvent::Reply(out));
      } else if let Some(rgb) = parse_color_spec(spec) {
        match code {
          10 => self.dynamic_fg = Some(rgb),
          11 => self.dynamic_bg = Some(rgb),
          12 => self.dynamic_cursor = Some(rgb),
          _ => (),
        }
      }
    }
  }

  /// OSC 52 ; selection ; base64 — `?` asks for the clipboard, which a
  /// program may not read through dekit.
  fn osc_clipboard(&mut self, pt: &str) {
    let Some((_selection, data)) = pt.split_once(';') else {
      return;
    };
    if data == "?" {
      return;
    }
    use base64::Engine as _;
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data)
    else {
      return;
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    self.events.push(VtEvent::SetClipboard(text));
  }

  /// OSC 777 ; notify ; title ; body
  fn osc_notify_777(&mut self, pt: &str) {
    let mut parts = pt.splitn(3, ';');
    if parts.next() != Some("notify") {
      return;
    }
    let title = parts.next().unwrap_or("").to_string();
    let body = parts.next().unwrap_or("").to_string();
    self.events.push(VtEvent::Notify { title, body });
  }
}

/// `rgb:rr/gg/bb`, `rgb:rrrr/gggg/bbbb`, or `#rrggbb`.
pub fn parse_color_spec(spec: &str) -> Option<Rgb> {
  if let Some(hex) = spec.strip_prefix('#') {
    if hex.len() != 6 {
      return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    return Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
  }
  let rest = spec.strip_prefix("rgb:")?;
  let mut parts = rest.split('/');
  let mut channel = || -> Option<u8> {
    let part = parts.next()?;
    if part.is_empty() || part.len() > 4 {
      return None;
    }
    let v = u32::from_str_radix(part, 16).ok()?;
    // Scale any width to 8 bits.
    let max = (1u32 << (4 * part.len() as u32)) - 1;
    Some((v * 255 / max.max(1)) as u8)
  };
  Some(Rgb(channel()?, channel()?, channel()?))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn color_specs() {
    assert_eq!(parse_color_spec("#ff0080"), Some(Rgb(255, 0, 128)));
    assert_eq!(parse_color_spec("rgb:ff/00/80"), Some(Rgb(255, 0, 128)));
    assert_eq!(parse_color_spec("rgb:ffff/0/8080"), Some(Rgb(255, 0, 128)));
    assert_eq!(parse_color_spec("rgb:f/0/8"), Some(Rgb(255, 0, 136)));
    // One to four digits a channel, as xterm takes.
    assert_eq!(parse_color_spec("rgb:ffffffff/0/0"), None);
    assert_eq!(parse_color_spec("rgb:fffffff/0/0"), None);
    assert_eq!(parse_color_spec("rgb:/0/0"), None);
    assert_eq!(parse_color_spec("#ff00"), None);
  }
}
