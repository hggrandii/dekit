use crate::command::Command;
use crate::console::action::Action;
use crate::console::keymap::Keymap;
use crate::console::theme::Theme;
use crate::target::Target;
use crate::term::{
  Grid,
  grid::Rect,
  key::{Key, KeyCode},
};
use unicode_width::UnicodeWidthStr;

use super::modal::{Modal, ModalResult};

/// Asks before removing a task, or a group with its tasks.
pub struct RemoveTaskModal {
  pub target: Target,
  /// "task web/api", or "group web and its 3 tasks".
  pub what: String,
  /// Running: a removal kills it.
  pub running: bool,
}

impl RemoveTaskModal {
  fn lines(&self) -> Vec<String> {
    let mut lines = vec![format!("Remove {}?", self.what)];
    if self.running {
      lines.push("It is running: it will be killed.".to_string());
    }
    lines.push("y remove   n keep".to_string());
    lines
  }
}

impl Modal for RemoveTaskModal {
  fn handle_key(&mut self, key: &Key) -> ModalResult {
    if !key.mods.is_empty() {
      return ModalResult::Keep;
    }
    match key.code {
      KeyCode::Char('y') => ModalResult::Run(Action::Command {
        command: Command::Remove {
          target: self.target.clone(),
        },
      }),
      KeyCode::Char('n') | KeyCode::Esc => ModalResult::Close,
      _ => ModalResult::Keep,
    }
  }

  fn size(&self, frame: Rect) -> (u16, u16) {
    let width = self.lines().iter().map(|l| l.width()).max().unwrap_or(0);
    (
      (width as u16 + 6).max(36).min(frame.width),
      self.lines().len() as u16 + 2,
    )
  }

  fn render(&mut self, grid: &mut Grid, _keymap: &Keymap, theme: &Theme) {
    let area = self.area(grid.area());
    let style = theme.block.modal();
    let inner = style
      .draw(grid, area)
      .title(" Remove ", style.title)
      .inner()
      .inner((0, 2));
    let lines = self.lines();
    let last = lines.len() - 1;
    for (i, (row, line)) in inner.rows().zip(lines).enumerate() {
      let attrs = if i == last {
        style.dim
      } else if i > 0 {
        style.error
      } else {
        style.text
      };
      grid.draw_text(row, &line, attrs);
    }
  }
}
