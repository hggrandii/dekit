//! Images a screen holds, as the program transmitted them.

use std::{
  collections::HashMap,
  sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
  },
};

use bytes::Bytes;

/// Identifies an image for as long as it exists, across every screen it
/// is copied into: a painter caches what it sent by this.
pub type ImageKey = u64;

fn next_key() -> ImageKey {
  static NEXT: AtomicU64 = AtomicU64::new(1);
  NEXT.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
  /// 24-bit RGB.
  Rgb,
  /// 32-bit RGBA.
  Rgba,
  Png,
}

impl Format {
  pub fn from_code(code: u32) -> Option<Format> {
    match code {
      24 => Some(Format::Rgb),
      32 => Some(Format::Rgba),
      100 => Some(Format::Png),
      _ => None,
    }
  }

  pub fn code(self) -> u32 {
    match self {
      Format::Rgb => 24,
      Format::Rgba => 32,
      Format::Png => 100,
    }
  }

  pub fn bytes_per_pixel(self) -> Option<usize> {
    match self {
      Format::Rgb => Some(3),
      Format::Rgba => Some(4),
      Format::Png => None,
    }
  }
}

/// Pixel data as transmitted: still compressed when it was.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageData {
  pub format: Format,
  pub zlib: bool,
  pub width: u32,
  pub height: u32,
  pub bytes: Bytes,
}

/// An animation frame (`a=f`), kept to be replayed to a terminal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnimationFrame {
  pub data: ImageData,
  /// The frame's control keys other than the data's, as sent.
  pub control: String,
}

/// Animation operations in arrival order: edits and compositions cannot
/// be replayed independently without changing their result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnimationCommand {
  Frame(AnimationFrame),
  Control(String),
}

impl AnimationCommand {
  fn bytes(&self) -> usize {
    std::mem::size_of::<Self>()
      + match self {
        Self::Frame(frame) => frame.data.bytes.len() + frame.control.len(),
        Self::Control(control) => control.len(),
      }
  }
}

/// An image with its frames and the animation commands applied to it.
#[derive(Clone, Debug)]
pub struct Image {
  pub key: ImageKey,
  /// The program's id (`i`), 0 when it used a number only.
  pub id: u32,
  /// The program's number (`I`), 0 when none.
  pub number: u32,
  pub data: ImageData,
  pub commands: Vec<AnimationCommand>,
}

impl Image {
  pub fn bytes(&self) -> usize {
    self.data.bytes.len()
      + self
        .commands
        .iter()
        .map(AnimationCommand::bytes)
        .sum::<usize>()
  }
}

/// Images by key, as frames carry them.
pub type ImageTable = HashMap<ImageKey, Arc<Image>>;

/// Kitty keeps 320 MiB of image data; so do we.
pub const MAX_STORE_BYTES: usize = 320 * 1024 * 1024;
/// One image larger than this is refused.
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub struct ImageStore {
  by_id: HashMap<u32, ImageKey>,
  by_number: HashMap<u32, ImageKey>,
  images: Arc<ImageTable>,
  bytes: usize,
  /// The next id to give an image transmitted by number.
  next_id: u32,
}

impl ImageStore {
  pub fn table(&self) -> Arc<ImageTable> {
    self.images.clone()
  }

  pub fn get(&self, id: u32) -> Option<Arc<Image>> {
    self
      .by_id
      .get(&id)
      .and_then(|key| self.images.get(key))
      .cloned()
  }

  /// The key of the image with id `id`.
  pub fn key_of(&self, id: u32) -> Option<ImageKey> {
    self.by_id.get(&id).copied()
  }

  pub fn get_by_number(&self, number: u32) -> Option<Arc<Image>> {
    self
      .by_number
      .get(&number)
      .and_then(|key| self.images.get(key))
      .cloned()
  }

  pub fn get_by_key(&self, key: ImageKey) -> Option<Arc<Image>> {
    self.images.get(&key).cloned()
  }

  pub fn len(&self) -> usize {
    self.images.len()
  }

  pub fn is_empty(&self) -> bool {
    self.images.is_empty()
  }

  pub fn bytes(&self) -> usize {
    self.bytes
  }

