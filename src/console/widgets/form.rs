//! A form: fields one under another, one of them focused. Tab and S-Tab,
//! or Up and Down, move the focus; a text field takes the readline keys
//! of `to_input_request`; Space toggles; Left and Right choose. A
//! dropdown opens its list on Space. A text field with completions offers them on
//! `C-n`/`C-p`, as vim's insert mode does: each press puts the next one
//! in.

use std::path::{Path, PathBuf};

use tui_input::Input;
use unicode_width::UnicodeWidthStr;

use crate::console::theme::{BlockStyle, Theme};
use crate::console::widgets::{
  is_dir,
  list::ListState,
  split_path,
  text_input::{paste_line, render_text_field, to_input_request},
};
use crate::term::{
  Grid,
  grid::{Pos, Rect},
  key::{Key, KeyCode, KeyMods},
  mouse::{MouseButton, MouseEvent, MouseEventKind},
  style::Style,
};

pub struct Form {
  pub fields: Vec<Field>,
  focus: usize,
  label_width: u16,
  /// What was drawn where, for clicks; later entries are on top.
  hits: Vec<(Rect, Spot)>,
  popup: Option<Popup>,
  /// A dropdown's open list: the field, and the cursor over its options.
  list: Option<(usize, ListState)>,
}

pub struct Field {
  /// What the owner knows it by.
  pub id: String,
  pub label: String,
  pub kind: FieldKind,
  /// Dim text after the value; after a section's title, what is in it.
  pub hint: Option<String>,
  /// Left out by its owner, as an input the choice before it takes none.
  pub hidden: bool,
  /// Drawn after the field before it, on its row and without a label.
  pub inline: bool,
  /// Why the value is refused; drawn in place of the hint.
  pub error: Option<String>,
}

pub enum FieldKind {
  Text {
    input: Input,
    /// Dim text shown while the field is empty.
    placeholder: String,
    /// The value the field stands for while what is typed leads up to it.
    suggestion: Option<String>,
    complete: Completion,
  },
  Toggle(bool),
  /// Options to choose one of: all in sight, or in a list that opens
  /// under it.
  Choice {
    options: Vec<String>,
    selected: usize,
    dropdown: bool,
  },
  /// A row that shows or hides the fields after it.
  Section {
    open: bool,
  },
}

#[derive(Clone)]
pub enum Completion {
  None,
  /// Directories, relative to this one unless typed absolute.
  Dirs(PathBuf),
  /// Any of these, for the word at the cursor.
  Words(Vec<String>),
}

struct Popup {
  field: usize,
  candidates: Vec<String>,
  index: usize,
  /// Where the word being completed starts, in chars.
  start: usize,
}

#[derive(Clone, Copy)]
enum Spot {
  /// A field's row: a click focuses it, and flips a toggle or section.
  Row(usize),
  /// A text field's value, starting at the rect's left.
  Text(usize),
  /// An option of a choice.
  Option(usize, usize),
  /// A dropdown's box: a click opens its list.
  Dropdown(usize),
}

/// What a key did to the form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormEvent {
  Submit,
  Cancel,
  /// Nothing for the owner to do.
  Keep,
}

impl Field {
  pub fn text(id: &str, label: &str, value: &str) -> Field {
    Field {
      id: id.to_string(),
      label: label.to_string(),
      kind: FieldKind::Text {
        input: Input::new(value.to_string()),
        placeholder: String::new(),
        suggestion: None,
        complete: Completion::None,
      },
      hint: None,
      hidden: false,
      inline: false,
      error: None,
    }
  }

  pub fn toggle(id: &str, label: &str, on: bool) -> Field {
    Field {
      kind: FieldKind::Toggle(on),
      ..Field::text(id, label, "")
    }
  }

  pub fn choice(
    id: &str,
    label: &str,
    options: &[&str],
    selected: usize,
  ) -> Field {
    Field {
      kind: FieldKind::Choice {
        options: options.iter().map(|o| o.to_string()).collect(),
        selected,
        dropdown: false,
      },
      ..Field::text(id, label, "")
    }
  }

  pub fn select(
    id: &str,
    label: &str,
    options: Vec<String>,
    selected: usize,
  ) -> Field {
    Field {
      kind: FieldKind::Choice {
        options,
        selected,
        dropdown: true,
      },
      ..Field::text(id, label, "")
    }
  }

  pub fn section(id: &str, label: &str, open: bool) -> Field {
    Field {
      kind: FieldKind::Section { open },
      ..Field::text(id, label, "")
    }
  }

