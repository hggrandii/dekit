//! What a real terminal can show. The attach client measures its
//! terminal once and sends this to the runner, whose painter shapes its
//! output accordingly; everything in between is lossless. It is a wire
//! shape (`caps` on `attach`, PROTOCOL.md "Terminal capabilities") and
//! the painter's input at once, so it grows by defaulted fields and new
//! feature names only.

use std::marker::PhantomData;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::term::color::ColorDepth;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct TermCaps {
  pub colors: ColorDepth,
  /// What the terminal draws; a feature not listed is not drawn.
  pub features: Features,
  /// The kitty protocols the terminal speaks.
  pub kitty: KittyCaps,
  /// How the terminal measures the text dekit is unsure about.
  pub widths: WidthPolicy,
  /// Whether the terminal's background is light or dark, when it said
  /// (OSC 11) or the environment did (`COLORFGBG`). The console's
  /// `tui.theme: auto` follows it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub background: Option<Background>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Background {
  Dark,
  Light,
}

pub type Features = FeatureSet<Feature>;

/// A terminal feature the painter can leave out on its own, named as on
/// the wire. PROTOCOL.md lists each with its sequence and terminfo name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Feature {
  /// SGR 3 shows as italics. Under a `screen` terminfo name it shows as
  /// standout, which is reverse video: tmux draws italics so when its
  /// `default-terminal` is `screen` or `screen-*`, and GNU screen does.
  Italic,
  /// SGR 9.
  Strikethrough,
  /// SGR 4:2..4:5: double, curly, dotted and dashed underlines; without
  /// them every underline is single.
  UnderlineStyles,
  /// SGR 58: underlines in their own color; without it the line takes
  /// the text's.
  UnderlineColors,
  /// OSC 8.
  Hyperlinks,
  /// Mode 2026: paints can be wrapped so they show at once.
  Sync,
  /// Mode 1004: focus in/out events arrive.
  Focus,
  Sixel,
}

impl Named for Feature {
  const ALL: &'static [Feature] = &[
    Feature::Italic,
    Feature::Strikethrough,
    Feature::UnderlineStyles,
    Feature::UnderlineColors,
    Feature::Hyperlinks,
    Feature::Sync,
    Feature::Focus,
    Feature::Sixel,
  ];

  fn name(self) -> &'static str {
    match self {
      Feature::Italic => "italic",
      Feature::Strikethrough => "strikethrough",
      Feature::UnderlineStyles => "underline-styles",
      Feature::UnderlineColors => "underline-colors",
      Feature::Hyperlinks => "hyperlinks",
      Feature::Sync => "sync",
      Feature::Focus => "focus",
      Feature::Sixel => "sixel",
    }
  }

  fn index(self) -> u32 {
    self as u32
  }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct KittyCaps {
  /// Keyboard protocol flags the terminal accepted.
  pub keyboard: u8,
  /// The graphics protocol, when the terminal answered a query, with
  /// what of it works beyond placing images.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub graphics: Option<FeatureSet<KittyGraphics>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KittyGraphics {
  /// Unicode placeholders (`U=1`).
  Placeholders,
  /// Animation frames and controls (`a=f`, `a=a`).
  Animation,
}

impl Named for KittyGraphics {
  const ALL: &'static [KittyGraphics] =
    &[KittyGraphics::Placeholders, KittyGraphics::Animation];

  fn name(self) -> &'static str {
    match self {
      KittyGraphics::Placeholders => "placeholders",
      KittyGraphics::Animation => "animation",
    }
  }

  fn index(self) -> u32 {
    self as u32
  }
}

/// A feature that travels by name.
pub trait Named: Copy + Eq + 'static {
  /// Every one, in wire order.
  const ALL: &'static [Self];
  fn name(self) -> &'static str;
  /// Its bit in a set.
  fn index(self) -> u32;

  fn from_name(name: &str) -> Option<Self> {
    Self::ALL
      .iter()
      .copied()
      .find(|feature| feature.name() == name)
  }
}

/// A set of features. On the wire it is a list of names; a name this
/// build does not know is dropped, so a newer client's list still reads.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct FeatureSet<F>(u32, PhantomData<F>);

impl<F: Named> FeatureSet<F> {
  pub fn has(self, feature: F) -> bool {
    self.0 & (1 << feature.index()) != 0
  }

  pub fn with(mut self, feature: F) -> Self {
    self.set(feature, true);
    self
  }

  pub fn set(&mut self, feature: F, on: bool) {
    if on {
      self.0 |= 1 << feature.index();
    } else {
      self.0 &= !(1 << feature.index());
    }
  }

  pub fn iter(self) -> impl Iterator<Item = F> {
    F::ALL
      .iter()
      .copied()
      .filter(move |feature| self.has(*feature))
  }
}

impl<F> Default for FeatureSet<F> {
  fn default() -> Self {
    FeatureSet(0, PhantomData)
  }
}

impl<F: Named> FromIterator<F> for FeatureSet<F> {
  fn from_iter<I: IntoIterator<Item = F>>(iter: I) -> Self {
    let mut features = FeatureSet::default();
    for feature in iter {
      features.set(feature, true);
    }
    features
  }
}

impl<F: Named> std::fmt::Debug for FeatureSet<F> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_set().entries(self.iter().map(F::name)).finish()
  }
}

