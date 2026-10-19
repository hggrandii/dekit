//! Why the config does not load: the file, and what is wrong in it, in
//! a dialog of its own.

use std::path::Path;

use crate::config::config::InFile;
use crate::project::relative_path;

use crate::console::{
  keymap::Keymap,
  theme::Theme,
  widgets::{border::draw_hints, wrap::wrap},
};
use crate::term::{
  Grid,
  grid::Rect,
  key::{Key, KeyCode, KeyMods},
};

use super::modal::{Modal, ModalResult};

const WIDTH: u16 = 76;

pub struct ConfigErrorModal {
  /// The file the error is in, from the project root.
  file: Option<String>,
  error: String,
}

impl ConfigErrorModal {
  pub fn new(error: &anyhow::Error, root: &Path) -> Self {
    match error.downcast_ref::<InFile>() {
      Some(in_file) => ConfigErrorModal {
        file: Some(relative_path(&in_file.file, root)),
        error: format!("{:#}", in_file.error),
      },
      None => ConfigErrorModal {
        file: None,
        error: format!("{error:#}"),
      },
    }
  }

  /// What the modal says, as lines `width` wide: the file, the error,
  /// and what happens meanwhile.
  fn lines(&self, width: u16) -> Vec<(String, Line)> {
    let mut lines = Vec::new();
    if let Some(file) = &self.file {
      lines.push((file.clone(), Line::File));
      lines.push((String::new(), Line::Note));
    }
    for line in wrap(&self.error, width) {
      lines.push((line, Line::Error));
    }
    lines.push((String::new(), Line::Note));
    let note = "The tasks keep running as they are; dekit reads the \
                config again when it is saved.";
    for line in wrap(note, width) {
      lines.push((line, Line::Note));
    }
    lines
  }
}

#[derive(Clone, Copy)]
enum Line {
  File,
  Error,
  Note,
}

impl Modal for ConfigErrorModal {
  fn handle_key(&mut self, key: &Key) -> ModalResult {
    match (key.code, key.mods) {
      (KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q'), KeyMods::NONE) => {
        ModalResult::Close
      }
      _ => ModalResult::Keep,
    }
  }

  fn size(&self, frame: Rect) -> (u16, u16) {
    let width = WIDTH.min(frame.width);
    let lines = self.lines(width.saturating_sub(4)).len() as u16;
    (width, (lines + 2).min(frame.height))
  }

  fn render(&mut self, grid: &mut Grid, _keymap: &Keymap, theme: &Theme) {
    let area = self.area(grid.area());
    let style = theme.block.modal();
    let inner = style
      .draw(grid, area)
      .title(" Config error ", style.title)
      .inner()
      .inner((0, 1));
    for (row, (text, line)) in inner.rows().zip(self.lines(inner.width)) {
      let attrs = match line {
        Line::File => style.text.set_bold(true),
        Line::Error => style.error,
        Line::Note => style.dim,
      };
      grid.draw_text(row, &text, attrs);
    }
    draw_hints(grid, area, &[("<Esc>", "close")], &style);
  }
}

#[cfg(test)]
mod tests {
  use unicode_width::UnicodeWidthStr;

  use super::*;

  #[test]
  fn shows_the_file_from_the_root_and_wraps_the_error() {
    let error = anyhow::Error::new(InFile {
      file: "/proj/dekit.yaml".into(),
      error: anyhow::anyhow!("unknown field 'title' at <config>.tasks.api"),
    })
    .context("reloading");
    let modal = ConfigErrorModal::new(&error, Path::new("/proj"));
    assert_eq!(modal.file.as_deref(), Some("dekit.yaml"));
    assert_eq!(modal.error, "unknown field 'title' at <config>.tasks.api");
    let lines: Vec<String> =
      modal.lines(20).into_iter().map(|(text, _)| text).collect();
    assert_eq!(
      lines[..5],
      [
        "dekit.yaml",
        "",
        "unknown field",
        "'title' at",
        "<config>.tasks.api",
      ]
    );
    assert!(lines.iter().all(|line| line.width() <= 20));
    // An error with no file is shown as it is.
    let modal =
      ConfigErrorModal::new(&anyhow::anyhow!("no project"), Path::new("/proj"));
    assert_eq!(modal.file, None);
  }
}
