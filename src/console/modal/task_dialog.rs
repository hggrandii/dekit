//! The dialog that adds, duplicates, and edits a task: every field of a
//! task entry, the common ones in sight and the rest under "More".

use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Value};
use unicode_width::UnicodeWidthStr;

use crate::command::Command;
use crate::console::{
  keymap::Keymap,
  theme::Theme,
  widgets::{
    border::draw_hints,
    form::{Completion, Field, FieldKind, Form, FormEvent},
  },
};
use crate::kernel::kernel_message::Place;
use crate::kernel::task::{RestartMode, TaskKind};
use crate::kernel::task_path::{
  TaskPath, is_at_or_under, overlaps, path_name, unique,
};
use crate::project::relative_path;
use crate::spec::{Source, TaskSpec, join_argv, show_value};
use crate::target::Target;
use crate::term::{
  CursorStyle, Grid,
  grid::Rect,
  key::Key,
  mouse::{MouseButton, MouseEvent, MouseEventKind},
  style::Style,
};

use super::modal::{IssueError, Modal, ModalResult};

pub enum TaskDialogMode {
  /// A new task, listed after `after`: a copy of it when `copy`.
  Add { after: Option<TaskPath>, copy: bool },
  Edit {
    path: String,
    running: bool,
    origin: Option<Source>,
  },
}

impl TaskDialogMode {
  /// The path of the task being edited, which is not taken by itself.
  fn own_path(&self) -> Option<&str> {
    match self {
      TaskDialogMode::Edit { path, .. } => Some(path),
      TaskDialogMode::Add { .. } => None,
    }
  }
}

/// What the dialog needs to know about the runner.
pub struct DialogContext {
  /// Where relative directories are from.
  pub root: PathBuf,
  /// Every task's path, for deps and for paths already taken.
  pub paths: Vec<String>,
  /// Commands run through the system shell (mprocs.yaml).
  pub system_shell: bool,
}

pub struct TaskDialog {
  mode: TaskDialogMode,
  form: Form,
  /// The definition being edited; keys the form doesn't show stay.
  spec: TaskSpec,
  /// What each field held when the dialog opened.
  initial: Vec<(String, String)>,
  cx: DialogContext,
  error: Option<String>,
  /// Waiting for the runner to take it.
  busy: bool,
  /// The definition an edit sent, first of its commands: what the task
  /// has if the move after it fails.
  sent: Option<TaskSpec>,
  buttons: [Rect; 2],
}

/// The ready checks: config key, how the dialog says it, and what the
/// value is.
const READY: [(&str, &str, &str); 6] = [
  ("none", "it starts", ""),
  ("log", "output has", "text in the output"),
  ("tcp", "port opens", "port, or host:port"),
  ("http", "URL answers", "http://localhost:3000/health"),
  ("cmd", "command passes", "a command that exits with 0"),
  ("file", "file exists", "a path, from the task's directory"),
];
/// Signals offered to stop with; one the task has besides goes in too.
const SIGNALS: [&str; 5] =
  ["SIGTERM", "SIGINT", "SIGHUP", "SIGQUIT", "SIGKILL"];
const STOP_KEYS: &str = "keys";
const ENV_PLACEHOLDER: &str = "NAME=value, or NAME to unset";
const STOP_CMD: &str = "command";