impl<F: Named> Serialize for FeatureSet<F> {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(self.iter().map(F::name))
  }
}

impl<'de, F: Named> Deserialize<'de> for FeatureSet<F> {
  fn deserialize<D: Deserializer<'de>>(
    deserializer: D,
  ) -> Result<Self, D::Error> {
    let names = Vec::<String>::deserialize(deserializer)?;
    Ok(names.iter().filter_map(|name| F::from_name(name)).collect())
  }
}

/// Where terminals disagree on widths, measured by the client: `true`
/// means the terminal agrees with dekit (TERMINAL.md, `unicode.rs`),
/// `false` that it does not, and none that it was not measured. Only
/// `true` is agreed: the painter addresses the cells of the rest, which
/// is safe on any terminal.
#[derive(
  Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize,
)]
#[serde(default)]
pub struct WidthPolicy {
  /// VS16 makes a narrow emoji two columns.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub vs16_wide: Option<bool>,
  /// A ZWJ sequence is one two-column cell.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub zwj_joins: Option<bool>,
  /// A pair of regional indicators is one two-column cell.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub flags_wide: Option<bool>,
}

impl WidthPolicy {
  /// A terminal that measures everything as dekit does.
  pub fn agreed() -> Self {
    WidthPolicy {
      vs16_wide: Some(true),
      zwj_joins: Some(true),
      flags_wide: Some(true),
    }
  }
}

impl Default for TermCaps {
  /// What a client that says nothing of its terminal is painted as: true
  /// color and the common SGR attributes, nothing that could garble the
  /// screen when unsupported.
  fn default() -> Self {
    TermCaps {
      colors: ColorDepth::TrueColor,
      features: [Feature::Italic, Feature::Strikethrough, Feature::Hyperlinks]
        .into_iter()
        .collect(),
      kitty: KittyCaps::default(),
      widths: WidthPolicy::default(),
      background: None,
    }
  }
}

impl TermCaps {
  /// A terminal that does everything: in-process observers and tests.
  pub fn full() -> Self {
    TermCaps {
      colors: ColorDepth::TrueColor,
      features: Feature::ALL
        .iter()
        .copied()
        .filter(|feature| *feature != Feature::Sixel)
        .collect(),
      kitty: KittyCaps {
        keyboard: crate::term::modes::KITTY_ALL_FLAGS,
        graphics: Some(KittyGraphics::ALL.iter().copied().collect()),
      },
      widths: WidthPolicy::agreed(),
      background: None,
    }
  }

  pub fn has(&self, feature: Feature) -> bool {
    self.features.has(feature)
  }

  pub fn without(mut self, feature: Feature) -> Self {
    self.features.set(feature, false);
    self
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
    assert_eq!(partial.features, TermCaps::default().features);
    assert_eq!(partial.widths, WidthPolicy::default());
    assert_eq!(partial.background, None);
    let light = TermCaps {
      background: Some(Background::Light),
      ..TermCaps::default()
    };
    let json = serde_json::to_string(&light).unwrap();
    assert!(json.contains(r#""background":"light""#), "{json}");
    assert_eq!(serde_json::from_str::<TermCaps>(&json).unwrap(), light);
    // A width a client did not measure is unknown, not agreed, and one
    // it measured as different says so.
    let partial: TermCaps = serde_json::from_str(
      r#"{"widths":{"vs16_wide":true,"zwj_joins":false}}"#,
    )
    .unwrap();
    assert_eq!(
      partial.widths,
      WidthPolicy {
        vs16_wide: Some(true),
        zwj_joins: Some(false),
        flags_wide: None,
      }
    );
    assert_eq!(
      serde_json::to_string(&partial.widths).unwrap(),
      r#"{"vs16_wide":true,"zwj_joins":false}"#
    );
  }

  #[test]
  fn features_are_names_and_unknown_ones_are_dropped() {
    let features: Features = [Feature::UnderlineColors, Feature::Italic]
      .into_iter()
      .collect();
    assert_eq!(
      serde_json::to_string(&features).unwrap(),
      r#"["italic","underline-colors"]"#
    );
    // A listed feature is on, one not listed is off.
    let caps: TermCaps =
      serde_json::from_str(r#"{"features":["sync","from-the-future"]}"#)
        .unwrap();
    assert_eq!(caps.features, [Feature::Sync].into_iter().collect());
    for feature in Feature::ALL.iter().copied() {
      assert_eq!(Feature::from_name(feature.name()), Some(feature));
    }
    for feature in KittyGraphics::ALL.iter().copied() {
      assert_eq!(KittyGraphics::from_name(feature.name()), Some(feature));
    }
  }

  #[test]
  fn kitty_graphics_are_absent_or_a_list() {
    let kitty = |json| serde_json::from_str::<TermCaps>(json).unwrap().kitty;
    assert_eq!(kitty(r#"{"kitty":{"keyboard":1}}"#).graphics, None);
    let graphics = kitty(r#"{"kitty":{"graphics":[]}}"#).graphics.unwrap();
    assert!(!graphics.has(KittyGraphics::Animation));
    assert_eq!(
      serde_json::to_string(&TermCaps::full().kitty).unwrap(),
      r#"{"keyboard":31,"graphics":["placeholders","animation"]}"#
    );
  }
}
