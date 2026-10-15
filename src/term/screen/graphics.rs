//! The kitty graphics protocol as it acts on a screen: transmissions
//! into the image store, placements on the grid, deletions, replies.

use std::sync::Arc;

use bytes::Bytes;

use super::{Screen, VtEvent};
use crate::term::{
  grid::Pos,
  image::{
    kitty::{Command, Pending, decode_payload, read_medium},
    placement::{Placement, SrcRect, cell_size},
    png,
    store::{AnimationCommand, Format, Image, ImageData, MAX_IMAGE_BYTES},
  },
  vt::emit,
};

impl Screen {
  /// `ESC _ G body ESC \`: `body` is everything after the `G`.
  pub(crate) fn graphics_command(&mut self, body: &[u8]) {
    let (control, payload) = match body.iter().position(|b| *b == b';') {
      Some(i) => (&body[..i], &body[i + 1..]),
      None => (body, &b""[..]),
    };
    // A continuation chunk carries only `m` (and maybe `q`).
    if let Some(pending) = &mut self.pending_image {
      let more = control.split(|b| *b == b',').any(|pair| pair == b"m=1");
      decode_payload(payload, &mut pending.data);
      if pending.data.len() > MAX_IMAGE_BYTES {
        let pending = self.pending_image.take().expect("pending");
        self.graphics_reply(&pending.command, "EFBIG:image too large");
        return;
      }
      if more {
        return;
      }
      let pending = self.pending_image.take().expect("pending");
      self.finish_transmission(pending.command, pending.data);
      return;
    }
    let command = match Command::parse(control) {
      Ok(command) => command,
      Err(error) => {
        // Nothing to address the error to; kitty answers anyway.
        let mut reply = Vec::new();
        emit::kitty_graphics(&mut reply, "", error.as_bytes());
        self.events.push(VtEvent::Reply(reply));
        return;
      }
    };
    match command.action {
      b't' | b'T' | b'q' | b'f' => {
        let mut data = Vec::new();
        decode_payload(payload, &mut data);
        if command.more && command.medium == b'd' {
          self.pending_image = Some(Pending { command, data });
          return;
        }
        self.finish_transmission(command, data);
      }
      b'p' => self.graphics_put(command),
      b'd' => self.graphics_delete(command),
      b'a' | b'c' => self.graphics_control(command),
      _ => self.graphics_reply(&command, "EINVAL:unknown action"),
    }
  }

  fn graphics_reply(&mut self, command: &Command, status: &str) {
    if command.quiet >= 2 || (command.quiet == 1 && status == "OK") {
      return;
    }
    let mut keys = Vec::new();
    if command.id != 0 {
      keys.push(format!("i={}", command.id));
    }
    if command.number != 0 {
      keys.push(format!("I={}", command.number));
    }
    if command.placement != 0 {
      keys.push(format!("p={}", command.placement));
    }
    let mut reply = Vec::new();
    emit::kitty_graphics(&mut reply, &keys.join(","), status.as_bytes());
    self.events.push(VtEvent::Reply(reply));
  }

