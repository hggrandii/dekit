//! The projects this user opened, newest first, for the console's
//! `recent` tab. A runner records itself when it starts and whenever a
//! console attaches to it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::runner::{
  RunnerKind, RunnerSpec, atomic_write, lockfile::now_secs, user_data_dir,
  validate_root,
};

/// Entries kept, newest first.
const KEEP: usize = 50;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecentEntry {
  pub kind: RunnerKind,
  pub root: String,
  /// Unix seconds.
  pub opened_at: u64,
}

fn path() -> anyhow::Result<PathBuf> {
  Ok(user_data_dir()?.join("recent.json"))
}

/// Newest first; empty when nothing was recorded yet.
pub fn list() -> anyhow::Result<Vec<RecentEntry>> {
  list_from(&path()?)
}

/// Moves `runner` to the top. Nothing depends on this, so a failure is
/// only logged.
pub fn touch(runner: &RunnerSpec) {
  let result = path().and_then(|path| touch_at(&path, runner, now_secs()));
  if let Err(err) = result {
    log::warn!("Cannot record the recent project: {err:#}");
  }
}

fn list_from(path: &Path) -> anyhow::Result<Vec<RecentEntry>> {
  let data = match std::fs::read_to_string(path) {
    Ok(data) => data,
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
      return Ok(Vec::new());
    }
    Err(err) => return Err(err.into()),
  };
  let mut entries: Vec<RecentEntry> = serde_json::from_str(&data)?;
  entries.sort_by_key(|entry| std::cmp::Reverse(entry.opened_at));
  Ok(entries)
}

fn touch_at(
  path: &Path,
  runner: &RunnerSpec,
  opened_at: u64,
) -> anyhow::Result<()> {
  validate_root(&runner.root)?;
  let root = runner.root.to_str().expect("validated runner root");
  // A corrupt file is replaced rather than kept forever.
  let mut entries = list_from(path).unwrap_or_default();
  entries.retain(|entry| entry.kind != runner.kind || entry.root != root);
  entries.insert(
    0,
    RecentEntry {
      kind: runner.kind.clone(),
      root: root.to_string(),
      opened_at,
    },
  );
  entries.truncate(KEEP);
  if let Some(dir) = path.parent() {
    std::fs::create_dir_all(dir)?;
  }
  atomic_write(path, &serde_json::to_vec_pretty(&entries)?)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_util::TempDir;

  fn project(root: &str) -> RunnerSpec {
    RunnerSpec {
      kind: RunnerKind::Project,
      root: PathBuf::from(root),
    }
  }

  #[test]
  fn newest_first_and_without_duplicates() {
    let dir = TempDir::new("recent-order");
    let path = dir.0.join("nested").join("recent.json");
    assert_eq!(list_from(&path).unwrap(), Vec::new());

    touch_at(&path, &project("/a"), 1).unwrap();
    touch_at(&path, &project("/b"), 2).unwrap();
    touch_at(&path, &project("/a"), 3).unwrap();
    let roots: Vec<(String, u64)> = list_from(&path)
      .unwrap()
      .into_iter()
      .map(|entry| (entry.root, entry.opened_at))
      .collect();
    assert_eq!(roots, [("/a".to_string(), 3), ("/b".to_string(), 2)]);
  }

  #[test]
  fn keeps_the_newest_fifty_and_survives_garbage() {
    let dir = TempDir::new("recent-cap");
    let path = dir.0.join("recent.json");
    std::fs::write(&path, "not json").unwrap();
    for i in 0..60 {
      touch_at(&path, &project(&format!("/p{i}")), i).unwrap();
    }
    let entries = list_from(&path).unwrap();
    assert_eq!(entries.len(), KEEP);
    assert_eq!(entries[0].root, "/p59");
    assert_eq!(entries[KEEP - 1].root, "/p10");
  }
}