  pub fn placeholder(mut self, text: &str) -> Field {
    self.set_placeholder(text);
    self
  }

  pub fn set_suggestion(&mut self, text: Option<String>) {
    if let FieldKind::Text { suggestion, .. } = &mut self.kind {
      *suggestion = text;
    }
  }

  pub fn set_placeholder(&mut self, text: &str) {
    if let FieldKind::Text { placeholder, .. } = &mut self.kind {
      *placeholder = text.to_string();
    }
  }

  pub fn complete(mut self, completion: Completion) -> Field {
    if let FieldKind::Text { complete, .. } = &mut self.kind {
      *complete = completion;
    }
    self
  }

  pub fn hint(mut self, hint: &str) -> Field {
    self.hint = Some(hint.to_string());
    self
  }

  pub fn inline(mut self) -> Field {
    self.inline = true;
    self
  }

  pub fn value(&self) -> &str {
    match &self.kind {
      FieldKind::Text { input, .. } => input.value(),
      FieldKind::Choice {
        options, selected, ..
      } => &options[*selected],
      FieldKind::Toggle(_) | FieldKind::Section { .. } => "",
    }
  }

  /// The option chosen, of a choice.
  pub fn selected(&self) -> usize {
    match &self.kind {
      FieldKind::Choice { selected, .. } => *selected,
      FieldKind::Text { .. }
      | FieldKind::Toggle(_)
      | FieldKind::Section { .. } => 0,
    }
  }

  /// Gives a choice other options, keeping the chosen one when it is
  /// among them and taking `fallback` otherwise.
  pub fn set_options(&mut self, new: &[&str], fallback: usize) {
    if let FieldKind::Choice {
      options, selected, ..
    } = &mut self.kind
    {
      let current = options[*selected].clone();
      *selected = new.iter().position(|o| *o == current).unwrap_or(fallback);
      *options = new.iter().map(|o| o.to_string()).collect();
    }
  }

  pub fn on(&self) -> bool {
    match &self.kind {
      FieldKind::Toggle(on) | FieldKind::Section { open: on } => *on,
      FieldKind::Text { .. } | FieldKind::Choice { .. } => false,
    }
  }
}

impl Form {
  pub fn new(fields: Vec<Field>) -> Form {
    let label_width = fields
      .iter()
      .filter(|field| {
        !field.inline && !matches!(field.kind, FieldKind::Section { .. })
      })
      .map(|field| field.label.width() as u16)
      .max()
      .unwrap_or(0);
    let mut form = Form {
      fields,
      focus: 0,
      label_width,
      hits: Vec::new(),
      popup: None,
      list: None,
    };
    form.settle_focus();
    form
  }

  pub fn field(&self, id: &str) -> Option<&Field> {
    self.fields.iter().find(|field| field.id == id)
  }

  pub fn field_mut(&mut self, id: &str) -> Option<&mut Field> {
    self.fields.iter_mut().find(|field| field.id == id)
  }

  pub fn focused(&self) -> Option<&Field> {
    self.fields.get(self.focus)
  }

  /// Whether a field is drawn: not hidden, not in a closed section, and
  /// an inline field only with the one it follows.
  pub fn shown(&self, index: usize) -> bool {
    let field = &self.fields[index];
    if field.hidden {
      return false;
    }
    if matches!(field.kind, FieldKind::Section { .. }) {
      return true;
    }
    if field.inline {
      return self.fields[..index]
        .iter()
        .rposition(|f| !f.inline)
        .is_some_and(|anchor| self.shown(anchor));
    }
    self.section_of(index).is_none_or(|section| {
      matches!(self.fields[section].kind, FieldKind::Section { open: true })
    })
  }

  fn section_of(&self, index: usize) -> Option<usize> {
    self.fields[..index]
      .iter()
      .rposition(|field| matches!(field.kind, FieldKind::Section { .. }))
  }

  /// Focuses a field, opening the section it is in.
  pub fn focus(&mut self, id: &str) {
    let Some(index) = self.fields.iter().position(|field| field.id == id)
    else {
      return;
    };
    if let Some(section) = self.section_of(index) {
      self.fields[section].kind = FieldKind::Section { open: true };
    }
    self.focus = index;
    self.popup = None;
    self.list = None;
  }

  /// Adds a field right after the one with `after`'s id.
  pub fn insert_after(&mut self, after: &str, field: Field) {
    let index = self
      .fields
      .iter()
      .rposition(|f| f.id == after || f.id.starts_with(&format!("{after}.")))
      .map_or(self.fields.len(), |i| i + 1);
    self.fields.insert(index, field);
    if index <= self.focus {
      self.focus += 1;
    }
  }