  /// A whole transmission (direct data joined, or a file read): checks
  /// it, stores it, and shows it when asked.
  fn finish_transmission(&mut self, mut command: Command, data: Vec<u8>) {
    let data = match command.medium {
      b'd' => data,
      b'f' | b't' | b's' => match read_medium(&command, &data) {
        Ok(data) => data,
        Err(error) => {
          self.graphics_reply(&command, error);
          return;
        }
      },
      _ => {
        self.graphics_reply(&command, "EINVAL:unknown transmission medium");
        return;
      }
    };
    let Some(format) = Format::from_code(command.format) else {
      self.graphics_reply(&command, "EINVAL:unknown format");
      return;
    };
    let (width, height) = match format {
      Format::Png => match png::dimensions(&data) {
        Some(size) => size,
        None => {
          self.graphics_reply(&command, "EINVAL:not a PNG");
          return;
        }
      },
      Format::Rgb | Format::Rgba => {
        if command.width == 0 || command.height == 0 {
          self.graphics_reply(&command, "EINVAL:missing image size");
          return;
        }
        if !command.zlib {
          let expected = command.width as usize
            * command.height as usize
            * format.bytes_per_pixel().unwrap_or(4);
          if data.len() != expected {
            self.graphics_reply(&command, "EINVAL:data size does not match");
            return;
          }
        }
        (command.width, command.height)
      }
    };
    if data.len() > MAX_IMAGE_BYTES {
      self.graphics_reply(&command, "EFBIG:image too large");
      return;
    }
    let image_data = ImageData {
      format,
      zlib: command.zlib,
      width,
      height,
      bytes: Bytes::from(data),
    };

    match command.action {
      b'q' => self.graphics_reply(&command, "OK"),
      b'f' => {
        let frame = crate::term::image::store::AnimationFrame {
          data: image_data,
          control: command.extra.clone(),
        };
        let result = self
          .images
          .append(command.id, AnimationCommand::Frame(frame));
        self.graphics_reply(&command, result.err().unwrap_or("OK"));
      }
      _ => {
        if command.id == 0 && command.number != 0 {
          command.id = self.images.allocate_id();
        }
        if command.id == 0 {
          // Kitty allows this: an image with no id can only be shown
          // by this very command.
          command.id = self.images.allocate_id();
          let image = self.images.insert(command.id, 0, image_data);
          if command.action == b'T' {
            self.place_image(&image, &command, false);
          }
          command.id = 0;
          self.graphics_reply(&command, "OK");
          return;
        }
        let image = self.images.insert(command.id, command.number, image_data);
        if command.action == b'T' {
          self.place_image(&image, &command, false);
        }
        self.graphics_reply(&command, "OK");
      }
    }
  }

  fn graphics_put(&mut self, command: Command) {
    let image = if command.id != 0 {
      self.images.get(command.id)
    } else if command.number != 0 {
      self.images.get_by_number(command.number)
    } else {
      None
    };
    match image {
      Some(image) => {
        let mut command = command;
        command.id = image.id;
        self.place_image(&image, &command, false);
        self.graphics_reply(&command, "OK");
      }
      None => self.graphics_reply(&command, "ENOENT:no such image"),
    }
  }

  /// Puts an image at the cursor (or as a virtual placement), scrolling
  /// to make room as kitty does.
  pub(crate) fn place_image(
    &mut self,
    image: &Arc<Image>,
    command: &Command,
    cells: bool,
  ) {
    let src = SrcRect {
      x: command.x,
      y: command.y,
      w: command.w,
      h: command.h,
    }
    .resolve(image.data.width, image.data.height);
    let (cols, rows) =
      cell_size(src, command.cols, command.rows, self.cell_pixels);
    let placement = Placement {
      key: image.key,
      id: command.placement,
      row: 0,
      col: 0,
      cols,
      rows,
      src,
      offset_x: command.off_x.min(u16::MAX as u32) as u16,
      offset_y: command.off_y.min(u16::MAX as u32) as u16,
      z: command.z,
      virtual_: command.unicode,
      cells,
    };
    if command.unicode {
      // Replaces a virtual placement with the same id.
      self.grid_mut().retain_placements(|p| {
        !(p.virtual_ && p.key == image.key && p.id == command.placement)
      });
      self.grid_mut().add_placement(placement);
      return;
    }
    let style = self.style_id();
    let grid = self.grid_mut();
    let size = grid.size();
    let pos = grid.pos();
    let mut row = pos.row;
    let col = pos.col.min(size.width - 1);
    // Scroll so the image fits below the cursor, as far as it can.
    let need =
      (u32::from(row) + u32::from(rows)).saturating_sub(u32::from(size.height));
    let scroll = need.min(u32::from(row)) as u16;
    if scroll > 0 {
      grid.scroll_up(scroll, style);
      row -= scroll;
    }
    // A placement with the same id on this image is replaced.
    let key = image.key;
    let id = command.placement;
    if id != 0 {
      grid.retain_placements(|p| !(p.key == key && p.id == id && !p.virtual_));
    }
    grid.add_placement(Placement {
      row: grid.row_base() + u64::from(row),
      col,
      ..placement
    });
    if !command.cursor_stay {
      let last_row = (u32::from(row) + u32::from(rows) - 1)
        .min(u32::from(size.height) - 1) as u16;
      let next_col = col.saturating_add(cols);
      grid.set_pos(Pos {
        row: last_row,
        col: next_col.min(size.width - 1),
      });
      if next_col >= size.width {
        grid.col_inc(1);
      }
    }
  }

