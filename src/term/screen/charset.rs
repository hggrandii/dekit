//! Character set designation (ESC ( / ESC ) / ...) and translation.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CharSet {
  #[default]
  Ascii,
  Uk,
  DecLineDrawing,
}

impl CharSet {
  /// The final byte of a designation, or `None` for a set we do not
  /// have (which designates ASCII).
  pub fn from_final(final_: u8) -> Option<CharSet> {
    match final_ {
      b'A' => Some(CharSet::Uk),
      b'B' | b'1' | b'2' => Some(CharSet::Ascii),
      b'0' => Some(CharSet::DecLineDrawing),
      _ => None,
    }
  }

  pub fn name(self) -> &'static str {
    match self {
      CharSet::Ascii => "ascii",
      CharSet::Uk => "uk",
      CharSet::DecLineDrawing => "dec",
    }
  }

  pub fn from_name(name: &str) -> CharSet {
    match name {
      "uk" => CharSet::Uk,
      "dec" => CharSet::DecLineDrawing,
      _ => CharSet::Ascii,
    }
  }

  /// Maps a character through the set. Only `0x20..0x7F` is affected.
  pub fn translate(self, c: char) -> char {
    match self {
      CharSet::Ascii => c,
      CharSet::Uk => {
        if c == '#' {
          '£'
        } else {
          c
        }
      }
      CharSet::DecLineDrawing => dec_line_drawing(c),
    }
  }
}

/// The DEC Special Graphics set, as xterm and kitty draw it.
fn dec_line_drawing(c: char) -> char {
  match c {
    '_' => ' ',
    '`' => '◆',
    'a' => '▒',
    'b' => '␉',
    'c' => '␌',
    'd' => '␍',
    'e' => '␊',
    'f' => '°',
    'g' => '±',
    'h' => '␤',
    'i' => '␋',
    'j' => '┘',
    'k' => '┐',
    'l' => '┌',
    'm' => '└',
    'n' => '┼',
    'o' => '⎺',
    'p' => '⎻',
    'q' => '─',
    'r' => '⎼',
    's' => '⎽',
    't' => '├',
    'u' => '┤',
    'v' => '┴',
    'w' => '┬',
    'x' => '│',
    'y' => '≤',
    'z' => '≥',
    '{' => 'π',
    '|' => '≠',
    '}' => '£',
    '~' => '·',
    c => c,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn translation() {
    assert_eq!(CharSet::DecLineDrawing.translate('q'), '─');
    assert_eq!(CharSet::DecLineDrawing.translate('x'), '│');
    assert_eq!(CharSet::DecLineDrawing.translate('A'), 'A');
    assert_eq!(CharSet::Uk.translate('#'), '£');
    assert_eq!(CharSet::Ascii.translate('q'), 'q');
    assert_eq!(CharSet::from_final(b'0'), Some(CharSet::DecLineDrawing));
    assert_eq!(CharSet::from_final(b'B'), Some(CharSet::Ascii));
    assert_eq!(CharSet::from_final(b'Z'), None);
  }
}
