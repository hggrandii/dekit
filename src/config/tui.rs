use anyhow::{Result, bail};

use crate::cfg::{CfgCx, CfgNode, CfgObj, FromCfg};

const DEFAULT_SIDEBAR_WIDTH: usize = 30;
const DEFAULT_SIDEBAR_TITLE: &str = "Tasks";

#[derive(Clone)]
pub struct TuiConfig {
  pub sidebar: SidebarConfig,
  pub tips: TipsConfig,
  pub zoom_tip: bool,
  pub theme: ThemeMode,
}

/// The console's colors: `auto` follows the attached terminal's
/// background, light or dark.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemeMode {
  #[default]
  Auto,
  Dark,
  Light,
}

impl ThemeMode {
  pub fn parse(s: &str) -> Option<Self> {
    match s {
      "auto" => Some(ThemeMode::Auto),
      "dark" => Some(ThemeMode::Dark),
      "light" => Some(ThemeMode::Light),
      _ => None,
    }
  }
}

impl FromCfg for ThemeMode {
  fn from_cfg(node: &CfgNode<'_>, _cx: &CfgCx) -> Result<Self> {
    match node.raw().as_str().and_then(ThemeMode::parse) {
      Some(mode) => Ok(mode),
      None => bail!(node.error("expected auto, dark, or light")),
    }
  }
}

#[derive(Clone)]
pub struct SidebarConfig {
  pub title: String,
  pub width: usize,
}

#[derive(Clone)]
pub struct TipsConfig {
  pub show: bool,
}

impl TuiConfig {
  pub(crate) fn builtin() -> Self {
    TuiConfig {
      sidebar: SidebarConfig {
        title: DEFAULT_SIDEBAR_TITLE.to_string(),
        width: DEFAULT_SIDEBAR_WIDTH,
      },
      tips: TipsConfig { show: true },
      zoom_tip: true,
      theme: ThemeMode::Auto,
    }
  }

  pub(crate) fn merge(&mut self, obj: &CfgObj<'_>, cx: &CfgCx) -> Result<()> {
    let tui_obj = match obj.get("tui") {
      Some(node) => node.as_obj()?,
      None => return Ok(()),
    };
    tui_obj.known_keys(&["sidebar", "tips", "zoom_tip", "theme"])?;

    if let Some(pl) = tui_obj.get("sidebar") {
      let pl = pl.as_obj()?;
      pl.known_keys(&["title", "width"])?;
      self.sidebar.title =
        pl.default("title", self.sidebar.title.clone(), cx)?;
      self.sidebar.width = pl.default("width", self.sidebar.width, cx)?;
    }

    if let Some(tips) = tui_obj.get("tips") {
      let tips = tips.as_obj()?;
      tips.known_keys(&["show"])?;
      self.tips.show = tips.default("show", self.tips.show, cx)?;
    }

    self.zoom_tip = tui_obj.default("zoom_tip", self.zoom_tip, cx)?;
    self.theme = tui_obj.default("theme", self.theme, cx)?;

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use crate::cfg::{CfgCx, CfgDoc};

  use super::*;

  fn merged(yaml: &str) -> Result<TuiConfig> {
    let value: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
    let cx = CfgCx::new(std::path::PathBuf::from("."));
    let doc = CfgDoc::from_value(value);
    let mut config = TuiConfig::builtin();
    config.merge(&doc.root().as_obj().unwrap(), &cx)?;
    Ok(config)
  }

  #[test]
  fn theme_defaults_to_auto_and_takes_each_value() {
    assert_eq!(TuiConfig::builtin().theme, ThemeMode::Auto);
    assert_eq!(merged("tui: {}").unwrap().theme, ThemeMode::Auto);
    assert_eq!(merged("tui: {theme: auto}").unwrap().theme, ThemeMode::Auto);
    assert_eq!(merged("tui: {theme: dark}").unwrap().theme, ThemeMode::Dark);
    assert_eq!(
      merged("tui: {theme: light}").unwrap().theme,
      ThemeMode::Light
    );
    let err = merged("tui: {theme: solarized}").err().unwrap();
    assert!(
      format!("{err:#}").contains("auto, dark, or light"),
      "{err:#}"
    );
  }
}
