use unicode_width::UnicodeWidthStr;

use crate::console::action::{Action, RunnerTab};
use crate::console::{
  keymap::{Keymap, KeymapGroup},
  theme::Theme,
  widgets::list::ListState,
};
use crate::term::{
  Grid,
  grid::Rect,
  key::{Key, KeyCode, KeyMods},
  mouse::MouseEvent,
};

use super::modal::{Modal, ModalResult};

/// The menu that drops down from the `dekit` badge in the header.
pub struct MainMenuModal {
  /// The badge, which the menu hangs under.
  anchor: Rect,
  list: ListState,
  items: Vec<Item>,
}

struct Item {
  label: &'static str,
  action: Action,
}

impl MainMenuModal {
  pub fn new(anchor: Rect) -> Self {
    let items = vec![
      Item {
        label: "All commands",
        action: Action::ShowCommandsMenu,
      },
      Item {
        label: "Open project",
        action: Action::ShowRunnerSelect {
          tab: RunnerTab::Open,
        },
      },
      Item {
        label: "Switch project",
        action: Action::ShowRunnerSelect {
          tab: RunnerTab::Switch,
        },
      },
      Item {
        label: "Quit",
        action: Action::ShowQuit,
      },
    ];
    let mut list = ListState::new(0);
    list.reset(items.len());
    MainMenuModal {
      anchor,
      list,
      items,
    }
  }

  fn pick(&self, index: usize) -> ModalResult {
    match self.items.get(index) {
      Some(item) => ModalResult::Run(item.action.clone()),
      None => ModalResult::Close,
    }
  }

  fn key_of(&self, keymap: &Keymap, item: &Item) -> Option<String> {
    keymap
      .key(KeymapGroup::Tasks, &item.action)
      .map(|key| key.to_string())
  }
}

/// Space around the label and between it and the key.
const PAD: u16 = 2;
const GAP: u16 = 3;

impl Modal for MainMenuModal {
  fn handle_key(&mut self, key: &Key) -> ModalResult {
    match (key.code, key.mods) {
      (KeyCode::Enter, KeyMods::NONE) => {
        return match self.list.selected() {
          Some(index) => self.pick(index),
          None => ModalResult::Close,
        };
      }
      (KeyCode::Esc, KeyMods::NONE) => return ModalResult::Close,
      (KeyCode::Char('k'), KeyMods::NONE) => self.list.prev(),
      (KeyCode::Char('j'), KeyMods::NONE) => self.list.next(),
      _ => {
        self.list.handle_key(key);
      }
    }
    ModalResult::Keep
  }

  fn handle_mouse(&mut self, mouse: &MouseEvent) -> ModalResult {
    match self.list.handle_mouse(mouse) {
      Some(index) => self.pick(index),
      None => ModalResult::Keep,
    }
  }

  fn size(&self, _frame: Rect) -> (u16, u16) {
    let label_w = self
      .items
      .iter()
      .map(|item| item.label.width())
      .max()
      .unwrap_or(0) as u16;
    // Border, padding, label, gap, the widest key, padding, border.
    (
      2 + PAD + label_w + GAP + 7 + PAD,
      self.items.len() as u16 + 2,
    )
  }

  /// The badge stays lit while its menu is open.
  fn unshaded(&self) -> Option<Rect> {
    Some(self.anchor)
  }

  /// Hangs under the badge instead of floating in the middle.
  fn area(&self, frame: Rect) -> Rect {
    let (width, height) = self.size(frame);
    let width = width.min(frame.width);
    let height = height.min(frame.height);
    let x = self.anchor.x.min(frame.right().saturating_sub(width));
    let y = self
      .anchor
      .bottom()
      .min(frame.bottom().saturating_sub(height));
    Rect::new(x, y, width, height)
  }

  fn render(&mut self, grid: &mut Grid, keymap: &Keymap, theme: &Theme) {
    let area = self.area(grid.area());
    let style = theme.block.modal();
    let inner = style.draw(grid, area).inner();
    for row in self.list.rows(inner) {
      let item = &self.items[row.index];
      let style = theme.block.modal_row(style, row.selected);
      if row.selected {
        grid.fill_area(row.area, ' ', style.text);
      }
      let mut rest = row.area.inner((0, PAD));
      grid.draw_text(
        rest.take_left(item.label.width() as u16),
        item.label,
        style.text,
      );
      if let Some(key) = self.key_of(keymap, item) {
        let key_area =
          rest.take_right(key.width().min(rest.width as usize) as u16);
        grid.draw_text(key_area, &key, style.key);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::term::{
    Size,
    mouse::{MouseButton, MouseEventKind},
  };

  fn menu() -> MainMenuModal {
    MainMenuModal::new(Rect::new(0, 0, 9, 1))
  }

  fn frame() -> Rect {
    Rect::new(0, 0, 80, 24)
  }

  fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
      kind,
      x: x as i32,
      y: y as i32,
      mods: KeyMods::NONE,
    }
  }

  #[test]
  fn hangs_under_the_badge() {
    let area = menu().area(frame());
    assert_eq!((area.x, area.y), (0, 1));
    assert_eq!(area.height, 6, "four items inside a border");

    let near_the_edge = MainMenuModal::new(Rect::new(70, 0, 9, 1));
    let area = near_the_edge.area(frame());
    assert_eq!(area.right(), 80, "pulled back inside the frame");
  }

  #[test]
  fn the_mouse_moves_the_selection_and_clicks_pick() {
    let mut menu = menu();
    let mut grid = Grid::new(
      Size {
        width: 80,
        height: 24,
      },
      0,
    );
    menu.render(&mut grid, &Keymap::new(), &Theme::dark());

    // Rows start under the top border.
    assert!(matches!(
      menu.handle_mouse(&mouse(MouseEventKind::Moved, 3, 3)),
      ModalResult::Keep
    ));
    assert_eq!(menu.list.selected(), Some(1));
    assert!(matches!(
      menu.handle_key(&Key::new(KeyCode::Enter, KeyMods::NONE)),
      ModalResult::Run(Action::ShowRunnerSelect {
        tab: RunnerTab::Open
      })
    ));

    assert!(matches!(
      menu.handle_mouse(&mouse(MouseEventKind::Down(MouseButton::Left), 3, 5)),
      ModalResult::Run(Action::ShowQuit)
    ));
    // Outside the rows nothing happens; the app closes it on a click
    // outside the menu.
    assert!(matches!(
      menu.handle_mouse(&mouse(
        MouseEventKind::Down(MouseButton::Left),
        60,
        20
      )),
      ModalResult::Keep
    ));
  }
}
