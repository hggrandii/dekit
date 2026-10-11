use crate::console::{action::Action, keymap::Keymap, theme::Theme};
use crate::term::{Grid, grid::Rect, key::Key, mouse::MouseEvent};

#[derive(Clone)]
pub enum ModalResult {
  Keep,
  Close,
  /// Close the modal, then run the action.
  Run(Action),
  /// Close the modal and detach the attachment that pressed the key.
  Detach,
}

pub trait Modal: Send {
  fn handle_key(&mut self, key: &Key) -> ModalResult;

  /// Mouse input while the modal is open. A click outside the modal is
  /// handled by the app, which closes it. Ignored unless the modal says
  /// otherwise.
  fn handle_mouse(&mut self, _mouse: &MouseEvent) -> ModalResult {
    ModalResult::Keep
  }

  /// Width and height, given the frame the modal is centered in.
  fn size(&self, frame: Rect) -> (u16, u16);

  /// What the shade under the modal leaves alone: the control it
  /// dropped down from, if any.
  fn unshaded(&self) -> Option<Rect> {
    None
  }

  fn render(&mut self, grid: &mut Grid, keymap: &Keymap, theme: &Theme);

  fn area(&self, frame: Rect) -> Rect {
    let (w, h) = self.size(frame);
    frame.centered(w, h)
  }
}