  /// The rows the visible fields take.
  pub fn height(&self) -> u16 {
    (0..self.fields.len())
      .filter(|&i| !self.fields[i].inline && self.shown(i))
      .count() as u16
  }

  /// Moves the focus off a field that is no longer shown.
  fn settle_focus(&mut self) {
    if self.focus >= self.fields.len() || !self.shown(self.focus) {
      self.move_focus(1);
    }
  }

  fn move_focus(&mut self, delta: isize) {
    self.popup = None;
    self.list = None;
    let len = self.fields.len() as isize;
    if len == 0 {
      return;
    }
    let mut index = self.focus as isize;
    for _ in 0..len {
      index = (index + delta).rem_euclid(len);
      if self.shown(index as usize) {
        self.focus = index as usize;
        return;
      }
    }
  }

  pub fn handle_key(&mut self, key: &Key) -> FormEvent {
    if let Some(event) = self.list_key(key) {
      return event;
    }
    if let Some(event) = self.popup_key(key) {
      return event;
    }
    match (key.code, key.mods) {
      (KeyCode::Esc, KeyMods::NONE) => return FormEvent::Cancel,
      (KeyCode::Enter, KeyMods::NONE) => {
        if let Some(Field {
          kind: FieldKind::Section { open },
          ..
        }) = self.fields.get_mut(self.focus)
        {
          *open = !*open;
          return FormEvent::Keep;
        }
        return FormEvent::Submit;
      }
      (KeyCode::Tab, KeyMods::NONE) | (KeyCode::Down, KeyMods::NONE) => {
        self.move_focus(1);
        return FormEvent::Keep;
      }
      (KeyCode::Tab, KeyMods::SHIFT) | (KeyCode::Up, KeyMods::NONE) => {
        self.move_focus(-1);
        return FormEvent::Keep;
      }
      _ => (),
    }
    self.settle_focus();
    let focus = self.focus;
    let Some(field) = self.fields.get_mut(focus) else {
      return FormEvent::Keep;
    };
    match (&mut field.kind, key.code, key.mods) {
      (
        FieldKind::Text { .. },
        KeyCode::Char(c @ ('n' | 'p')),
        KeyMods::CONTROL,
      ) => {
        self.open_popup(c == 'n');
      }
      (FieldKind::Text { input, .. }, ..) => {
        if let Some(req) = to_input_request(key) {
          input.handle(req);
          field.error = None;
        }
      }
      (FieldKind::Toggle(on), KeyCode::Char(' '), KeyMods::NONE)
      | (FieldKind::Section { open: on }, KeyCode::Char(' '), KeyMods::NONE) => {
        *on = !*on;
      }
      (
        FieldKind::Choice {
          options,
          selected,
          dropdown,
        },
        code,
        KeyMods::NONE,
      ) => {
        let len = options.len();
        match code {
          KeyCode::Char(' ') if *dropdown => {
            self.list = Some((focus, open_list(len, *selected)));
          }
          KeyCode::Left | KeyCode::Char('h') => {
            *selected = (*selected + len - 1) % len;
            field.error = None;
          }
          KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => {
            *selected = (*selected + 1) % len;
            field.error = None;
          }
          _ => (),
        }
      }
      _ => (),
    }
    FormEvent::Keep
  }

  /// Keys while a dropdown's list is open: the list's, and `j`/`k`,
  /// move; Enter or Space takes the option, Esc closes it. Any other key
  /// closes it and goes on as usual.
  fn list_key(&mut self, key: &Key) -> Option<FormEvent> {
    let (field, list) = self.list.as_mut()?;
    let take = match (key.code, key.mods) {
      (KeyCode::Enter | KeyCode::Char(' '), KeyMods::NONE) => list.selected(),
      (KeyCode::Esc, KeyMods::NONE) => None,
      _ if list.handle_vi_key(key) => return Some(FormEvent::Keep),
      _ => {
        self.list = None;
        return None;
      }
    };
    let field = *field;
    self.list = None;
    self.choose(field, take);
    Some(FormEvent::Keep)
  }

  /// Takes an option of a choice, if any.
  fn choose(&mut self, field: usize, option: Option<usize>) {
    if let (Some(option), FieldKind::Choice { selected, .. }) =
      (option, &mut self.fields[field].kind)
    {
      *selected = option;
    }
  }

