//! The write-back dialog: what changed here and is not in the config,
//! one row per task, written by the writer, an agent, or copied as a
//! prompt.

use crate::console::theme::BlockStyle;
use crate::spec::show_value;
use crate::term::{
  Grid,
  grid::Rect,
  key::{Key, KeyCode, KeyMods},
};
use crate::write_back::{How, WriteKind, WritePlan};

use super::checklist::{
  Check, CheckRow, Checklist, ChecklistModal, key_column, listed,
};
use super::modal::ModalResult;

pub type WriteBackModal = ChecklistModal<WriteBack>;

pub struct WriteBack {
  pub plan: WritePlan,
  /// The agent's program, when one is set up or found.
  pub agent: Option<String>,
}

impl WriteBack {
  fn result(&self, how: How) -> ModalResult {
    if self.plan.chosen().next().is_none() {
      return ModalResult::Keep;
    }
    ModalResult::WriteBack(Box::new(self.plan.clone()), how)
  }
}

impl Checklist for WriteBack {
  const TITLE: &'static str = "Write back to the config";
  const NOTHING: &'static str = "The config has everything changed here.";
  const DETAIL: u16 = 7;

  fn len(&self) -> usize {
    self.plan.items.len()
  }

  fn row(&self, index: usize) -> CheckRow {
    let item = &self.plan.items[index];
    let (mark, what) = match item.kind {
      WriteKind::Changed => {
        ("~", listed(item.fields.iter().map(|f| f.key.as_str())))
      }
      WriteKind::Added => ("+", format!("add to {}", item.source.file)),
      WriteKind::Removed => ("-", format!("remove from {}", item.source.file)),
    };
    CheckRow {
      check: if item.apply { Check::On } else { Check::Off },
      mark,
      path: item.path.clone(),
      what,
      note: item.note.clone(),
    }
  }

  fn toggle(&mut self, index: usize) {
    if let Some(item) = self.plan.items.get_mut(index) {
      item.apply = !item.apply;
    }
  }

  fn key(&mut self, key: &Key) -> Option<ModalResult> {
    match (key.code, key.mods) {
      (KeyCode::Enter | KeyCode::Char('w'), KeyMods::NONE) => {
        Some(self.result(How::Write))
      }
      (KeyCode::Char('A'), KeyMods::NONE) if self.agent.is_some() => {
        Some(self.result(How::Agent))
      }
      (KeyCode::Char('y'), KeyMods::NONE) => Some(self.result(How::Copy)),
      _ => None,
    }
  }

  fn hints(&self) -> Vec<(&'static str, &'static str)> {
    let mut hints = vec![("<Space>", "pick"), ("<CR>", "write")];
    if self.agent.is_some() {
      hints.push(("A", "ask agent"));
    }
    hints.extend([("y", "copy prompt"), ("<Esc>", "cancel")]);
    hints
  }

  /// The entry, then each key as the file has it and as it will.
  fn detail(
    &self,
    index: usize,
    grid: &mut Grid,
    area: Rect,
    style: BlockStyle,
  ) {
    let item = &self.plan.items[index];
    let mut rows = area.rows();
    if let Some(row) = rows.next() {
      let what = format!("{}: task {}", item.source.file, item.source.key);
      grid.draw_text(row, &what, style.dim);
    }
    let key_w = key_column(item.fields.iter().map(|f| f.key.as_str()));
    for field in &item.fields {
      let Some(mut row) = rows.next() else { break };
      grid.draw_text(row.take_left(key_w), &field.key, style.key);
      row.take_left(2);
      let now = show_value(field.now.as_ref());
      let text = match item.kind {
        WriteKind::Added => now,
        WriteKind::Changed | WriteKind::Removed => {
          format!("{} → {now}", show_value(field.was.as_ref()))
        }
      };
      grid.draw_text(row, &text, style.text);
    }
  }
}