impl TaskDialog {
  pub fn new(
    mode: TaskDialogMode,
    spec: TaskSpec,
    cx: DialogContext,
    path: String,
  ) -> Self {
    let script = spec.get("script").and_then(Value::as_str);
    let mut fields = Vec::new();
    let command = match script {
      Some(script) => Field::text("script", "Script", script),
      None => Field::text(
        "cmd",
        "Command",
        &spec.argv().map(|argv| join_argv(&argv)).unwrap_or_default(),
      )
      .placeholder("npm run dev"),
    };
    let label = spec.label().unwrap_or_default();
    fields.extend([
      command,
      Field::text("title", "Title", label),
      Field::text("path", "Task path", &path).placeholder("group/name"),
    ]);
    let text = |key: &str| {
      spec
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
    };
    let cwd = text("cwd");
    let shown_cwd = if cwd.is_empty() {
      cwd
    } else {
      relative_path(Path::new(&cwd), &cx.root)
    };
    match &mode {
      TaskDialogMode::Edit { running: true, .. } => fields.push(
        Field::toggle("start", "Restart now", true)
          .hint("or the change applies at its next start"),
      ),
      TaskDialogMode::Edit { running: false, .. } => (),
      TaskDialogMode::Add { .. } => {
        fields.push(Field::toggle("start", "Start now", true));
      }
    }

    fields.push(Field::section("more", "More", false));
    fields.push(
      Field::text("cwd", "Directory", &shown_cwd)
        .placeholder("the project root")
        .complete(Completion::Dirs(cx.root.clone())),
    );
    let kind = usize::from(spec.kind() == TaskKind::Job);
    fields.push(Field::choice("type", "Type", &["service", "job"], kind));
    let restart = spec
      .get("autorestart")
      .and_then(Value::as_str)
      .and_then(|name| RestartMode::ALL.iter().position(|r| r.name() == name))
      .unwrap_or(0);
    let names = RestartMode::ALL.map(RestartMode::name);
    fields.push(Field::choice("restart", "Restart", &names, restart));
    let autostart = spec.get("autostart") == Some(&Value::Bool(true));
    fields.push(
      Field::toggle("autostart", "Autostart", autostart)
        .hint("started by `up`"),
    );
    let own = mode.own_path();
    let others: Vec<String> = cx
      .paths
      .iter()
      .filter(|p| Some(p.as_str()) != own)
      .cloned()
      .collect();
    fields.push(
      Field::text("deps", "Deps", &spec.deps().join(" "))
        .placeholder("tasks it needs")
        .complete(Completion::Words(others)),
    );
    let env: Vec<String> = spec
      .get("env")
      .and_then(Value::as_mapping)
      .into_iter()
      .flatten()
      .filter_map(|(k, v)| {
        let k = k.as_str()?;
        Some(match v {
          Value::Null => k.to_string(),
          v => {
            format!("{k}={}", show(v))
          }
        })
      })
      .collect();
    for (i, var) in env
      .iter()
      .chain(std::iter::once(&String::new()))
      .enumerate()
    {
      let label = if i == 0 { "Env" } else { "" };
      fields.push(
        Field::text(&format!("env.{i}"), label, var)
          .placeholder(ENV_PLACEHOLDER),
      );
    }

    let ready = spec.get("ready").and_then(Value::as_mapping);
    let (check, check_value) = ready
      .and_then(|ready| {
        READY[1..].iter().enumerate().find_map(|(i, (key, ..))| {
          ready.get(*key).map(|value| (i + 1, show(value)))
        })
      })
      .unwrap_or((0, String::new()));
    let ready_value = |key| ready.and_then(|r| r.get(key)).map(show);
    fields.push(Field::select(
      "ready",
      "Ready when",
      READY.iter().map(|(_, said, _)| said.to_string()).collect(),
      check,
    ));
    fields.push(Field::text("ready.value", "", &check_value).inline());
    fields.push(
      Field::text(
        "ready.interval",
        "Check every",
        &ready_value("interval").unwrap_or_default(),
      )
      .placeholder("1s"),
    );
    fields.push(
      Field::text(
        "ready.timeout",
        "Ready within",
        &ready_value("timeout").unwrap_or_default(),
      )
      .placeholder("no limit, or 30s"),
    );

    let (stop_with, stop_value, stop_timeout) = stop_of(spec.get("stop"));
    let mut stop_options: Vec<String> =
      SIGNALS.iter().map(|s| s.to_string()).collect();
    if !stop_options.contains(&stop_with)
      && ![STOP_KEYS, STOP_CMD].contains(&stop_with.as_str())
    {
      stop_options.push(stop_with.clone());
    }
    stop_options.extend([STOP_KEYS.to_string(), STOP_CMD.to_string()]);
    let stop_index = stop_options
      .iter()
      .position(|option| *option == stop_with)
      .unwrap_or(0);
    fields.push(Field::select("stop", "Stop with", stop_options, stop_index));
    fields.push(Field::text("stop.value", "", &stop_value).inline());
    fields.push(
      Field::text("stop.timeout", "Kill after", &stop_timeout)
        .placeholder("10s"),
    );
    let tags = spec.strings("tags").join(" ");
    fields.push(Field::text("tags", "Tags", &tags).placeholder("words"));

    let form = Form::new(fields);
    let initial = form
      .fields
      .iter()
      .map(|field| (field.id.clone(), field_value(field)))
      .collect();
    let mut dialog = TaskDialog {
      mode,
      form,
      spec,
      initial,
      cx,
      error: None,
      busy: false,
      sent: None,
      buttons: [Rect::default(); 2],
    };
    dialog.update();
    dialog
  }