  /// Keys while completions are shown: `C-n`/`C-p` go on, `C-y` keeps
  /// the one put in, `C-e` and Esc put back what was typed.
  fn popup_key(&mut self, key: &Key) -> Option<FormEvent> {
    let popup = self.popup.as_mut()?;
    match (key.code, key.mods) {
      (KeyCode::Char('n'), KeyMods::CONTROL) => {
        popup.index = (popup.index + 1) % popup.candidates.len();
      }
      (KeyCode::Char('p'), KeyMods::CONTROL) => {
        let len = popup.candidates.len();
        popup.index = (popup.index + len - 1) % len;
      }
      (KeyCode::Char('y'), KeyMods::CONTROL) => {
        self.popup = None;
        return Some(FormEvent::Keep);
      }
      (KeyCode::Esc, KeyMods::NONE)
      | (KeyCode::Char('e'), KeyMods::CONTROL) => {
        // The first candidate is what was typed.
        popup.index = 0;
        let popup = self.popup.take()?;
        self.put_candidate(&popup);
        return Some(FormEvent::Keep);
      }
      _ => {
        self.popup = None;
        return None;
      }
    }
    let popup = self.popup.take()?;
    self.put_candidate(&popup);
    self.popup = Some(popup);
    Some(FormEvent::Keep)
  }

  fn open_popup(&mut self, forward: bool) {
    let Some(field) = self.fields.get(self.focus) else {
      return;
    };
    let FieldKind::Text {
      input, complete, ..
    } = &field.kind
    else {
      return;
    };
    let value: Vec<char> = input.value().chars().collect();
    let cursor = input.cursor().min(value.len());
    let (start, candidates) = match complete {
      Completion::None => return,
      Completion::Words(words) => {
        let start = value[..cursor]
          .iter()
          .rposition(|c| c.is_whitespace() || *c == ',')
          .map_or(0, |i| i + 1);
        let word: String = value[start..cursor].iter().collect();
        let typed = value[..start].iter().collect::<String>();
        let mut found: Vec<String> = words
          .iter()
          .filter(|w| {
            w.starts_with(&word) && !typed.split_whitespace().any(|t| t == *w)
          })
          .cloned()
          .collect();
        found.sort();
        (start, found)
      }
      Completion::Dirs(base) => {
        let typed: String = value[..cursor].iter().collect();
        (0, dir_candidates(base, &typed))
      }
    };
    if candidates.is_empty() {
      return;
    }
    let typed: String = value[start..cursor].iter().collect();
    let mut all = vec![typed];
    all.extend(candidates);
    let popup = Popup {
      field: self.focus,
      index: if forward { 1 } else { all.len() - 1 },
      candidates: all,
      start,
    };
    self.put_candidate(&popup);
    self.popup = Some(popup);
  }

  /// Puts the current candidate in place of the word being completed, as
  /// far as the cursor: what follows it stays, as in vim.
  fn put_candidate(&mut self, popup: &Popup) {
    let Some(Field {
      kind: FieldKind::Text { input, .. },
      ..
    }) = self.fields.get_mut(popup.field)
    else {
      return;
    };
    let value: Vec<char> = input.value().chars().collect();
    let cursor = input.cursor().min(value.len());
    let before: String = value[..popup.start].iter().collect();
    let after: String = value[cursor..].iter().collect();
    let candidate = &popup.candidates[popup.index];
    let new = format!("{before}{candidate}{after}");
    let cursor = before.chars().count() + candidate.chars().count();
    *input = Input::new(new).with_cursor(cursor);
  }

  /// Text pasted into the focused text field, on one line.
  pub fn paste(&mut self, text: &str) {
    self.settle_focus();
    let Some(Field {
      kind: FieldKind::Text { input, .. },
      error,
      ..
    }) = self.fields.get_mut(self.focus)
    else {
      return;
    };
    if paste_line(input, text) {
      *error = None;
    }
  }

