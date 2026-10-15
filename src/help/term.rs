use super::layout::Line;
use crate::term::{style::Style, vt::emit};

/// Lines as terminal text; `color` false emits the bare characters.
pub fn render(lines: &[Line], color: bool) -> String {
  let mut out = Vec::new();
  for line in lines {
    let mut current = Style::default();
    for span in line {
      if color {
        emit::sgr(&mut out, current, span.attrs);
        current = span.attrs;
      }
      out.extend_from_slice(span.text.as_bytes());
    }
    if color {
      emit::sgr(&mut out, current, Style::default());
    }
    out.push(b'\n');
  }
  String::from_utf8(out).expect("spans are UTF-8")
}
