use super::{frame::Frame, style::Style, vt::emit};

/// Render the frame as ANSI-styled text, one line per row, trailing
/// blanks trimmed; a blank with a visible style stays.
pub fn render_frame_ansi(frame: &Frame) -> String {
  let size = frame.size;
  let mut out: Vec<u8> = Vec::new();
  let mut brush = Style::DEFAULT;
  let mut buf = [0u8; 4];

  for row in 0..size.height {
    if row > 0 {
      out.extend_from_slice(b"\r\n");
    }
    let end = (0..size.width)
      .rev()
      .find(|&col| {
        frame.cell(row, col).is_some_and(|cell| {
          cell.has_contents() || frame.style(cell).is_visible_blank()
        })
      })
      .map_or(0, |col| col + 1);

    for col in 0..end {
      let Some(cell) = frame.cell(row, col) else {
        continue;
      };
      if cell.is_continuation() {
        continue;
      }
      let style = frame.style(cell).with_link(Default::default());
      emit::sgr(&mut out, brush, style);
      brush = style;
      let text = frame.text(cell, &mut buf);
      if text.is_empty() {
        out.push(b' ');
      } else {
        out.extend_from_slice(text.as_bytes());
      }
    }
  }

  if brush != Style::DEFAULT {
    out.extend_from_slice(emit::SGR_RESET.as_bytes());
  }

  String::from_utf8(out).expect("emitted ANSI is valid utf-8")
}