  /// A click focuses a field; on a toggle, choice, or section it acts.
  pub fn handle_mouse(&mut self, mouse: &MouseEvent) {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
      return;
    }
    let (x, y) = (mouse.x as u16, mouse.y as u16);
    // An open list is drawn over the fields.
    let list = self.list.take();
    if let Some((field, list)) = &list
      && let Some(row) = list.row_at(x, y)
    {
      self.choose(*field, Some(row.index));
      return;
    }
    let Some(&(area, spot)) =
      self.hits.iter().rev().find(|(area, _)| area.contains(x, y))
    else {
      return;
    };
    self.popup = None;
    let index = match spot {
      Spot::Row(index)
      | Spot::Text(index)
      | Spot::Option(index, _)
      | Spot::Dropdown(index) => index,
    };
    self.focus = index;
    let field = &mut self.fields[index];
    match (&mut field.kind, spot) {
      (FieldKind::Toggle(on), _) => *on = !*on,
      (FieldKind::Section { open }, _) => *open = !*open,
      (FieldKind::Choice { selected, .. }, Spot::Option(_, option)) => {
        *selected = option;
      }
      (
        FieldKind::Choice {
          options, selected, ..
        },
        Spot::Dropdown(_),
      ) => {
        if list.is_none() {
          self.list = Some((index, open_list(options.len(), *selected)));
        }
      }
      (FieldKind::Text { input, .. }, Spot::Text(_)) => {
        let column = x.saturating_sub(area.x) as usize;
        let len = input.value().chars().count();
        *input =
          Input::new(input.value().to_string()).with_cursor(column.min(len));
      }
      _ => (),
    }
  }

  /// Draws the visible fields from the top of `area`; returns where the
  /// cursor goes, if a text field has the focus.
  pub fn render(
    &mut self,
    grid: &mut Grid,
    area: Rect,
    style: &BlockStyle,
    theme: &Theme,
  ) -> Option<Pos> {
    self.settle_focus();
    self.hits.clear();
    let mut cursor = None;
    let mut rows = area.rows();
    let error_style = style.error;
    let mut popup_at = None;
    let mut list_at = None;
    // What is left of the last row, for an inline field.
    let mut rest = Rect::default();
    for index in 0..self.fields.len() {
      if !self.shown(index) {
        continue;
      }
      let field = &self.fields[index];
      let focused = index == self.focus;
      if field.inline {
        rest.take_left(2);
      } else {
        let Some(row) = rows.next() else {
          break;
        };
        self.hits.push((row, Spot::Row(index)));
        rest = row;
        let marker = rest.take_left(1);
        if focused {
          grid.draw_text(marker, "\u{258e}", style.key);
        }
        if let FieldKind::Section { open } = field.kind {
          render_section(grid, rest, field, open, focused, style, theme);
          continue;
        }
        let label = rest.take_left(self.label_width);
        let label_style = if field.error.is_some() {
          error_style
        } else if focused {
          style.key
        } else {
          style.dim
        };
        let pad = self.label_width as usize - field.label.width();
        grid.draw_text(
          label,
          &format!("{}{}", " ".repeat(pad), field.label),
          label_style,
        );
        rest.take_left(2);
      }
      let line = if field.error.is_some() {
        theme.block.error
      } else if focused {
        theme.block.border_focused
      } else {
        theme.block.field_line
      };
      match &field.kind {
        FieldKind::Text {
          input,
          placeholder,
          suggestion,
          ..
        } => {
          let hint_w = field
            .error
            .as_ref()
            .or(field.hint.as_ref())
            .map_or(0, |hint| hint.width() as u16 + 2)
            .min(rest.width / 2);
          let value_area = rest.take_left(rest.width.saturating_sub(hint_w));
          let pos = render_text_field(
            input,
            value_area,
            grid,
            placeholder,
            suggestion.as_deref(),
            style,
            line,
          );
          self.hits.push((value_area, Spot::Text(index)));
          if focused {
            cursor = Some(pos);
          }
          if self.popup.as_ref().is_some_and(|p| p.field == index) {
            popup_at = Some(Rect {
              y: value_area.y + 1,
              ..value_area
            });
          }
        }
        FieldKind::Toggle(on) => {
          let text = if *on { "[x]" } else { "[ ]" };
          let at = grid.draw_text(rest, text, style.text.set_bold(focused));
          rest.take_left(at.width + 2);
        }
        FieldKind::Choice {
          options,
          selected,
          dropdown: false,
        } => {
          for (i, option) in options.iter().enumerate() {
            let chosen = i == *selected;
            let (mark, attrs) = if chosen {
              ("(\u{2022}) ", style.text)
            } else {
              ("( ) ", style.dim)
            };
            let mark_attrs = if chosen && focused { style.key } else { attrs };
            let at = rest.take_left((mark.width() + option.width()) as u16);
            grid.draw_text(at, mark, mark_attrs);
            grid.draw_text(at.inner((0, 0, 0, 4)), option, attrs);
            self.hits.push((at, Spot::Option(index, i)));
            rest.take_left(3);
          }
        }
        FieldKind::Choice {
          options,
          selected,
          dropdown: true,
        } => {
          let width = options.iter().map(|o| o.width()).max().unwrap_or(0);
          let at = rest.take_left(width as u16 + 2);
          let under =
            |attrs: Style| attrs.set_underlined(true).ul_color(line.into());
          grid.fill_area(at, ' ', under(style.text));
          grid.draw_text(at, &options[*selected], under(style.text));
          let arrow = Rect {
            x: at.right() - 1,
            width: 1,
            ..at
          };
          let arrow_attrs = if focused { style.key } else { style.dim };
          grid.draw_text(arrow, "\u{25bc}", under(arrow_attrs));
          self.hits.push((at, Spot::Dropdown(index)));
          if self.list.as_ref().is_some_and(|(field, _)| *field == index) {
            list_at = Some(at);
          }
        }
        FieldKind::Section { .. } => (),
      }
      if let Some(error) = &field.error {
        let mut at = rest;
        at.take_left(1);
        grid.draw_text(at, error, error_style);
      } else if let Some(hint) = &field.hint {
        let mut at = rest;
        at.take_left(1);
        grid.draw_text(at, hint, style.dim);
      }
    }
    if let (Some(popup), Some(area)) = (&self.popup, popup_at) {
      self.render_popup(grid, popup, area, style, theme);
    }
    if let Some(at) = list_at {
      self.render_list(grid, at, area, style, theme);
    }
    cursor
  }

  /// Draws a dropdown's open list under `at`, or over it when `bounds`
  /// has more room there.
  fn render_list(
    &mut self,
    grid: &mut Grid,
    at: Rect,
    bounds: Rect,
    style: &BlockStyle,
    theme: &Theme,
  ) {
    let Some((field, list)) = &mut self.list else {
      return;
    };
    let FieldKind::Choice { options, .. } = &self.fields[*field].kind else {
      return;
    };
    let len = options.len() as u16;
    let below = bounds.bottom().saturating_sub(at.y + 1);
    let above = at.y.saturating_sub(bounds.y);
    let area = if below >= len || below >= above {
      Rect::new(at.x, at.y + 1, at.width, below.min(len))
    } else {
      let rows = above.min(len);
      Rect::new(at.x, at.y - rows, at.width, rows)
    };
    for row in list.rows(area) {
      let option = &options[row.index];
      option_row(grid, row.area, option, row.selected, style, theme);
    }
  }

  fn render_popup(
    &self,
    grid: &mut Grid,
    popup: &Popup,
    under: Rect,
    style: &BlockStyle,
    theme: &Theme,
  ) {
    const SHOWN: usize = 6;
    // The typed word is candidate 0 and not listed; the window follows
    // the one put in.
    let start = popup.index.saturating_sub(1).saturating_sub(SHOWN - 1);
    let shown: Vec<(usize, &String)> = popup
      .candidates
      .iter()
      .enumerate()
      .skip(1 + start)
      .take(SHOWN)
      .collect();
    let width = shown
      .iter()
      .map(|(_, c)| c.width() as u16 + 2)
      .max()
      .unwrap_or(0)
      .min(under.width);
    for (row, (i, candidate)) in shown.into_iter().enumerate() {
      let area = Rect::new(under.x, under.y + row as u16, width, 1);
      option_row(grid, area, candidate, i == popup.index, style, theme);
    }
  }
}

