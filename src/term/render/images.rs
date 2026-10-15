//! Keeps a terminal's kitty images in step with the placements a frame
//! shows: transmits images once, adds and deletes placements by the
//! difference from the last paint.

use std::collections::HashMap;

use base64::Engine as _;

use crate::term::{
  frame::Frame,
  image::{
    Image, ImageKey, VisiblePlacement,
    store::{AnimationCommand, ImageData},
  },
  vt::emit,
};

/// Image ids dekit uses on the terminal start here: well above what
/// programs pick for themselves.
const FIRST_CLIENT_ID: u32 = 0x4000_0000;
/// Base64 characters per chunk, as the protocol asks.
const CHUNK: usize = 4096;

#[derive(Debug)]
struct Sent {
  id: u32,
  commands: usize,
}

pub struct ImagePainter {
  next_id: u32,
  next_placement: u32,
  sent: HashMap<ImageKey, Sent>,
  shown: Vec<(VisiblePlacement, u32)>,
  animation: bool,
}

impl ImagePainter {
  pub fn new(animation: bool) -> Self {
    ImagePainter {
      next_id: FIRST_CLIENT_ID,
      next_placement: 1,
      sent: HashMap::new(),
      shown: Vec::new(),
      animation,
    }
  }

  /// Deletes every placement on the terminal (harmless for ones it
  /// already dropped) so the next paint puts them all back; the image
  /// data stays transmitted.
  pub fn forget_placements(&mut self, out: &mut Vec<u8>) {
    for (shown, pid) in self.shown.drain(..) {
      if let Some(sent) = self.sent.get(&shown.key) {
        delete_placement(out, sent.id, pid);
      }
    }
  }

  /// Brings the terminal's images to what `frame` shows. Returns whether
  /// the cursor was moved (placements are positioned with it).
  pub fn paint(&mut self, out: &mut Vec<u8>, frame: &Frame) -> bool {
    let mut moved = false;
    // Placements that left, or changed.
    let mut kept: Vec<(VisiblePlacement, u32)> = Vec::new();
    for (shown, pid) in self.shown.drain(..) {
      if frame.placements.contains(&shown) {
        kept.push((shown, pid));
      } else if let Some(sent) = self.sent.get(&shown.key) {
        delete_placement(out, sent.id, pid);
      }
    }
    // Images that are gone.
    let gone: Vec<ImageKey> = self
      .sent
      .keys()
      .filter(|key| !frame.images.contains_key(key))
      .copied()
      .collect();
    for key in gone {
      if let Some(sent) = self.sent.remove(&key) {
        delete_image(out, sent.id);
      }
    }
    // New placements, transmitting their images first.
    for placement in &frame.placements {
      if kept.iter().any(|(shown, _)| shown == placement) {
        continue;
      }
      let Some(image) = frame.images.get(&placement.key) else {
        continue;
      };
      let id = self.ensure_sent(out, image);
      let pid = self.next_placement;
      self.next_placement += 1;
      emit::cup(out, placement.row, placement.col);
      moved = true;
      put_placement(out, id, pid, placement);
      kept.push((placement.clone(), pid));
    }
    // Frames and controls added since.
    for (key, image) in frame.images.iter() {
      if let Some(sent) = self.sent.get_mut(key) {
        send_updates(out, sent, image, self.animation);
      }
    }
    self.shown = kept;
    moved
  }

  fn ensure_sent(&mut self, out: &mut Vec<u8>, image: &Image) -> u32 {
    if let Some(sent) = self.sent.get(&image.key) {
      return sent.id;
    }
    let id = self.next_id;
    self.next_id += 1;
    transmit(out, id, image);
    let mut sent = Sent { id, commands: 0 };
    send_updates(out, &mut sent, image, self.animation);
    self.sent.insert(image.key, sent);
    id
  }

  /// Deletes everything dekit put on the terminal.
  pub fn cleanup(&mut self, out: &mut Vec<u8>) {
    for (_, sent) in self.sent.drain() {
      delete_image(out, sent.id);
    }
    self.shown.clear();
  }
}

/// The control keys that describe `data` for action `action` on image
/// `id`: format, pixel size for raw formats, compression.
fn data_control(action: char, id: u32, data: &ImageData) -> String {
  let mut control = format!("a={action},q=2,i={id},f={}", data.format.code());
  if data.format.bytes_per_pixel().is_some() {
    control.push_str(&format!(",s={},v={}", data.width, data.height));
  }
  if data.zlib {
    control.push_str(",o=z");
  }
  control
}

fn transmit(out: &mut Vec<u8>, id: u32, image: &Image) {
  chunks(out, &data_control('t', id, &image.data), &image.data.bytes);
}

fn send_updates(
  out: &mut Vec<u8>,
  sent: &mut Sent,
  image: &Image,
  animation: bool,
) {
  if !animation {
    return;
  }
  for command in &image.commands[sent.commands..] {
    let frame = match command {
      AnimationCommand::Control(control) => {
        emit::kitty_graphics(out, &format!("{control},q=2,i={}", sent.id), b"");
        continue;
      }
      AnimationCommand::Frame(frame) => frame,
    };
    let data = &frame.data;
    let mut control = data_control('f', sent.id, data);
    if !frame.control.is_empty() {
      // The frame's own keys, minus those the data already set.
      for pair in frame.control.split(',') {
        if !pair.starts_with("f=")
          && !pair.starts_with("s=")
          && !pair.starts_with("v=")
          && !pair.starts_with("o=")
        {
          control.push(',');
          control.push_str(pair);
        }
      }
    }
    chunks(out, &control, &data.bytes);
  }
  sent.commands = image.commands.len();
}

/// Writes `data` base64-encoded in chunks after `control`.
fn chunks(out: &mut Vec<u8>, control: &str, data: &[u8]) {
  let encoded = base64::engine::general_purpose::STANDARD.encode(data);
  let bytes = encoded.as_bytes();
  if bytes.is_empty() {
    emit::kitty_graphics(out, control, b"");
    return;
  }
  let mut first = true;
  let mut offset = 0;
  while offset < bytes.len() {
    let end = (offset + CHUNK).min(bytes.len());
    let more = if end < bytes.len() { 1 } else { 0 };
    let keys = if first { control } else { "q=2" };
    first = false;
    emit::kitty_graphics(out, &format!("{keys},m={more}"), &bytes[offset..end]);
    offset = end;
  }
}

fn put_placement(out: &mut Vec<u8>, id: u32, pid: u32, p: &VisiblePlacement) {
  let control = format!(
    "a=p,q=2,C=1,i={id},p={pid},x={},y={},w={},h={},c={},r={},X={},Y={},z={}",
    p.src.x,
    p.src.y,
    p.src.w,
    p.src.h,
    p.cols,
    p.rows,
    p.offset_x,
    p.offset_y,
    p.z
  );
  emit::kitty_graphics(out, &control, b"");
}

fn delete_placement(out: &mut Vec<u8>, id: u32, pid: u32) {
  emit::kitty_graphics(out, &format!("a=d,d=i,q=2,i={id},p={pid}"), b"");
}

fn delete_image(out: &mut Vec<u8>, id: u32) {
  emit::kitty_graphics(out, &format!("a=d,d=I,q=2,i={id}"), b"");
}
