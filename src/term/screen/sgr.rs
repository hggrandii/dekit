//! SGR: select graphic rendition.

use crate::term::{
  color::Color,
  style::{Style, Underline},
  vt::Params,
};

use super::Screen;

impl Screen {
  pub(super) fn sgr(&mut self, p: &Params) {
    if p.is_empty() {
      self.reset_style();
      return;
    }
    let mut i = 0;
    while i < p.len() {
      if p.is_sub(i) {
        // A subparameter of an attribute that does not use it.
        i += 1;
        continue;
      }
      let mut style = self.style;
      match p.get(i, 0) {
        0 => {
          self.reset_style();
          i += 1;
          continue;
        }
        1 => style = style.set_bold(true),
        2 => style = style.set_dim(true),
        3 => style = style.set_italic(true),
        4 => {
          // `4:n` picks the shape; plain `4` is single.
          let shape = if p.is_sub(i + 1) {
            Underline::from_u8(p.get(i + 1, 1).min(255) as u8)
          } else {
            Underline::Single
          };
          style = style.set_underline(shape);
        }
        5 | 6 => style = style.set_blink(true),
        7 => style = style.set_inverse(true),
        8 => style = style.set_hidden(true),
        9 => style = style.set_strike(true),
        21 => style = style.set_underline(Underline::Double),
        22 => style = style.set_bold(false).set_dim(false),
        23 => style = style.set_italic(false),
        24 => style = style.set_underline(Underline::None),
        25 => style = style.set_blink(false),
        27 => style = style.set_inverse(false),
        28 => style = style.set_hidden(false),
        29 => style = style.set_strike(false),
        n @ 30..=37 => style.fg = Color::Idx(n as u8 - 30),
        38 => {
          let (color, next) = sgr_color(p, i);
          style.fg = color;
          self.style = style;
          i = next;
          continue;
        }
        39 => style.fg = Color::Default,
        n @ 40..=47 => style.bg = Color::Idx(n as u8 - 40),
        48 => {
          let (color, next) = sgr_color(p, i);
          style.bg = color;
          self.style = style;
          i = next;
          continue;
        }
        49 => style.bg = Color::Default,
        53 => style = style.set_overline(true),
        55 => style = style.set_overline(false),
        58 => {
          let (color, next) = sgr_color(p, i);
          style.ul = color;
          self.style = style;
          i = next;
          continue;
        }
        59 => style.ul = Color::Default,
        n @ 90..=97 => style.fg = Color::Idx(n as u8 - 90 + 8),
        n @ 100..=107 => style.bg = Color::Idx(n as u8 - 100 + 8),
        n => log::debug!("Ignored SGR {n}"),
      }
      self.style = style;
      i += 1;
    }
  }

  fn reset_style(&mut self) {
    // The link is not an SGR attribute: OSC 8 ends it.
    let link = self.style.link;
    self.style = Style::DEFAULT.with_link(link);
  }
}

/// Extended color argument of SGR 38/48/58; accepts both the semicolon
/// and the colon forms. Returns the color and the index after the
/// consumed params.
fn sgr_color(p: &Params, i: usize) -> (Color, usize) {
  // In the colon form all parts are subparams; in the semicolon form
  // they are plain params. Count how many follow either way.
  match p.get(i + 1, 2) {
    2 => {
      // The ITU colon form may carry an empty color space id before the
      // components: `38:2::r:g:b`.
      let i = if p.is_sub(i + 2) && p.get_opt(i + 2).is_none() {
        i + 1
      } else {
        i
      };
      (
        Color::Rgb(
          p.get(i + 2, 0).min(255) as u8,
          p.get(i + 3, 0).min(255) as u8,
          p.get(i + 4, 0).min(255) as u8,
        ),
        i + 5,
      )
    }
    5 => (Color::Idx(p.get(i + 2, 0).min(255) as u8), i + 3),
    _ => (Color::Default, i + 2),
  }
}