  fn title(&self) -> &'static str {
    match self.mode {
      TaskDialogMode::Add { copy: false, .. } => " New task ",
      TaskDialogMode::Add { copy: true, .. } => " Duplicate task ",
      TaskDialogMode::Edit { .. } => " Edit task ",
    }
  }

  fn action(&self) -> &'static str {
    match self.mode {
      TaskDialogMode::Add { .. } => "Add",
      TaskDialogMode::Edit { .. } => "Save",
    }
  }

  fn note(&self) -> Option<String> {
    match &self.mode {
      TaskDialogMode::Edit {
        origin: Some(origin),
        ..
      } => Some(format!(
        "From {}; changes stay here until written back.",
        origin.file
      )),
      TaskDialogMode::Edit { origin: None, .. } => {
        Some("Added here; not in the config.".to_string())
      }
      TaskDialogMode::Add { .. } => None,
    }
  }

  fn value(&self, id: &str) -> String {
    self
      .form
      .field(id)
      .map(|f| f.value().trim().to_string())
      .unwrap_or_default()
  }

  /// A field's words, between blanks or commas.
  fn words(&self, id: &str) -> Vec<String> {
    self
      .value(id)
      .split(|c: char| c.is_whitespace() || c == ',')
      .filter(|word| !word.is_empty())
      .map(String::from)
      .collect()
  }

  fn changed(&self, id: &str) -> bool {
    let now = self.form.field(id).map(field_value).unwrap_or_default();
    let then = self
      .initial
      .iter()
      .find(|(field, _)| field == id)
      .map(|(_, value)| value.clone())
      .unwrap_or_default();
    now != then
  }

  fn env_changed(&self) -> bool {
    let now: Vec<String> = self.env_rows();
    let then: Vec<String> = self
      .initial
      .iter()
      .filter(|(id, value)| id.starts_with("env.") && !value.trim().is_empty())
      .map(|(_, value)| value.trim().to_string())
      .collect();
    now != then
  }

  fn env_rows(&self) -> Vec<String> {
    self
      .form
      .fields
      .iter()
      .filter(|f| f.id.starts_with("env."))
      .map(|f| f.value().trim().to_string())
      .filter(|v| !v.is_empty())
      .collect()
  }

  /// The path the task gets while the path typed is a group, as `web/`,
  /// or nothing: its title, or its program, made a name in that group
  /// and unique.
  fn suggested_path(&self) -> String {
    let group = self.value("path");
    if !group.is_empty() && !group.ends_with('/') {
      return String::new();
    }
    let name = self.value("title");
    let base = if name.is_empty() {
      let cmd = self.value("cmd");
      cmd
        .split_whitespace()
        .next()
        .map(|prog| prog.rsplit('/').next().unwrap_or(prog).to_string())
        .unwrap_or_default()
    } else {
      name
    };
    if base.is_empty() {
      return String::new();
    }
    let path = format!("{group}{}", path_name(&base));
    unique(&path, |candidate| self.taken(candidate))
  }

  /// Fits the form to what is chosen: the path a new task would get,
  /// the fields each ready check and way to stop take, what a job takes,
  /// an env row to type into, and what is set under "More".
  fn update(&mut self) {
    let suggested = self.suggested_path();
    if let Some(field) = self.form.field_mut("path") {
      field.set_suggestion((!suggested.is_empty()).then(|| suggested.clone()));
    }
    // Without a title, the task list shows the last part of the path.
    let path = match self.value("path") {
      path if path.is_empty() || path.ends_with('/') => suggested,
      path => path,
    };
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let shown = if name.is_empty() {
      "the task's name"
    } else {
      name
    };
    if let Some(field) = self.form.field_mut("title") {
      field.set_placeholder(shown);
    }

    let kind = self.kind();
    let restart: Vec<&str> = RestartMode::ALL
      .into_iter()
      .filter(|restart| kind.takes_restart(*restart))
      .map(RestartMode::name)
      .collect();
    if let Some(field) = self.form.field_mut("restart") {
      field.set_options(&restart, 1);
    }
    let check = self.ready_check();
    let (key, _, placeholder) = READY[check];
    let polls = !matches!(key, "none" | "log");
    let no_ready = !kind.takes_ready();
    for (id, hidden) in [
      ("ready", no_ready),
      ("ready.value", no_ready || key == "none"),
      ("ready.interval", no_ready || !polls),
      ("ready.timeout", no_ready || key == "none"),
    ] {
      if let Some(field) = self.form.field_mut(id) {
        field.hidden = hidden;
      }
    }
    if let Some(field) = self.form.field_mut("ready.value") {
      field.set_placeholder(placeholder);
    }
    let stop_with = self.value("stop");
    let placeholder = match stop_with.as_str() {
      STOP_KEYS => Some("<C-c>, as keys are written in the keymap"),
      STOP_CMD => Some("docker compose stop"),
      _ => None,
    };
    if let Some(field) = self.form.field_mut("stop.value") {
      field.hidden = placeholder.is_none();
      field.set_placeholder(placeholder.unwrap_or_default());
    }

    // An env row typed into gets an empty one after it.
    let rows: Vec<(String, bool)> = self
      .form
      .fields
      .iter()
      .filter(|f| f.id.starts_with("env."))
      .map(|f| (f.id.clone(), f.value().is_empty()))
      .collect();
    if let Some((last, false)) = rows.last() {
      let n = rows.len();
      self.form.insert_after(
        last,
        Field::text(&format!("env.{n}"), "", "").placeholder(ENV_PLACEHOLDER),
      );
    }

    let summary = self.more_summary();
    if let Some(field) = self.form.field_mut("more") {
      field.hint = (!summary.is_empty()).then_some(summary);
    }
  }

  /// The settings under "More" that are not left as they come, for the
  /// section's row while it is closed.
  fn more_summary(&self) -> String {
    let kind = self.kind();
    let mut set = Vec::new();
    if !self.value("cwd").is_empty() {
      set.push("directory".to_string());
    }
    if kind == TaskKind::Job {
      set.push("job".to_string());
    }
    let restart = self.value("restart");
    if restart != "never" {
      set.push(format!("restart {restart}"));
    }
    if self.form.field("autostart").is_some_and(Field::on) {
      set.push("autostart".to_string());
    }
    if !self.value("deps").is_empty() {
      set.push("deps".to_string());
    }
    if !self.env_rows().is_empty() {
      set.push("env".to_string());
    }
    if kind.takes_ready() && self.ready_check() != 0 {
      set.push("ready".to_string());
    }
    if self.form.field("stop").is_some_and(|f| f.selected() != 0)
      || !self.value("stop.timeout").is_empty()
    {
      set.push("stop".to_string());
    }
    if !self.value("tags").is_empty() {
      set.push("tags".to_string());
    }
    set.join(", ")
  }

  fn ready_check(&self) -> usize {
    self.form.field("ready").map_or(0, Field::selected)
  }

  fn kind(&self) -> TaskKind {
    match self.value("type").as_str() {
      "job" => TaskKind::Job,
      _ => TaskKind::Service,
    }
  }

  /// Whether a path, or one above or under it, has a task already. The
  /// task edited can't move under where it is either, but it can up to a
  /// group it alone is in.
  fn taken(&self, path: &str) -> bool {
    let own = self.mode.own_path();
    self.cx.paths.iter().any(|other| match own {
      Some(own) if other == own => path != own && is_at_or_under(path, own),
      _ => overlaps(other, path),
    })
  }

  /// The task the form describes, or the field that is wrong and why.
  fn build(
    &mut self,
  ) -> Result<(String, TaskSpec), (Option<&'static str>, String)> {
    let mut spec = self.spec.clone();
    let field = |id: &'static str, err: String| (Some(id), err);

    if self.changed("title")
      || !matches!(self.mode, TaskDialogMode::Edit { .. })
    {
      let title = self.value("title");
      spec.set_label((!title.is_empty()).then_some(title.as_str()));
    }
    if self.form.field("script").is_some() {
      if self.changed("script") {
        let script = self.value("script");
        if script.is_empty() {
          return Err(field("script", "a task needs a script".into()));
        }
        spec.set("script", Some(Value::String(script)));
      }
    } else if self.changed("cmd") || spec.get("cmd").is_none() {
      let line = self.value("cmd");
      if line.is_empty() {
        return Err(field("cmd", "a task needs a command".into()));
      }
      let argv = crate::parse_shell::typed_argv(&line, self.cx.system_shell)
        .map_err(|err| field("cmd", err.to_string()))?;
      spec.set_strings("cmd", argv);
    }
    let mut path = self.value("path");
    if path.is_empty() || path.ends_with('/') {
      path = self.suggested_path();
    }
    let path = path.trim_matches('/').to_string();
    if path.is_empty() {
      return Err(field("path", "a task needs a path".to_string()));
    }
    TaskPath::new(&path).map_err(|err| field("path", err.to_string()))?;
    if path.starts_with('@') {
      return Err(field(
        "path",
        "a task here can't be in another space".into(),
      ));
    }
    if self.taken(&path) {
      return Err(field(
        "path",
        "taken, or a task is above or under it".into(),
      ));
    }

    if self.changed("cwd") {
      let cwd = self.value("cwd");
      let cwd = match cwd.as_str() {
        "" => None,
        "." => Some(self.cx.root.to_string_lossy().into_owned()),
        cwd => Some(cwd.to_string()),
      };
      spec.set("cwd", cwd.map(Value::String));
    }
    if self.changed("type") {
      let job = self.kind() == TaskKind::Job;
      spec.set("type", job.then(|| Value::from("job")));
    }
    if self.changed("restart") {
      let restart = self.value("restart");
      spec.set(
        "autorestart",
        (restart != "never" || spec.get("autorestart").is_some())
          .then(|| Value::String(restart)),
      );
    }
    if self.changed("autostart") {
      // Said either way: `defaults` may say otherwise.
      let on = self.form.field("autostart").is_some_and(Field::on);
      spec.set("autostart", Some(Value::Bool(on)));
    }
    if self.env_changed() {
      let mut env = Mapping::new();
      for row in self.env_rows() {
        let (name, value) = match row.split_once('=') {
          Some((name, value)) => (name.trim(), Value::from(value)),
          None => (row.as_str(), Value::Null),
        };
        if name.is_empty() || name.contains(char::is_whitespace) {
          return Err((None, format!("env: '{row}' is not NAME=value")));
        }
        env.insert(Value::from(name), value);
      }
      spec.set("env", (!env.is_empty()).then_some(Value::Mapping(env)));
    }
    if self.changed("deps") {
      spec.set_deps(self.words("deps"));
    }
    if self.changed("tags") {
      spec.set_strings("tags", self.words("tags"));
    }
    if !self.kind().takes_ready() {
      if spec.get("ready").is_some() {
        spec.set("ready", None);
      }
    } else if ["ready", "ready.value", "ready.interval", "ready.timeout"]
      .iter()
      .any(|id| self.changed(id))
    {
      let (check, said, _) = READY[self.ready_check()];
      let value = self.value("ready.value");
      let ready = match check {
        "none" => None,
        check => {
          if value.is_empty() {
            return Err(field("ready.value", format!("ready when {said}?")));
          }
          let mut ready = spec
            .get("ready")
            .and_then(Value::as_mapping)
            .cloned()
            .unwrap_or_default();
          for (key, ..) in &READY[1..] {
            ready.remove(*key);
          }
          let value = match (check, value.parse::<u16>()) {
            ("tcp", Ok(port)) => Value::from(port),
            ("cmd", _) => {
              argv_value(&value).map_err(|err| field("ready.value", err))?
            }
            _ => Value::String(value),
          };
          ready.insert(Value::from(check), value);
          for (key, id) in
            [("interval", "ready.interval"), ("timeout", "ready.timeout")]
          {
            match self.value(id).as_str() {
              _ if key == "interval" && check == "log" => ready.remove(key),
              "" => ready.remove(key),
              value => ready.insert(key.into(), Value::from(value)),
            };
          }
          Some(Value::Mapping(ready))
        }
      };
      spec.set("ready", ready);
    }
    if ["stop", "stop.value", "stop.timeout"]
      .iter()
      .any(|id| self.changed(id))
    {
      let mut stop = match spec.get("stop") {
        Some(Value::Mapping(stop)) => stop.clone(),
        Some(Value::String(signal)) => {
          Mapping::from_iter([("signal".into(), Value::from(signal.as_str()))])
        }
        _ => Mapping::new(),
      };
      if self.changed("stop") || self.changed("stop.value") {
        for key in ["signal", "keys", "cmd"] {
          stop.remove(key);
        }
        let value = self.value("stop.value");
        match self.value("stop").as_str() {
          STOP_KEYS => {
            let keys: Vec<Value> =
              value.split_whitespace().map(Value::from).collect();
            if keys.is_empty() {
              return Err(field("stop.value", "which keys to type?".into()));
            }
            stop.remove("group");
            stop.insert("keys".into(), Value::Sequence(keys));
          }
          STOP_CMD => {
            if value.is_empty() {
              return Err(field("stop.value", "what to run?".into()));
            }
            let cmd =
              argv_value(&value).map_err(|err| field("stop.value", err))?;
            stop.remove("group");
            stop.insert("cmd".into(), cmd);
          }
          signal => {
            stop.insert("signal".into(), Value::from(signal));
          }
        }
      }
      match self.value("stop.timeout").as_str() {
        "" => stop.remove("timeout"),
        timeout => stop.insert("timeout".into(), Value::from(timeout)),
      };
      spec.set("stop", (!stop.is_empty()).then_some(Value::Mapping(stop)));
    }

    // As the kernel keeps it, so an edit that changes nothing is none.
    let spec = spec
      .normalized(&path, &self.cx.root)
      .map_err(|err| (None, format!("{err:#}")))?;
    Ok((path, spec))
  }

  fn submit(&mut self) -> ModalResult {
    for field in &mut self.form.fields {
      field.error = None;
    }
    self.error = None;
    let (path, spec) = match self.build() {
      Ok(built) => built,
      Err((Some(id), err)) => {
        if let Some(field) = self.form.field_mut(id) {
          field.error = Some(err);
        }
        self.form.focus(id);
        return ModalResult::Keep;
      }
      Err((None, err)) => {
        self.error = Some(err);
        return ModalResult::Keep;
      }
    };
    let start = self.form.field("start").is_some_and(Field::on);
    let commands = match &self.mode {
      // The whole entry: it opened on the `defaults` it gets.
      TaskDialogMode::Add { after, .. } => vec![Command::Add {
        target: Target::glob(&path),
        spec,
        place: after.clone().map_or(Place::Keep, Place::After),
        start,
        defaults: false,
        origin: None,
      }],
      TaskDialogMode::Edit {
        path: old, running, ..
      } => {
        let mut commands = Vec::new();
        self.sent = None;
        if spec != self.spec {
          self.sent = Some(spec.clone());
          commands.push(Command::Update {
            target: Target::glob(old),
            spec,
            was: Some(self.spec.clone()),
            restart: *running && start,
          });
        }
        if path != *old {
          commands.push(Command::Move {
            target: Target::glob(old),
            to: Some(Target::glob(&path)),
            place: Place::Keep,
          });
        }
        if commands.is_empty() {
          return ModalResult::Close;
        }
        commands
      }
    };
    self.busy = true;
    ModalResult::Issue(commands)
  }
}

