//! The reload dialog: what the config on disk would change, one row per
//! task, each taken or kept, and what changed in the selected one.

use crate::console::theme::BlockStyle;
use crate::reload::{ChangeKind, Plan};
use crate::spec::show_value;
use crate::term::{
  Grid,
  grid::Rect,
  key::{Key, KeyCode, KeyMods},
};

use super::checklist::{
  Check, CheckRow, Checklist, ChecklistModal, key_column, listed,
};
use super::modal::ModalResult;

pub type ReloadModal = ChecklistModal<Plan>;

impl Checklist for Plan {
  const TITLE: &'static str = "Reload the config";
  const NOTHING: &'static str = "Nothing new in the config.";
  const DETAIL: u16 = 8;

  fn len(&self) -> usize {
    Plan::len(self)
  }

  fn row(&self, index: usize) -> CheckRow {
    let check = |on: bool| if on { Check::On } else { Check::Off };
    let Some(change) = self.changes.get(index) else {
      let order = self.order.as_ref().expect("the last row");
      return CheckRow {
        check: check(order.apply),
        mark: "↕",
        path: "order".to_string(),
        what: "the config reordered its tasks".to_string(),
        note: order.conflict.then(|| "reordered here too".to_string()),
      };
    };
    let what = match change.kind {
      ChangeKind::Added => "new in the config".to_string(),
      ChangeKind::Removed => "gone from the config".to_string(),
      ChangeKind::RemovedHere => "removed here; add it back".to_string(),
      ChangeKind::Written => "a config task from now on".to_string(),
      ChangeKind::Changed => {
        listed(change.fields.iter().map(|f| f.key.as_str()))
      }
    };
    CheckRow {
      check: if change.mixed() {
        Check::Partly
      } else {
        check(change.apply)
      },
      mark: change.kind.mark(),
      path: change.path.clone(),
      what,
      note: change.reason.as_ref().map(crate::reload::Reason::text),
    }
  }

  fn toggle(&mut self, index: usize) {
    if let Some(change) = self.changes.get(index) {
      self.set_apply(index, change.mixed() || !change.apply);
    } else if let Some(order) = &mut self.order {
      order.apply = !order.apply;
    }
  }

  fn key(&mut self, key: &Key) -> Option<ModalResult> {
    match (key.code, key.mods) {
      (KeyCode::Enter, KeyMods::NONE) => {
        Some(ModalResult::ApplyReload(Box::new(self.clone())))
      }
      (KeyCode::Char('a'), KeyMods::NONE) => {
        self.toggle_all();
        Some(ModalResult::Keep)
      }
      _ => None,
    }
  }

  fn hints(&self) -> Vec<(&'static str, &'static str)> {
    vec![
      ("<Space>", "take/keep"),
      ("a", "all"),
      ("<CR>", "apply"),
      ("<Esc>", "cancel"),
    ]
  }

  fn summary(&self) -> Option<String> {
    let conflicts = self
      .changes
      .iter()
      .filter(|c| c.fields.iter().any(|f| f.conflict))
      .count();
    Some(match conflicts {
      0 => format!("{} to look at", Plan::len(self)),
      n => format!("{} to look at · {n} changed here too", Plan::len(self)),
    })
  }

  fn footer(&self) -> Option<String> {
    (!self.edited_here.is_empty())
      .then(|| format!("Kept as changed here: {}", self.edited_here.join(", ")))
  }

  /// Each field the config changed, here and in the config, the one
  /// taken lit; or what taking an added or removed task does.
  fn detail(
    &self,
    index: usize,
    grid: &mut Grid,
    area: Rect,
    style: BlockStyle,
  ) {
    let Some(change) = self.changes.get(index) else {
      return;
    };
    let mut rows = area.rows();
    let key_w = key_column(change.fields.iter().map(|f| f.key.as_str()));
    for field in &change.fields {
      let (Some(mut here), Some(mut config)) = (rows.next(), rows.next())
      else {
        break;
      };
      grid.draw_text(here.take_left(key_w), &field.key, style.key);
      config.take_left(key_w);
      here.take_left(2);
      config.take_left(2);
      let (here_mark, config_mark) = if field.take {
        (style.dim, style.key)
      } else {
        (style.key, style.dim)
      };
      grid.draw_text(here.take_left(8), "here", here_mark);
      grid.draw_text(config.take_left(8), "config", config_mark);
      grid.draw_text(here, &show_value(field.here.as_ref()), style.text);
      grid.draw_text(config, &show_value(field.config.as_ref()), style.text);
    }
    if change.fields.is_empty() {
      let what = match change.kind {
        ChangeKind::Added => "Added idle; `up` starts it if it autostarts.",
        ChangeKind::Removed => "Removed; a running one is stopped first.",
        ChangeKind::RemovedHere => "Added back, idle.",
        ChangeKind::Written => "Kept as it is; the config has it now.",
        ChangeKind::Changed => "",
      };
      if let Some(row) = rows.next() {
        grid.draw_text(row, what, style.dim);
      }
    }
  }
}