  pub fn keys(&self) -> impl Iterator<Item = ImageKey> + '_ {
    self.images.keys().copied()
  }

  /// An unused id for an image sent by number.
  pub fn allocate_id(&mut self) -> u32 {
    loop {
      // Ids above 2^31 are kitty's own range for this.
      let id = 0x8000_0000u32.wrapping_add(self.next_id);
      self.next_id = self.next_id.wrapping_add(1);
      if !self.by_id.contains_key(&id) && id != 0 {
        return id;
      }
    }
  }

  /// Stores an image under `id` (and `number`, when not 0), replacing
  /// one with the same id. Returns the image.
  pub fn insert(
    &mut self,
    id: u32,
    number: u32,
    data: ImageData,
  ) -> Arc<Image> {
    if let Some(old) = self.by_id.remove(&id) {
      self.remove_key(old);
    }
    let image = Arc::new(Image {
      key: next_key(),
      id,
      number,
      data,
      commands: Vec::new(),
    });
    self.bytes += image.bytes();
    self.by_id.insert(id, image.key);
    if number != 0 {
      self.by_number.insert(number, image.key);
    }
    Arc::make_mut(&mut self.images).insert(image.key, image.clone());
    self.enforce_quota(image.key);
    image
  }

  /// Adds an animation operation. Refuse growth beyond the quota before
  /// changing the image, including histories made only of controls.
  pub fn append(
    &mut self,
    id: u32,
    command: AnimationCommand,
  ) -> Result<(), &'static str> {
    let Some(key) = self.by_id.get(&id).copied() else {
      return Err("ENOENT:no such image");
    };
    let image = self.images.get(&key).ok_or("ENOENT:no such image")?;
    let added = command.bytes();
    if added > MAX_STORE_BYTES.saturating_sub(image.bytes()) {
      return Err("ENOSPC:animation exceeds image store quota");
    }
    let table = Arc::make_mut(&mut self.images);
    let image = table.get_mut(&key).expect("image checked above");
    Arc::make_mut(image).commands.push(command);
    self.bytes += added;
    self.enforce_quota(key);
    Ok(())
  }

  pub fn remove(&mut self, id: u32) -> Option<ImageKey> {
    let key = self.by_id.remove(&id)?;
    self.remove_key(key);
    Some(key)
  }

  fn remove_key(&mut self, key: ImageKey) {
    if let Some(image) = Arc::make_mut(&mut self.images).remove(&key) {
      self.bytes = self.bytes.saturating_sub(image.bytes());
      self.by_id.retain(|_, k| *k != key);
      self.by_number.retain(|_, k| *k != key);
    }
  }

  pub fn clear(&mut self) {
    *self = ImageStore::default();
  }

  /// Drops the oldest images while over the quota, never `keep`. Keys
  /// are given out in order, so the smallest is the oldest.
  fn enforce_quota(&mut self, keep: ImageKey) {
    while self.bytes > MAX_STORE_BYTES && self.images.len() > 1 {
      let Some(oldest) = self.images.keys().filter(|k| **k != keep).min()
      else {
        break;
      };
      self.remove_key(*oldest);
    }
  }

  /// Restores an image from a snapshot under its own ids.
  pub fn insert_restored(&mut self, image: Image) {
    self.bytes += image.bytes();
    self.by_id.insert(image.id, image.key);
    if image.number != 0 {
      self.by_number.insert(image.number, image.key);
    }
    Arc::make_mut(&mut self.images).insert(image.key, Arc::new(image));
  }
}

/// A fresh identity for an image restored or composed.
pub fn fresh_key() -> ImageKey {
  next_key()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn data() -> ImageData {
    ImageData {
      format: Format::Rgba,
      zlib: false,
      width: 512,
      height: 512,
      bytes: vec![0; 1024 * 1024].into(),
    }
  }

  #[test]
  fn animation_quota_rejects_growth_without_changing_the_image() {
    let data = data();
    let mut store = ImageStore::default();
    store.insert(1, 0, data.clone());
    let frame = AnimationCommand::Frame(AnimationFrame {
      data: data.clone(),
      control: String::new(),
    });
    // Share the pixel allocation while accounting for every transmission.
    while store.bytes() + frame.bytes() <= MAX_STORE_BYTES {
      store.append(1, frame.clone()).unwrap();
    }
    let remaining = MAX_STORE_BYTES - store.bytes();
    let mut last = data;
    last.bytes = last
      .bytes
      .slice(..remaining - std::mem::size_of::<AnimationCommand>());
    store
      .append(
        1,
        AnimationCommand::Frame(AnimationFrame {
          data: last,
          control: String::new(),
        }),
      )
      .unwrap();
    assert_eq!(store.bytes(), MAX_STORE_BYTES);
    let published = store.get(1).unwrap();
    for command in [frame, AnimationCommand::Control("a=a,s=3".into())] {
      assert_eq!(
        store.append(1, command),
        Err("ENOSPC:animation exceeds image store quota")
      );
      assert_eq!(store.bytes(), MAX_STORE_BYTES);
      assert!(Arc::ptr_eq(&published, &store.get(1).unwrap()));
    }
    assert_eq!(
      store.append(2, AnimationCommand::Control("a=a,s=3".into())),
      Err("ENOENT:no such image")
    );
  }

  #[test]
  fn animation_growth_still_evicts_older_images() {
    let data = data();
    let mut store = ImageStore::default();
    store.insert(1, 0, data.clone());
    store.insert(2, 0, data.clone());
    let frame = AnimationCommand::Frame(AnimationFrame {
      data,
      control: String::new(),
    });
    while store.get(1).is_some() {
      store.append(2, frame.clone()).unwrap();
    }
    assert_eq!(store.len(), 1);
    assert!(store.bytes() <= MAX_STORE_BYTES);
    assert_eq!(store.bytes(), store.get(2).unwrap().bytes());
  }
}