/// A value as one line for a field.
fn show(value: &Value) -> String {
  match value {
    Value::Null => String::new(),
    Value::Sequence(items) => {
      join_argv(&items.iter().map(show).collect::<Vec<_>>())
    }
    value => show_value(Some(value)),
  }
}

/// How a task's `stop` shows: the way it stops (a signal, `keys`, or
/// `command`), what to type or run, and the timeout.
fn stop_of(stop: Option<&Value>) -> (String, String, String) {
  let default = SIGNALS[0].to_string();
  match stop {
    Some(Value::String(signal)) => {
      (signal.clone(), String::new(), String::new())
    }
    Some(Value::Mapping(stop)) => {
      let timeout = stop.get("timeout").map(show).unwrap_or_default();
      if let Some(keys) = stop.get("keys") {
        // Keys are words of their own, not a command line to quote.
        let keys = match keys {
          Value::Sequence(keys) => {
            keys.iter().map(show).collect::<Vec<_>>().join(" ")
          }
          keys => show(keys),
        };
        (STOP_KEYS.to_string(), keys, timeout)
      } else if let Some(cmd) = stop.get("cmd") {
        (STOP_CMD.to_string(), show(cmd), timeout)
      } else {
        let signal = stop.get("signal").map_or(default, show);
        (signal, String::new(), timeout)
      }
    }
    _ => (default, String::new(), String::new()),
  }
}

