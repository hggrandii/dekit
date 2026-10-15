use tui_input::Input;
use unicode_width::UnicodeWidthStr;

use crate::console::action::{Action, RunnerTab, ScrollUnit};
use crate::console::{
  keymap::{Keymap, KeymapGroup},
  theme::Theme,
  widgets::{
    list::ListState,
    text_input::{render_filter_row, to_input_request},
  },
};
use crate::term::{
  CursorStyle, Grid,
  grid::Rect,
  key::{Key, KeyCode, KeyMods},
  style::Style,
};

use super::modal::{Modal, ModalResult};

pub struct CommandsMenuModal {
  input: Input,
  list: ListState,
  items: Vec<MenuItem>,
}

struct MenuItem {
  name: String,
  desc: String,
  action: Action,
}

impl CommandsMenuModal {
  pub fn new() -> Self {
    let items = menu_items("");
    let mut list = ListState::new(0);
    list.reset(items.len());
    CommandsMenuModal {
      input: Input::default(),
      list,
      items,
    }
  }
}

impl Modal for CommandsMenuModal {
  fn handle_key(&mut self, key: &Key) -> ModalResult {
    match (key.code, key.mods) {
      (KeyCode::Enter, KeyMods::NONE) => {
        let item = self.list.selected().and_then(|i| self.items.get(i));
        return match item {
          Some(item) => ModalResult::Run(item.action.clone()),
          None => ModalResult::Close,
        };
      }
      (KeyCode::Esc, KeyMods::NONE) => return ModalResult::Close,
      _ => (),
    }
    if !self.list.handle_key(key)
      && let Some(req) = to_input_request(key)
      && self.input.handle(req).is_some_and(|change| change.value)
    {
      self.items = menu_items(&self.input.value().to_lowercase());
      self.list.reset(self.items.len());
    }
    ModalResult::Keep
  }

  fn size(&self, _frame: Rect) -> (u16, u16) {
    (60, 30)
  }

  fn render(&mut self, grid: &mut Grid, keymap: &Keymap, theme: &Theme) {
    let area = self.area(grid.area());
    let style = theme.block.modal();
    let mut block = style.draw(grid, area);
    block.title(" Commands ", style.title);
    let mut inner = block.inner();
    let input_row = inner.take_top(1);
    let sep_row = inner.take_top(1);
    let list_area = inner;
    block.separator(sep_row.y);

    let counter = self.list.selected().map(|i| (i, self.items.len()));
    grid.cursor_pos = Some(render_filter_row(
      grid,
      input_row,
      "/ ",
      &self.input,
      counter,
      &style,
    ));
    grid.cursor_style = CursorStyle::BlinkingBar;

    let search = self.input.value().to_lowercase();
    for row in self.list.rows(list_area) {
      let item = &self.items[row.index];
      let style = theme.block.modal_row(style, row.selected);
      if row.selected {
        grid.fill_area(row.area, ' ', style.text);
        grid.draw_text(row.area, "\u{258e}", style.key);
      }

      // The description gets what the key leaves. The action's name is
      // not shown: actions with fields would share it. It still matches
      // the search.
      let mut rest = row.area.inner((0, 1, 0, 2));
      if let Some(key) = keymap.key(KeymapGroup::Tasks, &item.action) {
        let key = key.to_string();
        grid.draw_text(rest.take_right(key.width() as u16), &key, style.key);
        rest.take_right(1);
      }
      draw_highlighted(grid, rest, &item.desc, &search, style.text, style.key);
    }
  }
}

fn draw_highlighted(
  grid: &mut Grid,
  mut area: Rect,
  text: &str,
  search: &str,
  base: Style,
  hl: Style,
) {
  let mut draw = |area: &mut Rect, s: &str, attrs: Style| {
    grid.draw_text(area.take_left(s.width() as u16), s, attrs);
  };
  if search.is_empty() {
    draw(&mut area, text, base);
    return;
  }
  let lower = text.to_lowercase();
  let mut last = 0;
  for (start, _) in lower.match_indices(search) {
    let end = start + search.len();
    if start < last
      || !text.is_char_boundary(start)
      || !text.is_char_boundary(end)
    {
      continue;
    }
    draw(&mut area, &text[last..start], base);
    draw(&mut area, &text[start..end], hl);
    last = end;
  }
  draw(&mut area, &text[last..], base);
}

fn menu_items(search: &str) -> Vec<MenuItem> {
  let actions = [
    Action::Detach,
    Action::Quit,
    Action::ToggleFocus,
    Action::FocusTerm,
    Action::Zoom,
    Action::ShowCommandsMenu,
    Action::ShowMainMenu,
    Action::ShowRunnerSelect {
      tab: RunnerTab::Switch,
    },
    Action::ShowRunnerSelect {
      tab: RunnerTab::Open,
    },
    Action::NextTask,
    Action::PrevTask,
    Action::Expand,
    Action::Collapse,
    Action::StartTask,
    Action::StopTask,
    Action::KillTask,
    Action::VetoTask,
    Action::RestartTask,
    Action::RestartAll,
    Action::DuplicateTask,
    Action::ForceRestartTask,
    Action::ForceRestartAll,
    Action::ShowAddTask,
    Action::ShowRenameTask,
    Action::ShowRemoveTask,
    Action::CloseCurrentModal,
    Action::ScrollDown {
      n: 1,
      unit: ScrollUnit::HalfScreen,
    },
    Action::ScrollUp {
      n: 1,
      unit: ScrollUnit::HalfScreen,
    },
    Action::CopyModeEnter,
    Action::CopyModeLeave,
    Action::CopyModeEnd,
    Action::CopyModeCopy,
  ];
  actions
    .into_iter()
    .map(|action| MenuItem {
      name: action.name(),
      desc: action.desc(),
      action,
    })
    .filter(|item| {
      item.name.contains(search) || item.desc.to_lowercase().contains(search)
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The description is all a row shows, so two actions must not share
  /// one.
  #[test]
  fn descriptions_are_unique() {
    let items = menu_items("");
    let mut descs: Vec<&str> = items.iter().map(|i| i.desc.as_str()).collect();
    descs.sort_unstable();
    let before = descs.len();
    descs.dedup();
    assert_eq!(descs.len(), before, "{descs:?}");
  }

  #[test]
  fn the_search_matches_the_name_too() {
    let items = menu_items("show-runner");
    assert_eq!(items.len(), 2, "both runner-select spellings");
    assert!(items.iter().all(|i| i.name == "show-runner-select"));
    assert!(menu_items("open project").len() == 1);
  }
}