/// A list over the fields, its cursor on `selected` of `len` options.
fn open_list(len: usize, selected: usize) -> ListState {
  let mut list = ListState::new(0);
  list.reset(len);
  list.select(selected);
  list
}

/// An option of a list over the fields, lit under the cursor.
fn option_row(
  grid: &mut Grid,
  area: Rect,
  text: &str,
  lit: bool,
  style: &BlockStyle,
  theme: &Theme,
) {
  let attrs = match lit {
    true => Style {
      bg: theme.block.border_focused.into(),
      fg: theme.block.bg.into(),
      ..style.text
    },
    false => theme.block.modal_row(*style, true).text,
  };
  grid.fill_area(area, ' ', attrs);
  grid.draw_text(area.inner((0, 1)), text, attrs);
}

/// A section's row: its title with `+` or `-` for closed or open, what is
/// in it while closed, and a rule to the end.
fn render_section(
  grid: &mut Grid,
  mut row: Rect,
  field: &Field,
  open: bool,
  focused: bool,
  style: &BlockStyle,
  theme: &Theme,
) {
  let sign = if open { "\u{2212}" } else { "+" };
  let title = format!("{sign} {}", field.label);
  let attrs = if focused { style.key } else { style.text };
  let at = grid.draw_text(row, &title, attrs.set_bold(true));
  row.take_left(at.width + 1);
  if !open && let Some(hint) = &field.hint {
    let at = grid.draw_text(row.inner((0, 0, 0, 1)), hint, style.dim);
    row.take_left(at.width + 2);
  }
  let rule = Style {
    fg: theme.block.field_line.into(),
    ..style.text
  };
  grid.draw_text(row, &"\u{2500}".repeat(row.width as usize), rule);
}

