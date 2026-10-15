use super::common::Geometry;
use super::key::Key;
use super::mouse::MouseEvent;

/// Input from a terminal: what the attach client reads from the user's
/// terminal and sends to the runner, what the console hands the screens
/// it shows. Its wire forms live in `protocol::ctl`, not here, so a
/// shape here can change freely.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TermEvent {
  FocusGained,
  FocusLost,
  Key(Key),
  Mouse(MouseEvent),
  Paste(String),
  /// The terminal's new geometry.
  Resize(Geometry),
}

impl TermEvent {
  /// A resize of a terminal whose cell size is unknown.
  pub fn resize(width: u16, height: u16) -> TermEvent {
    TermEvent::Resize(Geometry::cells(width, height))
  }
}
