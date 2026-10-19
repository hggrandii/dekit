pub mod border;
pub mod filter_list;
pub mod form;
pub mod list;
pub mod text_input;
pub mod wrap;

/// A typed path as the directory part, up to its last `/`, and the name
/// being typed after it.
pub fn split_path(value: &str) -> (&str, &str) {
  match value.rfind('/') {
    Some(i) => (&value[..=i], &value[i + 1..]),
    None => ("", value),
  }
}

/// Whether a directory entry is a directory, or a link to one.
pub fn is_dir(entry: &std::fs::DirEntry) -> bool {
  match entry.file_type() {
    Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
    Ok(kind) => kind.is_dir(),
    Err(_) => false,
  }
}
