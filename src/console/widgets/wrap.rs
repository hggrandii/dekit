//! Text broken into lines that fit.

use unicode_width::UnicodeWidthStr;

/// Breaks text into lines at most `width` wide, between words where it
/// can.
pub fn wrap(text: &str, width: u16) -> Vec<String> {
  let width = usize::from(width.max(1));
  let mut lines = Vec::new();
  for paragraph in text.lines() {
    let mut line = String::new();
    for word in paragraph.split_whitespace() {
      let mut word = word.to_string();
      if !line.is_empty() && line.width() + 1 + word.width() > width {
        lines.push(std::mem::take(&mut line));
      }
      while word.width() > width {
        // At least a character a line, if one is wider than the line.
        let at = word
          .char_indices()
          .scan(0, |w, (i, c)| {
            *w += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            Some((i, *w))
          })
          .skip(1)
          .find(|(_, w)| *w > width)
          .map_or(word.len(), |(i, _)| i);
        lines.push(word[..at].to_string());
        word = word[at..].to_string();
      }
      if !line.is_empty() {
        line.push(' ');
      }
      line.push_str(&word);
    }
    lines.push(line);
  }
  lines
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn wrap_breaks_a_word_longer_than_the_line() {
    assert_eq!(wrap("abcdefgh ij", 3), ["abc", "def", "gh", "ij"]);
    assert_eq!(wrap("界界a", 1), ["界", "界", "a"]);
  }
}