/// A command line as the argv a config holds.
fn argv_value(line: &str) -> Result<Value, String> {
  let argv =
    crate::parse_shell::split_argv(line).map_err(|err| err.to_string())?;
  Ok(Value::Sequence(
    argv.into_iter().map(Value::String).collect(),
  ))
}

fn field_value(field: &Field) -> String {
  match &field.kind {
    FieldKind::Toggle(on) | FieldKind::Section { open: on } => on.to_string(),
    FieldKind::Text { .. } | FieldKind::Choice { .. } => {
      field.value().to_string()
    }
  }
}

impl Modal for TaskDialog {
  fn handle_key(&mut self, key: &Key) -> ModalResult {
    if self.busy {
      return ModalResult::Keep;
    }
    let event = self.form.handle_key(key);
    self.update();
    match event {
      FormEvent::Submit => self.submit(),
      FormEvent::Cancel => ModalResult::Close,
      FormEvent::Keep => ModalResult::Keep,
    }
  }

  fn handle_mouse(&mut self, mouse: &MouseEvent) -> ModalResult {
    if self.busy {
      return ModalResult::Keep;
    }
    if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
      let (x, y) = (mouse.x as u16, mouse.y as u16);
      if self.buttons[0].contains(x, y) {
        return ModalResult::Close;
      }
      if self.buttons[1].contains(x, y) {
        return self.submit();
      }
    }
    self.form.handle_mouse(mouse);
    self.update();
    ModalResult::Keep
  }

  fn handle_paste(&mut self, text: &str) -> ModalResult {
    if !self.busy {
      self.form.paste(text);
      self.update();
    }
    ModalResult::Keep
  }

  /// Closes on a click outside, unless that would drop what was typed or
  /// what is being saved.
  fn click_outside(&mut self) -> ModalResult {
    let edited = self.initial.iter().any(|(id, then)| {
      self.form.field(id).map(field_value).as_ref() != Some(then)
    });
    if self.busy || edited {
      ModalResult::Keep
    } else {
      ModalResult::Close
    }
  }

  fn issued(&mut self, result: Result<(), IssueError>) -> ModalResult {
    self.busy = false;
    match result {
      Ok(()) => ModalResult::Close,
      Err(IssueError { done, message }) => {
        // The edit went in: saving again only moves it.
        if let Some(spec) = self.sent.take().filter(|_| done > 0) {
          self.spec = spec;
        }
        self.error = Some(message);
        ModalResult::Keep
      }
    }
  }

  fn size(&self, frame: Rect) -> (u16, u16) {
    let note = u16::from(self.note().is_some());
    // Border, note, blank, fields, blank, buttons, border.
    let height = 2 + note * 2 + self.form.height() + 2;
    (76.min(frame.width), height.min(frame.height))
  }

  fn render(&mut self, grid: &mut Grid, _keymap: &Keymap, theme: &Theme) {
    let area = self.area(grid.area());
    let style = theme.block.modal();
    let mut block = style.draw(grid, area);
    block.title(self.title(), style.title);
    let mut inner = block.inner().inner((0, 1));
    if let Some(note) = self.note() {
      grid.draw_text(inner.take_top(1), &note, style.dim);
      inner.take_top(1);
    }
    let buttons_row = inner.take_bottom(1);
    inner.take_bottom(1);
    grid.cursor_pos = self.form.render(grid, inner, &style, theme);
    grid.cursor_style = CursorStyle::BlinkingBar;

    let mut row = buttons_row;
    let action = if self.busy {
      format!("[ {}… ]", self.action())
    } else {
      format!("[ {} ]", self.action())
    };
    let cancel = "[ Cancel ]";
    let primary = Style {
      bg: theme.block.border_focused.into(),
      fg: theme.block.modal_bg.into(),
      ..style.text
    }
    .set_bold(true);
    self.buttons[1] = row.take_right(action.width() as u16);
    grid.draw_text(self.buttons[1], &action, primary);
    row.take_right(2);
    self.buttons[0] = row.take_right(cancel.width() as u16);
    grid.draw_text(self.buttons[0], cancel, style.text);
    row.take_right(2);
    if let Some(error) = &self.error {
      grid.draw_text(row, error, style.error);
    }

    let submit = self.action().to_lowercase();
    let mut hints = vec![("<Tab>", "next")];
    let enter = match self.form.focused().map(|f| &f.kind) {
      Some(FieldKind::Text { complete, .. })
        if !matches!(complete, Completion::None) =>
      {
        hints.push(("<C-n>", "complete"));
        submit.as_str()
      }
      Some(FieldKind::Choice { dropdown: true, .. }) => {
        hints.push(("<Space>", "choose"));
        submit.as_str()
      }
      Some(FieldKind::Section { open: true }) => "hide",
      Some(FieldKind::Section { open: false }) => "show",
      _ => submit.as_str(),
    };
    hints.extend([("<CR>", enter), ("<Esc>", "cancel")]);
    draw_hints(grid, area, &hints, &style);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::term::key::{KeyCode, KeyMods};

  fn cx() -> DialogContext {
    DialogContext {
      root: PathBuf::from("/proj"),
      paths: vec!["db".into(), "web/api".into()],
      system_shell: false,
    }
  }

  fn key(code: KeyCode) -> Key {
    Key::new(code, KeyMods::NONE)
  }

  fn typed(dialog: &mut TaskDialog, text: &str) {
    for c in text.chars() {
      dialog.handle_key(&key(KeyCode::Char(c)));
    }
  }

  fn issued(result: ModalResult) -> Vec<Command> {
    match result {
      ModalResult::Issue(commands) => commands,
      _ => panic!("nothing issued"),
    }
  }

  #[test]
  fn adds_a_task_under_the_group_with_a_path_from_its_program() {
    let mut dialog = TaskDialog::new(
      TaskDialogMode::Add {
        after: Some(TaskPath::new("web/api").unwrap()),
        copy: false,
      },
      TaskSpec::default(),
      cx(),
      "web/".into(),
    );
    typed(&mut dialog, "npm run 'my dev'");
    let commands = issued(dialog.handle_key(&key(KeyCode::Enter)));
    let [
      Command::Add {
        target,
        spec,
        place,
        start: true,
        ..
      },
    ] = &commands[..]
    else {
      panic!("{commands:?}")
    };
    assert_eq!(target.to_string(), "web/npm");
    assert_eq!(*place, Place::After(TaskPath::new("web/api").unwrap()));
    assert_eq!(spec.argv().unwrap(), ["npm", "run", "my dev"]);
    assert_eq!(spec.label(), None);
  }

  /// Nor under itself, but up to the group it alone is in, whose place
  /// it takes.
  #[test]
  fn an_edited_task_cannot_move_under_itself() {
    let dialog = edit("{cmd: [api]}");
    assert!(dialog.taken("web/api/dev"));
    assert!(!dialog.taken("web"));
    assert!(!dialog.taken("web/api"));
    assert!(!dialog.taken("api"));
    assert!(dialog.taken("db/x"));
  }

  #[test]
  fn refuses_a_taken_path_and_a_bad_command_in_place() {
    let mut dialog = TaskDialog::new(
      TaskDialogMode::Add {
        after: None,
        copy: false,
      },
      TaskSpec::default(),
      cx(),
      String::new(),
    );
    typed(&mut dialog, "a && b");
    assert!(matches!(
      dialog.handle_key(&key(KeyCode::Enter)),
      ModalResult::Keep
    ));
    assert!(dialog.form.field("cmd").unwrap().error.is_some());

    let mut dialog = TaskDialog::new(
      TaskDialogMode::Add {
        after: None,
        copy: false,
      },
      TaskSpec::default(),
      cx(),
      String::new(),
    );
    typed(&mut dialog, "x");
    dialog.form.focus("path");
    typed(&mut dialog, "web");
    assert!(matches!(
      dialog.handle_key(&key(KeyCode::Enter)),
      ModalResult::Keep
    ));
    assert!(dialog.form.field("path").unwrap().error.is_some());
  }

  #[test]
  fn edits_only_what_changed_and_moves_on_a_new_path() {
    let spec = canonical(
      "{cmd: [cargo, run], cwd: /proj/api, autorestart: never, \
       env: {A: '1'}, log: {dir: /tmp}}",
    );
    let mut dialog = TaskDialog::new(
      TaskDialogMode::Edit {
        path: "web/api".into(),
        running: true,
        origin: None,
      },
      spec.clone(),
      cx(),
      "web/api".into(),
    );
    assert_eq!(dialog.value("cwd"), "api", "shown from the root");
    // Nothing changed: nothing to do.
    assert!(matches!(
      dialog.handle_key(&key(KeyCode::Enter)),
      ModalResult::Close
    ));
    dialog.form.focus("title");
    typed(&mut dialog, "API");
    dialog.form.focus("env.1");
    typed(&mut dialog, "B=2");
    dialog.form.focus("path");
    dialog.handle_key(&Key::new(KeyCode::Char('u'), KeyMods::CONTROL));
    typed(&mut dialog, "api");
    let commands = issued(dialog.handle_key(&key(KeyCode::Enter)));
    let [
      Command::Update {
        target,
        spec: new,
        was: Some(was),
        restart: true,
      },
      Command::Move { to: Some(to), .. },
    ] = &commands[..]
    else {
      panic!("{commands:?}")
    };
    // Made from what the task had when the dialog opened.
    assert_eq!(*was, spec);
    assert_eq!(target.to_string(), "web/api");
    assert_eq!(to.to_string(), "api");
    assert_eq!(new.label(), Some("API"));
    assert_eq!(
      new.get("env").unwrap(),
      &serde_yaml::from_str::<Value>("{A: '1', B: '2'}").unwrap()
    );
    // Untouched keys keep how they were written.
    assert_eq!(new.get("autorestart"), spec.get("autorestart"));
    assert_eq!(new.get("log"), spec.get("log"));
    assert_eq!(new.get("cwd"), spec.get("cwd"));
  }

  /// An edit that went in before its move failed is not sent again:
  /// saving once more only moves the task.
  #[test]
  fn after_a_failed_move_saving_again_only_moves() {
    let mut dialog = edit("{cmd: [api]}");
    typed(&mut dialog, " --v2");
    dialog.form.focus("path");
    dialog.handle_key(&Key::new(KeyCode::Char('u'), KeyMods::CONTROL));
    typed(&mut dialog, "api");
    let commands = issued(dialog.handle_key(&key(KeyCode::Enter)));
    assert!(matches!(
      &commands[..],
      [Command::Update { .. }, Command::Move { .. }]
    ));
    let failed = IssueError {
      done: 1,
      message: "a task already exists at 'api'".into(),
    };
    assert!(matches!(dialog.issued(Err(failed)), ModalResult::Keep));
    let commands = issued(dialog.handle_key(&key(KeyCode::Enter)));
    assert!(
      matches!(&commands[..], [Command::Move { .. }]),
      "{commands:?}"
    );
  }

  /// A spec as the kernel keeps one, which the dialog edits.
  fn canonical(yaml: &str) -> TaskSpec {
    serde_yaml::from_str::<TaskSpec>(yaml)
      .unwrap()
      .normalized("web/api", Path::new("/proj"))
      .unwrap()
  }

  /// A paste goes into the field with the focus, on one line; a click
  /// outside closes the dialog only while nothing was typed.
  #[test]
  fn a_paste_goes_into_the_field_and_a_click_outside_keeps_it() {
    let mut dialog = edit("{cmd: [api]}");
    assert!(matches!(dialog.click_outside(), ModalResult::Close));
    dialog.form.focus("cmd");
    dialog.handle_key(&Key::new(KeyCode::Char('u'), KeyMods::CONTROL));
    dialog.handle_paste("cargo run\n  --release\n");
    assert_eq!(dialog.value("cmd"), "cargo run   --release");
    assert!(matches!(dialog.click_outside(), ModalResult::Keep));
  }

  fn edit(spec: &str) -> TaskDialog {
    TaskDialog::new(
      TaskDialogMode::Edit {
        path: "web/api".into(),
        running: false,
        origin: None,
      },
      canonical(spec),
      cx(),
      "web/api".into(),
    )
  }

  fn saved(dialog: &mut TaskDialog) -> TaskSpec {
    let commands = issued(dialog.handle_key(&key(KeyCode::Enter)));
    let [Command::Update { spec, .. }] = &commands[..] else {
      panic!("{commands:?}")
    };
    spec.clone()
  }

  fn choose(dialog: &mut TaskDialog, id: &str, option: &str) {
    dialog.form.focus(id);
    dialog.handle_key(&key(KeyCode::Char(' ')));
    for _ in 0..10 {
      if dialog.form.focused().unwrap().value() == option {
        break;
      }
      dialog.handle_key(&key(KeyCode::Right));
    }
    assert_eq!(dialog.value(id), option);
  }

  fn shown(dialog: &TaskDialog, id: &str) -> bool {
    let index = dialog.form.fields.iter().position(|f| f.id == id).unwrap();
    dialog.form.shown(index)
  }

  #[test]
  fn ready_takes_the_fields_of_its_check() {
    let mut dialog = edit("{cmd: [pg]}");
    assert!(!shown(&dialog, "ready"), "under More");
    dialog.form.focus("ready");
    for id in ["ready.value", "ready.interval", "ready.timeout"] {
      assert!(!shown(&dialog, id), "{id} while it starts");
    }
    choose(&mut dialog, "ready", "output has");
    assert!(shown(&dialog, "ready.value") && shown(&dialog, "ready.timeout"));
    assert!(
      !shown(&dialog, "ready.interval"),
      "the output is not polled"
    );
    choose(&mut dialog, "ready", "port opens");
    assert!(shown(&dialog, "ready.interval"));
    dialog.form.focus("ready.value");
    typed(&mut dialog, "5432");
    dialog.form.focus("ready.interval");
    typed(&mut dialog, "2s");
    assert_eq!(
      saved(&mut dialog).get("ready").unwrap(),
      &serde_yaml::from_str::<Value>("{tcp: 5432, interval: 2s}").unwrap()
    );
  }

  /// Off is written, not left out: `defaults` would turn it back on.
  #[test]
  fn autostart_turned_off_says_so() {
    let mut dialog = edit("{cmd: [pg], autostart: true}");
    dialog.form.focus("autostart");
    dialog.handle_key(&key(KeyCode::Char(' ')));
    assert_eq!(
      saved(&mut dialog).get("autostart"),
      Some(&Value::Bool(false))
    );
  }

  /// A new task's dialog opens on the `defaults` it gets, and adds the
  /// whole entry: what is cleared there stays cleared.
  #[test]
  fn a_new_task_shows_its_defaults_and_can_leave_them() {
    let mut dialog = TaskDialog::new(
      TaskDialogMode::Add {
        after: None,
        copy: false,
      },
      serde_yaml::from_str(
        "{autostart: true, cwd: /proj/sub, env: {RUST_LOG: debug}}",
      )
      .unwrap(),
      cx(),
      String::new(),
    );
    assert_eq!(dialog.value("cwd"), "sub");
    assert_eq!(dialog.value("env.0"), "RUST_LOG=debug");
    assert!(dialog.form.field("autostart").is_some_and(Field::on));
    typed(&mut dialog, "pg");
    for id in ["cwd", "env.0"] {
      dialog.form.focus(id);
      dialog.handle_key(&Key::new(KeyCode::Char('u'), KeyMods::CONTROL));
    }
    let commands = issued(dialog.handle_key(&key(KeyCode::Enter)));
    let [
      Command::Add {
        spec,
        defaults: false,
        ..
      },
    ] = &commands[..]
    else {
      panic!("{commands:?}")
    };
    assert_eq!(spec.get("cwd"), None);
    assert_eq!(spec.get("env"), None);
    assert_eq!(spec.get("autostart"), Some(&Value::Bool(true)));
  }

  #[test]
  fn a_job_takes_no_ready_check_and_no_restart_always() {
    let mut dialog =
      edit("{cmd: [make], autorestart: always, ready: {log: done}}");
    assert!(dialog.form.field("more").unwrap().hint.is_some());
    dialog.form.focus("type");
    dialog.handle_key(&key(KeyCode::Right));
    assert!(!shown(&dialog, "ready"));
    assert_eq!(dialog.value("restart"), "on-failure");
    let spec = saved(&mut dialog);
    assert_eq!(spec.get("type"), Some(&Value::from("job")));
    assert_eq!(spec.get("autorestart"), Some(&Value::from("on-failure")));
    assert_eq!(spec.get("ready"), None);
  }

  #[test]
  fn stop_with_a_signal_keys_or_a_command() {
    let mut dialog = edit("{cmd: [up], stop: SIGUSR2}");
    assert_eq!(dialog.value("stop"), "SIGUSR2", "its own signal is offered");
    assert!(!shown(&dialog, "stop.value"));
    choose(&mut dialog, "stop", "command");
    assert!(shown(&dialog, "stop.value"));
    assert!(matches!(
      dialog.handle_key(&key(KeyCode::Enter)),
      ModalResult::Keep
    ));
    assert_eq!(dialog.form.focused().unwrap().id, "stop.value");
    typed(&mut dialog, "docker compose stop");
    assert_eq!(
      saved(&mut dialog).get("stop").unwrap(),
      &serde_yaml::from_str::<Value>("{cmd: [docker, compose, stop]}").unwrap()
    );

    let mut dialog = edit("{cmd: [up], stop: {keys: ['<C-c>'], timeout: 5s}}");
    assert_eq!(dialog.value("stop.value"), "<C-c>");
    choose(&mut dialog, "stop", "SIGINT");
    dialog.form.focus("stop.timeout");
    dialog.handle_key(&Key::new(KeyCode::Char('u'), KeyMods::CONTROL));
    assert_eq!(
      saved(&mut dialog).get("stop").unwrap(),
      &serde_yaml::from_str::<Value>("{signal: SIGINT}").unwrap()
    );
  }
}
