//! Helpers for the unit tests.

use std::path::PathBuf;

/// A fresh dir under the system temp dir; removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
  pub fn new(name: &str) -> Self {
    let dir =
      std::env::temp_dir().join(format!("dekit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
  }
}

impl Drop for TempDir {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}
