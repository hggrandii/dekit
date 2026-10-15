use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ProcessSpec {
  pub prog: String,
  pub args: Vec<String>,
  // pub pty: bool,
  pub cwd: Option<String>,
  pub env: BTreeMap<String, Option<String>>,
}

impl ProcessSpec {
  pub fn from_argv(mut argv: Vec<String>) -> ProcessSpec {
    let prog = if !argv.is_empty() {
      argv.remove(0)
    } else {
      String::new()
    };
    ProcessSpec {
      prog,
      args: argv,
      // pty: true,
      cwd: None,
      env: Default::default(),
    }
  }

  pub fn cwd<T: Into<String>>(&mut self, cwd: T) {
    self.cwd = Some(cwd.into());
  }
  pub fn get_cwd(&self) -> &Option<String> {
    &self.cwd
  }

  pub fn env<K: Into<String>, V: Into<String>>(&mut self, k: K, v: V) {
    self.env.insert(k.into(), Some(v.into()));
  }

  pub fn env_remove<K: Into<String>>(&mut self, k: K) {
    self.env.insert(k.into(), None);
  }

  /// The environment a child gets, in order: dekit's terminal identity
  /// over the inherited environment, then the spec's own settings over
  /// that, so a configured `TERM` wins.
  pub fn child_env(&self) -> Vec<(String, Option<String>)> {
    let mut env: Vec<(String, Option<String>)> = terminal_env();
    env.extend(self.env.iter().map(|(k, v)| (k.clone(), v.clone())));
    env
  }
}

/// What dekit's screens are: `TERM` and friends set, the variables the
/// terminal that started the runner identifies itself with unset, so
/// programs see dekit's emulation and not that terminal.
pub fn terminal_env() -> Vec<(String, Option<String>)> {
  let mut env: Vec<(String, Option<String>)> = vec![
    ("TERM".into(), Some("xterm-256color".into())),
    ("COLORTERM".into(), Some("truecolor".into())),
    ("TERM_PROGRAM".into(), Some("dekit".into())),
    (
      "TERM_PROGRAM_VERSION".into(),
      Some(env!("CARGO_PKG_VERSION").into()),
    ),
  ];
  for var in SCRUBBED_ENV {
    env.push(((*var).to_string(), None));
  }
  for (key, _) in std::env::vars_os() {
    let key = key.to_string_lossy();
    if SCRUBBED_ENV_PREFIXES.iter().any(|p| key.starts_with(p)) {
      env.push((key.into_owned(), None));
    }
  }
  env
}

/// Variables with which terminals and multiplexers announce themselves.
const SCRUBBED_ENV: &[&str] = &[
  "TMUX",
  "TMUX_PANE",
  "STY",
  "WINDOW",
  "TERM_SESSION_ID",
  "ITERM_SESSION_ID",
  "ITERM_PROFILE",
  "VTE_VERSION",
  "WT_SESSION",
  "WT_PROFILE_ID",
  "TERMINAL_EMULATOR",
  "TERMINFO",
  "ZELLIJ",
  "ZELLIJ_SESSION_NAME",
  "ZELLIJ_PANE_ID",
  "GHOSTTY_RESOURCES_DIR",
  "GHOSTTY_BIN_DIR",
  "GHOSTTY_SHELL_FEATURES",
  "WEZTERM_PANE",
  "WEZTERM_UNIX_SOCKET",
  "WEZTERM_EXECUTABLE",
  "WEZTERM_EXECUTABLE_DIR",
  "WEZTERM_CONFIG_FILE",
  "WEZTERM_CONFIG_DIR",
  "ALACRITTY_SOCKET",
  "ALACRITTY_LOG",
  "ALACRITTY_WINDOW_ID",
  "KONSOLE_VERSION",
  "KONSOLE_DBUS_SESSION",
  "KONSOLE_DBUS_WINDOW",
  "KONSOLE_DBUS_SERVICE",
  "GNOME_TERMINAL_SCREEN",
  "GNOME_TERMINAL_SERVICE",
  "WARP_IS_LOCAL_SHELL_SESSION",
  "WARP_USE_SSH_WRAPPER",
  "WARP_HONOR_PS1",
];

const SCRUBBED_ENV_PREFIXES: &[&str] = &["KITTY_", "TABBY_"];