  fn graphics_delete(&mut self, command: Command) {
    let free = command.delete.is_ascii_uppercase();
    let what = command.delete.to_ascii_lowercase();
    let cursor = self.grid().pos();
    let view_row_base = self.grid().row_base();
    let cell = |p: &Placement, col: u16, row: u16| -> bool {
      let row = view_row_base + u64::from(row);
      p.row <= row
        && row < p.row + u64::from(p.rows)
        && p.col <= col
        && col < p.col.saturating_add(p.cols)
    };
    let image_key =
      |screen: &Screen, id: u32| screen.images.get(id).map(|i| i.key);
    let freed: Vec<crate::term::image::ImageKey> = match what {
      b'a' | 0 => self.grid_mut().remove_placements(|_| true),
      b'i' | b'n' => {
        let key = if what == b'i' {
          image_key(self, command.id)
        } else {
          self.images.get_by_number(command.number).map(|i| i.key)
        };
        let Some(key) = key else {
          return self.graphics_delete_reply(&command, what);
        };
        let pid = command.placement;
        self
          .grid_mut()
          .remove_placements(|p| p.key == key && (pid == 0 || p.id == pid));
        vec![key]
      }
      b'c' => {
        let (col, row) = (cursor.col, cursor.row);
        self
          .grid_mut()
          .remove_placements(|p| !p.virtual_ && cell(p, col, row))
      }
      b'p' | b'q' => {
        let col = command.x.saturating_sub(1).min(u16::MAX as u32) as u16;
        let row = command.y.saturating_sub(1).min(u16::MAX as u32) as u16;
        let z = command.z;
        let with_z = what == b'q';
        self.grid_mut().remove_placements(|p| {
          !p.virtual_ && cell(p, col, row) && (!with_z || p.z == z)
        })
      }
      b'r' => {
        let (lo, hi) = (command.x.min(command.y), command.x.max(command.y));
        let keys: Vec<_> =
          (lo..=hi).filter_map(|id| image_key(self, id)).collect();
        self.grid_mut().remove_placements(|p| keys.contains(&p.key));
        keys
      }
      b'x' => {
        let col = command.x.saturating_sub(1).min(u16::MAX as u32) as u16;
        self.grid_mut().remove_placements(|p| {
          !p.virtual_ && p.col <= col && col < p.col.saturating_add(p.cols)
        })
      }
      b'y' => {
        let row = view_row_base
          + u64::from(command.y.saturating_sub(1).min(u16::MAX as u32) as u16);
        self.grid_mut().remove_placements(|p| {
          !p.virtual_ && p.row <= row && row < p.row + u64::from(p.rows)
        })
      }
      b'z' => {
        let z = command.z;
        self
          .grid_mut()
          .remove_placements(|p| !p.virtual_ && p.z == z)
      }
      // Frames (`f`) and the rest: nothing to delete here.
      _ => Vec::new(),
    };
    if free {
      // Image data goes once nothing shows it anywhere.
      for key in freed {
        let placed = self.grid.placements().iter().any(|p| p.key == key)
          || self.alt_grid.placements().iter().any(|p| p.key == key);
        if !placed && let Some(image) = self.images.get_by_key(key) {
          self.images.remove(image.id);
        }
      }
    }
    self.graphics_delete_reply(&command, what);
  }

  /// Deletes are not answered unless asked.
  fn graphics_delete_reply(&mut self, command: &Command, what: u8) {
    if command.quiet == 0 && command.id != 0 && what == b'i' {
      self.graphics_reply(command, "OK");
    }
  }

  /// Animation (`a=a`) and compose (`a=c`) commands are kept with the
  /// image and replayed to terminals that animate.
  fn graphics_control(&mut self, command: Command) {
    let control = format!("a={},{}", command.action as char, command.extra);
    let result = self
      .images
      .append(command.id, AnimationCommand::Control(control));
    self.graphics_reply(&command, result.err().unwrap_or("OK"));
  }
}
