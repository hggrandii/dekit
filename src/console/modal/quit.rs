use unicode_width::UnicodeWidthStr;

use crate::console::action::{Action, RunnerTab};
use crate::console::keymap::Keymap;
use crate::console::theme::Theme;
use crate::console::widgets::list::ListState;
use crate::term::{
  Grid,
  grid::Rect,
  key::{Key, KeyCode, KeyMods},
  mouse::MouseEvent,
};

use super::modal::{Modal, ModalResult};

/// How to leave: each way has a letter, and the list can be walked too.
pub struct QuitModal {
  list: ListState,
}

struct Item {
  key: char,
  label: &'static str,
  note: &'static str,
  result: ModalResult,
}

const ITEMS: [Item; 4] = [
  Item {
    key: 'x',
    label: "Quit",
    note: "stop everything, don't save",
    result: ModalResult::Run(Action::Quit),
  },
  Item {
    key: 's',
    label: "Save and quit",
    note: "stop everything, back on the next start",
    result: ModalResult::Run(Action::Down),
  },
  Item {
    key: 'd',
    label: "Detach",
    note: "leave everything running",
    result: ModalResult::Detach,
  },
  Item {
    key: 'r',
    label: "Switch project",
    note: "go to another runner",
    result: ModalResult::Run(Action::ShowRunnerSelect {
      tab: RunnerTab::Switch,
    }),
  },
];

/// Space inside the border; between the key, the label, and the note.
const PAD: u16 = 2;
const LABEL_W: u16 = 16;

impl Default for QuitModal {
  fn default() -> Self {
    let mut list = ListState::new(0);
    list.reset(ITEMS.len());
    QuitModal { list }
  }
}

fn pick(index: usize) -> ModalResult {
  ITEMS
    .get(index)
    .map_or(ModalResult::Close, |item| item.result.clone())
}

impl Modal for QuitModal {
  fn handle_key(&mut self, key: &Key) -> ModalResult {
    match (key.code, key.mods) {
      (KeyCode::Enter, KeyMods::NONE) => {
        return self.list.selected().map_or(ModalResult::Close, pick);
      }
      (KeyCode::Esc | KeyCode::Char('q'), KeyMods::NONE) => {
        return ModalResult::Close;
      }
      (KeyCode::Char('k'), KeyMods::NONE) => self.list.prev(),
      (KeyCode::Char('j'), KeyMods::NONE) => self.list.next(),
      (KeyCode::Char(c), KeyMods::NONE) => {
        if let Some(item) = ITEMS.iter().find(|item| item.key == c) {
          return item.result.clone();
        }
      }
      _ => {
        self.list.handle_key(key);
      }
    }
    ModalResult::Keep
  }

  fn handle_mouse(&mut self, mouse: &MouseEvent) -> ModalResult {
    self
      .list
      .handle_mouse(mouse)
      .map_or(ModalResult::Keep, pick)
  }

  fn size(&self, _frame: Rect) -> (u16, u16) {
    let note_w = ITEMS
      .iter()
      .map(|item| item.note.width())
      .max()
      .unwrap_or(0);
    // Border, padding, key, padding, label, padding, note, padding, border.
    (
      2 + PAD + 1 + PAD + LABEL_W + PAD + note_w as u16 + PAD,
      ITEMS.len() as u16 + 2,
    )
  }

  fn render(&mut self, grid: &mut Grid, _keymap: &Keymap, theme: &Theme) {
    let area = self.area(grid.area());
    let style = theme.block.modal();
    let inner = style.draw(grid, area).title(" Quit ", style.title).inner();
    for row in self.list.rows(inner) {
      let item = &ITEMS[row.index];
      let mut style = theme.block.modal_row(style, row.selected);
      if row.selected {
        grid.fill_area(row.area, ' ', style.text);
      }
      let mut rest = row.area.inner((0, PAD));
      grid.draw_text(
        rest.take_left(1),
        item.key.encode_utf8(&mut [0; 4]),
        style.key,
      );
      rest.take_left(PAD);
      grid.draw_text(
        rest.take_left(LABEL_W),
        item.label,
        style.text.set_bold(row.selected),
      );
      rest.take_left(PAD);
      grid.draw_text(rest, item.note, style.dim);
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

  fn key(code: KeyCode) -> Key {
    Key::new(code, KeyMods::NONE)
  }

  #[test]
  fn letters_pick_and_the_list_walks() {
    let mut modal = QuitModal::default();
    assert!(matches!(
      modal.handle_key(&key(KeyCode::Char('x'))),
      ModalResult::Run(Action::Quit)
    ));
    assert!(matches!(
      modal.handle_key(&key(KeyCode::Char('s'))),
      ModalResult::Run(Action::Down)
    ));
    assert!(matches!(
      modal.handle_key(&key(KeyCode::Char('d'))),
      ModalResult::Detach
    ));
    assert!(matches!(
      modal.handle_key(&key(KeyCode::Char('r'))),
      ModalResult::Run(Action::ShowRunnerSelect {
        tab: RunnerTab::Switch
      })
    ));
    assert!(matches!(
      modal.handle_key(&key(KeyCode::Char('z'))),
      ModalResult::Keep
    ));
    assert!(matches!(
      modal.handle_key(&key(KeyCode::Char('q'))),
      ModalResult::Close
    ));

    modal.handle_key(&key(KeyCode::Char('j')));
    modal.handle_key(&Key::new(KeyCode::Char('n'), KeyMods::CONTROL));
    assert!(matches!(
      modal.handle_key(&key(KeyCode::Enter)),
      ModalResult::Detach
    ));
  }

  #[test]
  fn the_mouse_hovers_and_clicks_rows() {
    let mut modal = QuitModal::default();
    let mut grid = Grid::new(
      Size {
        width: 80,
        height: 24,
      },
      0,
    );
    modal.render(&mut grid, &Keymap::new(), &Theme::dark());
    let inner = modal.area(grid.area()).inner(1);
    let at = |kind, row: u16| MouseEvent {
      kind,
      x: inner.x as i32 + 3,
      y: (inner.y + row) as i32,
      mods: KeyMods::NONE,
    };
    assert!(matches!(
      modal.handle_mouse(&at(MouseEventKind::Moved, 1)),
      ModalResult::Keep
    ));
    assert_eq!(modal.list.selected(), Some(1));
    assert!(matches!(
      modal.handle_mouse(&at(MouseEventKind::Down(MouseButton::Left), 3)),
      ModalResult::Run(Action::ShowRunnerSelect { .. })
    ));
  }
}
