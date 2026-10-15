//! What a real terminal can show. The attach client measures its
//! terminal once and sends this to the runner, whose painter shapes its
//! output accordingly; everything in between is lossless. It is a wire
//! shape (`caps` on `attach`, PROTOCOL.md "Terminal capabilities") and
//! the painter's input at once, so it grows by defaulted fields only.

use serde::{Deserialize, Serialize};

use crate::term::color::ColorDepth;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct TermCaps {
  pub colors: ColorDepth,
  /// SGR `4:n` underline shapes and `58` underline colors.
  pub underline_styles: bool,
  pub strikethrough: bool,
  pub hyperlinks: bool,
  /// `?2026`: paints can be wrapped so they show at once.
  pub sync_output: bool,
  /// Kitty keyboard protocol flags the terminal accepted.
  pub kitty_keyboard: u8,
  /// The kitty graphics protocol, when the terminal answered a query.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub kitty_graphics: Option<GraphicsCaps>,
  pub sixel: bool,
  /// Focus in/out events arrive.
  pub focus_events: bool,
  /// How the terminal measures the text dekit is unsure about.
  pub widths: WidthPolicy,
}

#[derive(
  Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize,
)]
#[serde(default)]
pub struct GraphicsCaps {
  /// Unicode placeholders (`U=1`) work.
  pub placeholders: bool,
  /// Animation frames and controls work.
  pub animation: bool,
}

/// Where terminals disagree on widths, measured by the client: `true`
/// means the terminal agrees with dekit (TERMINAL.md, `unicode.rs`). The
/// default is unknown, not agreed: a width a client did not send makes
/// the painter address those cells, which is safe on any terminal.
#[derive(
  Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize,
)]
#[serde(default)]
pub struct WidthPolicy {
  /// VS16 makes a narrow emoji two columns.
  pub vs16_wide: bool,
  /// A ZWJ sequence is one two-column cell.
  pub zwj_joins: bool,
  /// A pair of regional indicators is one two-column cell.
  pub flags_wide: bool,
}

impl WidthPolicy {
  /// A terminal that measures everything as dekit does.
  pub fn agreed() -> Self {
    WidthPolicy {
      vs16_wide: true,
      zwj_joins: true,
      flags_wide: true,
    }
  }
}

impl Default for TermCaps {
  /// What an unprobed terminal is assumed to do: true color and the
  /// common SGR attributes, nothing that could garble the screen when
  /// unsupported.
  fn default() -> Self {
    TermCaps {
      colors: ColorDepth::TrueColor,
      underline_styles: false,
      strikethrough: true,
      hyperlinks: true,
      sync_output: false,
      kitty_keyboard: 0,
      kitty_graphics: None,
      sixel: false,
      focus_events: false,
      widths: WidthPolicy::default(),
    }
  }
}

impl TermCaps {
  /// A terminal that does everything: in-process observers and tests.
  pub fn full() -> Self {
    TermCaps {
      colors: ColorDepth::TrueColor,
      underline_styles: true,
      strikethrough: true,
      hyperlinks: true,
      sync_output: true,
      kitty_keyboard: crate::term::modes::KITTY_ALL_FLAGS,
      kitty_graphics: Some(GraphicsCaps {
        placeholders: true,
        animation: true,
      }),
      sixel: false,
      focus_events: true,
      widths: WidthPolicy::agreed(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn wire_round_trip_and_defaults() {
    let caps = TermCaps::full();
    let json = serde_json::to_string(&caps).unwrap();
    assert_eq!(serde_json::from_str::<TermCaps>(&json).unwrap(), caps);
    // Fields an older client does not send take their defaults.
    let partial: TermCaps =
      serde_json::from_str(r#"{"colors":"ansi256"}"#).unwrap();
    assert_eq!(partial.colors, ColorDepth::Ansi256);
    assert!(partial.kitty_graphics.is_none());
    assert_eq!(partial.widths, WidthPolicy::default());
    // A width an older client did not measure is unknown, not agreed.
    let partial: TermCaps =
      serde_json::from_str(r#"{"widths":{"vs16_wide":true}}"#).unwrap();
    assert_eq!(
      partial.widths,
      WidthPolicy {
        vs16_wide: true,
        zwj_joins: false,
        flags_wide: false,
      }
    );
  }
}