/// Directories that complete what is typed: `src/c` gives `src/console/`.
fn dir_candidates(base: &Path, typed: &str) -> Vec<String> {
  let (dir, prefix) = split_path(typed);
  let expanded = match dir.strip_prefix("~/") {
    Some(rest) => crate::runner::home_dir().unwrap_or_default().join(rest),
    None if dir.starts_with('/') => PathBuf::from(dir),
    None => base.join(dir),
  };
  let Ok(entries) = std::fs::read_dir(&expanded) else {
    return Vec::new();
  };
  // Names first: a directory such as node_modules has many entries, and
  // only the ones that match are looked at.
  let mut found: Vec<String> = entries
    .filter_map(Result::ok)
    .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry)))
    .filter(|(name, _)| name.starts_with(prefix))
    .filter(|(name, _)| !name.starts_with('.') || prefix.starts_with('.'))
    .filter(|(_, entry)| is_dir(entry))
    .map(|(name, _)| format!("{dir}{name}/"))
    .collect();
  found.sort();
  found
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::term::Size;

  fn key(code: KeyCode) -> Key {
    Key::new(code, KeyMods::NONE)
  }

  fn ctrl(c: char) -> Key {
    Key::new(KeyCode::Char(c), KeyMods::CONTROL)
  }

  fn typed(form: &mut Form, text: &str) {
    for c in text.chars() {
      form.handle_key(&key(KeyCode::Char(c)));
    }
  }

  #[test]
  fn tabs_skip_hidden_fields_and_sections_open() {
    let mut form = Form::new(vec![
      Field::text("name", "Name", ""),
      Field::section("more", "More", false),
      Field::text("tags", "Tags", ""),
      Field::toggle("auto", "Auto", false),
    ]);
    assert_eq!(form.focused().unwrap().id, "name");
    form.handle_key(&key(KeyCode::Tab));
    assert_eq!(form.focused().unwrap().id, "more");
    form.handle_key(&key(KeyCode::Tab));
    assert_eq!(form.focused().unwrap().id, "name", "wraps past hidden");
    form.handle_key(&Key::new(KeyCode::Tab, KeyMods::SHIFT));
    assert_eq!(form.focused().unwrap().id, "more");
    assert_eq!(form.handle_key(&key(KeyCode::Enter)), FormEvent::Keep);
    form.handle_key(&key(KeyCode::Down));
    assert_eq!(form.focused().unwrap().id, "tags");
    typed(&mut form, "a b");
    form.handle_key(&key(KeyCode::Down));
    form.handle_key(&key(KeyCode::Char(' ')));
    assert!(form.field("auto").unwrap().on());
    assert_eq!(form.field("tags").unwrap().value(), "a b");
    assert_eq!(form.handle_key(&key(KeyCode::Enter)), FormEvent::Submit);
    assert_eq!(form.handle_key(&key(KeyCode::Esc)), FormEvent::Cancel);
  }

  #[test]
  fn completes_words_one_at_a_time_as_vim_does() {
    let words = vec!["db".to_string(), "dev".to_string(), "api".to_string()];
    let mut form = Form::new(vec![
      Field::text("deps", "Deps", "").complete(Completion::Words(words)),
    ]);
    typed(&mut form, "api d");
    form.handle_key(&ctrl('n'));
    assert_eq!(form.field("deps").unwrap().value(), "api db");
    form.handle_key(&ctrl('n'));
    assert_eq!(form.field("deps").unwrap().value(), "api dev");
    form.handle_key(&ctrl('n'));
    assert_eq!(form.field("deps").unwrap().value(), "api d", "typed again");
    form.handle_key(&ctrl('p'));
    form.handle_key(&ctrl('y'));
    assert_eq!(form.field("deps").unwrap().value(), "api dev");
    typed(&mut form, " ");
    form.handle_key(&ctrl('n'));
    assert_eq!(
      form.field("deps").unwrap().value(),
      "api dev db",
      "what is there already is not offered"
    );
    form.handle_key(&key(KeyCode::Esc));
    assert_eq!(form.field("deps").unwrap().value(), "api dev ");

    // Inside a word, what follows the cursor stays.
    typed(&mut form, "dx");
    form.handle_key(&key(KeyCode::Left));
    form.handle_key(&ctrl('n'));
    assert_eq!(form.field("deps").unwrap().value(), "api dev dbx");
    form.handle_key(&key(KeyCode::Esc));
    assert_eq!(form.field("deps").unwrap().value(), "api dev dx");
  }

  #[test]
  fn clicks_focus_toggle_and_choose() {
    let mut form = Form::new(vec![
      Field::text("name", "Name", "x"),
      Field::choice("type", "Type", &["service", "job"], 0),
      Field::toggle("start", "Start", true),
    ]);
    let mut grid = Grid::new(
      Size {
        width: 40,
        height: 5,
      },
      0,
    );
    let theme = Theme::dark();
    let style = theme.block.modal();
    form.render(&mut grid, Rect::new(0, 0, 40, 3), &style, &theme);
    let click = |x: u16, y: u16| MouseEvent {
      kind: MouseEventKind::Down(MouseButton::Left),
      x: x as i32,
      y: y as i32,
      mods: KeyMods::NONE,
    };
    // Marker, label (5), two blanks: values start at 8; "(•) service",
    // then three blanks.
    form.handle_mouse(&click(8 + 14 + 1, 1));
    assert_eq!(form.field("type").unwrap().value(), "job");
    form.handle_mouse(&click(9, 2));
    assert!(!form.field("start").unwrap().on());
    assert_eq!(form.focused().unwrap().id, "start");
  }

  fn select_form() -> Form {
    Form::new(vec![
      Field::select(
        "stop",
        "Stop",
        vec!["SIGTERM".into(), "keys".into(), "command".into()],
        0,
      ),
      Field::text("stop.value", "", "").inline(),
      Field::text("timeout", "Timeout", ""),
    ])
  }

  #[test]
  fn a_select_steps_and_chooses_from_its_list() {
    let mut form = select_form();
    form.handle_key(&key(KeyCode::Right));
    assert_eq!(form.field("stop").unwrap().value(), "keys");
    form.handle_key(&key(KeyCode::Left));
    form.handle_key(&key(KeyCode::Char(' ')));
    form.handle_key(&key(KeyCode::Char('j')));
    form.handle_key(&key(KeyCode::Char('j')));
    assert_eq!(form.field("stop").unwrap().value(), "SIGTERM", "not yet");
    assert_eq!(form.handle_key(&key(KeyCode::Enter)), FormEvent::Keep);
    assert_eq!(form.field("stop").unwrap().value(), "command");
    // Esc closes the list and keeps what was chosen.
    form.handle_key(&key(KeyCode::Char(' ')));
    form.handle_key(&key(KeyCode::Char('k')));
    assert_eq!(form.handle_key(&key(KeyCode::Esc)), FormEvent::Keep);
    assert_eq!(form.field("stop").unwrap().value(), "command");
    assert_eq!(form.handle_key(&key(KeyCode::Enter)), FormEvent::Submit);
  }

  #[test]
  fn an_inline_field_shares_a_row_and_hides_with_it() {
    let mut form = select_form();
    assert_eq!(form.height(), 2);
    form.handle_key(&key(KeyCode::Tab));
    assert_eq!(form.focused().unwrap().id, "stop.value");
    form.field_mut("stop.value").unwrap().hidden = true;
    form.handle_key(&key(KeyCode::Tab));
    assert_eq!(form.focused().unwrap().id, "timeout", "off a hidden field");
    form.handle_key(&key(KeyCode::Tab));
    form.handle_key(&key(KeyCode::Tab));
    assert_eq!(form.focused().unwrap().id, "timeout", "skips the hidden");
    form.field_mut("stop.value").unwrap().hidden = false;
    form.field_mut("stop").unwrap().hidden = true;
    assert!(!form.shown(1), "an inline field goes with its row");
    assert_eq!(form.height(), 1);
  }

  #[test]
  fn a_closed_section_says_what_is_in_it() {
    let mut form = Form::new(vec![
      Field::text("name", "Name", ""),
      Field::section("more", "More", false).hint("env"),
      Field::text("env", "Env", "A=1"),
    ]);
    let mut grid = Grid::new(
      Size {
        width: 30,
        height: 2,
      },
      0,
    );
    let theme = Theme::dark();
    form.render(
      &mut grid,
      Rect::new(0, 0, 30, 2),
      &theme.block.modal(),
      &theme,
    );
    let row: String = (0..30)
      .map(|col| {
        let cell = *grid.cell_mut(Pos { col, row: 1 }).unwrap();
        let mut buf = [0; 4];
        match grid.text_of(cell, &mut buf) {
          "" => " ".to_string(),
          text => text.to_string(),
        }
      })
      .collect();
    assert_eq!(
      row,
      " + More  env \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}"
    );
    assert_eq!(form.height(), 2);
  }
}
