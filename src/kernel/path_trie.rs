use indexmap::IndexMap;

use super::kernel_message::{MoveError, Place};
use super::task::TaskId;
use super::task_path::{TaskPath, is_at_or_under, join};

#[derive(Debug)]
struct TrieNode {
  task: Option<TaskId>,
  /// In list order.
  children: IndexMap<String, TrieNode>,
}

impl TrieNode {
  fn new() -> Self {
    Self {
      task: None,
      children: IndexMap::new(),
    }
  }

  fn is_empty(&self) -> bool {
    self.task.is_none() && self.children.is_empty()
  }

  /// Collect all (path, task_id) pairs under this node via DFS.
  fn collect_all(&self, prefix: &str, result: &mut Vec<(TaskPath, TaskId)>) {
    if let Some(id) = self.task {
      if let Ok(path) = TaskPath::new(prefix) {
        result.push((path, id));
      }
    }
    for (component, child) in &self.children {
      child.collect_all(&join(prefix, component), result);
    }
  }

  #[cfg(test)]
  fn collect_ids(&self, result: &mut Vec<TaskId>) {
    result.extend(self.task);
    for child in self.children.values() {
      child.collect_ids(result);
    }
  }

  /// A task at or under this node.
  fn any_task(&self, prefix: &str) -> Option<TaskPath> {
    if self.task.is_some() {
      return TaskPath::new(prefix).ok();
    }
    self
      .children
      .iter()
      .find_map(|(component, child)| child.any_task(&join(prefix, component)))
  }

  /// The last task listed at or under this node.
  fn last_task(&self) -> Option<TaskId> {
    self
      .task
      .or_else(|| self.children.values().rev().find_map(TrieNode::last_task))
  }

  /// Walk the trie matching glob pattern components.
  fn glob_walk(
    &self,
    prefix: &str,
    pattern: &[&str],
    result: &mut Vec<(TaskPath, TaskId)>,
  ) {
    if pattern.is_empty() {
      // Pattern exhausted: collect this node if it's a task
      if let Some(id) = self.task {
        if let Ok(path) = TaskPath::new(prefix) {
          result.push((path, id));
        }
      }
      return;
    }

    let pat = pattern[0];
    let rest = &pattern[1..];

    if pat == "**" {
      // Match zero components (skip **)
      self.glob_walk(prefix, rest, result);
      // Match one or more components
      for (component, child) in &self.children {
        // Continue with ** (match more) and with rest (done matching **)
        child.glob_walk(&join(prefix, component), pattern, result);
      }
    } else if pat == "*" {
      // Match exactly one component
      for (component, child) in &self.children {
        child.glob_walk(&join(prefix, component), rest, result);
      }
    } else {
      // Literal match
      if let Some(child) = self.children.get(pat) {
        child.glob_walk(&join(prefix, pat), rest, result);
      }
    }
  }
}

/// Why a task cannot go at a path.
#[derive(Debug, Eq, PartialEq)]
pub enum PathConflict {
  /// A task is at the path already.
  Taken(TaskId),
  /// A task is above or under the path. Tasks are always leaves.
  Nested(TaskPath),
}

/// A task a move took elsewhere.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Moved {
  pub id: TaskId,
  pub from: TaskPath,
  pub to: TaskPath,
}

/// Task paths, the names under each path in list order: the order they
/// were added. Tasks are leaves, and every non-task node has a task under
/// it, but for the places `place` holds while a batch is registered.
pub struct PathTrie {
  root: TrieNode,
}

impl PathTrie {
  pub fn new() -> Self {
    Self {
      root: TrieNode::new(),
    }
  }

  /// Refused, with no change, if a task is at, above, or under `path`.
  /// A new path is listed last among those beside it, or right after
  /// `after` when that is one of them.
  pub fn insert(
    &mut self,
    path: &TaskPath,
    id: TaskId,
    place: &Place,
  ) -> Result<(), PathConflict> {
    if let Some(above) = std::iter::successors(path.parent(), TaskPath::parent)
      .find(|parent| self.resolve(parent).is_some())
    {
      return Err(PathConflict::Nested(above));
    }
    if let Some(node) = self.walk_to(path) {
      if let Some(existing) = node.task {
        return Err(PathConflict::Taken(existing));
      }
      if let Some(below) = node.any_task(path.as_str()) {
        return Err(PathConflict::Nested(below));
      }
    }
    let mut node = &mut self.root;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
      let index = match node.children.get_index_of(component) {
        Some(index) => index,
        None => {
          let last = node.children.len();
          node.children.insert(component.to_string(), TrieNode::new());
          // The place is the task's; the groups it makes go last.
          let place = match place {
            _ if components.peek().is_some() => last,
            Place::First => 0,
            Place::After(after) if after.parent() == path.parent() => node
              .children
              .get_index_of(after.name())
              .map_or(last, |index| index + 1),
            Place::Keep | Place::Last | Place::After(_) => last,
          };
          node.children.move_index(last, place);
          place
        }
      };
      node = &mut node.children[index];
    }
    node.task = Some(id);
    Ok(())
  }

  /// Holds the path's place in the lists until a task is inserted there
  /// or `prune` drops it. Nothing is held where an insert would be
  /// refused for a task above.
  pub fn place(&mut self, path: &TaskPath) {
    let mut node = &mut self.root;
    for component in path.components() {
      if node.task.is_some() {
        return;
      }
      node = node
        .children
        .entry(component.to_string())
        .or_insert_with(TrieNode::new);
    }
  }

  /// Remove the task at the given path. Returns the TaskId if found.
  /// Prunes empty ancestor nodes.
  pub fn remove(&mut self, path: &TaskPath) -> Option<TaskId> {
    let mut node = &mut self.root;
    for component in path.components() {
      node = node.children.get_mut(component)?;
    }
    let id = node.task.take();
    self.prune(path);
    id
  }

  /// Drops the nodes along `path` that have no task at or under them.
  pub fn prune(&mut self, path: &TaskPath) {
    fn prune<'a>(
      node: &mut TrieNode,
      mut components: impl Iterator<Item = &'a str>,
    ) {
      let Some(component) = components.next() else {
        return;
      };
      let Some(child) = node.children.get_mut(component) else {
        return;
      };
      prune(child, components);
      if child.is_empty() {
        node.children.shift_remove(component);
      }
    }
    prune(&mut self.root, path.components());
  }

  /// Moves the task or group at `from`, with everything under it, to
  /// `to`, placed among the names beside it there. A group at `to` that
  /// holds nothing but it is not in the way: the move empties it, and it
  /// takes the group's place. Nothing changes when it is refused. Returns
  /// every task under the new place in list order, with its old path.
  pub fn move_node(
    &mut self,
    from: &TaskPath,
    to: &TaskPath,
    place: &Place,
  ) -> Result<Vec<Moved>, MoveError> {
    let not_found = || MoveError::NotFound(from.clone());
    if from.is_root() || to.is_root() {
      return Err(not_found());
    }
    match self.walk_to(from) {
      Some(node) if !node.is_empty() => (),
      Some(_) | None => return Err(not_found()),
    }
    let name = to.name();
    if from != to {
      if is_at_or_under(to.as_str(), from.as_str()) {
        return Err(MoveError::IntoItself);
      }
      if let Some(above) = std::iter::successors(to.parent(), TaskPath::parent)
        .find(|parent| self.resolve(parent).is_some())
      {
        return Err(MoveError::UnderTask(above));
      }
      let emptied = std::iter::successors(from.parent(), TaskPath::parent)
        .take_while(|group| {
          self
            .walk_to(group)
            .is_some_and(|node| node.children.len() == 1)
        })
        .any(|group| group == *to);
      if self.walk_to(to).is_some() && !emptied {
        return Err(MoveError::Taken(to.clone()));
      }
    }
    // The path to place it after must be beside the destination, as it
    // will be once the node is taken out.
    if let Place::After(after) = place {
      let beside = after.parent() == to.parent()
        && after != from
        && after != to
        && self.walk_to(after).is_some();
      if !beside {
        return Err(MoveError::BadAfter(after.clone()));
      }
    }

    let mut old: std::collections::HashMap<TaskId, TaskPath> = {
      let mut tasks = Vec::new();
      if let Some(node) = self.walk_to(from) {
        node.collect_all(from.as_str(), &mut tasks);
      }
      tasks.into_iter().map(|(path, id)| (id, path)).collect()
    };
    let from_parent = from.parent().unwrap_or_else(TaskPath::root);
    let parent = self
      .walk_to_mut(&from_parent)
      .expect("the parent of a node is there");
    let index = parent
      .children
      .get_index_of(from.name())
      .expect("the node is there");
    let (_, node) = parent
      .children
      .shift_remove_index(index)
      .expect("the node is there");
    let keep = (from.parent() == to.parent()).then_some(index);
    let to_parent = to.parent().unwrap_or_else(TaskPath::root);
    // The group the move makes beside where the node was, if any.
    let made = std::iter::successors(Some(to_parent.clone()), TaskPath::parent)
      .take_while(|group| !group.is_root() && self.walk_to(group).is_none())
      .last()
      .filter(|group| group.parent() == from.parent());
    // In first, and only then the groups it left empty pruned: one of them
    // can be the place to go after, or on the way to the destination.
    self.place(&to_parent);
    if let (Place::Keep, Some(_)) = (place, made) {
      let parent = self
        .walk_to_mut(&from_parent)
        .expect("the parent of a node is there");
      let last = parent.children.len() - 1;
      parent.children.move_index(last, index.min(last));
    }
    let parent = self
      .walk_to_mut(&to_parent)
      .expect("the destination's groups are there");
    // The group it left empty, there before it.
    let emptied = parent.children.shift_remove_full(name).map(|(i, ..)| i);
    let last = parent.children.len();
    parent.children.insert(name.to_string(), node);
    let at = match place {
      Place::First => 0,
      Place::Last => last,
      Place::After(after) => parent
        .children
        .get_index_of(after.name())
        .map_or(last, |index| index + 1),
      Place::Keep => keep.or(emptied).unwrap_or(last).min(last),
    };
    parent.children.move_index(last, at);
    let mut moved = Vec::new();
    parent.children[at].collect_all(to.as_str(), &mut moved);
    if from.parent() != to.parent() {
      self.prune(from);
    }
    Ok(
      moved
        .into_iter()
        .map(|(path, id)| Moved {
          id,
          from: old.remove(&id).unwrap_or_else(|| path.clone()),
          to: path,
        })
        .collect(),
    )
  }

  /// Resolve a path to its TaskId. O(depth).
  pub fn resolve(&self, path: &TaskPath) -> Option<TaskId> {
    let mut node = &self.root;
    for component in path.components() {
      node = node.children.get(component)?;
    }
    node.task
  }

  /// The task listed just before `path`.
  pub fn before(&self, path: &TaskPath) -> Option<TaskId> {
    let mut node = &self.root;
    let mut before = None;
    for component in path.components() {
      let index = node.children.get_index_of(component)?;
      let earlier = node.children.get_range(..index)?;
      // A task under a later path part is listed after one beside an
      // earlier part.
      before = earlier
        .values()
        .rev()
        .find_map(TrieNode::last_task)
        .or(before);
      node = &node.children[index];
    }
    before
  }

  /// Where a path is listed: its index among the names beside it, at each
  /// part of the path. Paths in list order have these in order.
  pub fn position(&self, path: &TaskPath) -> Option<Vec<usize>> {
    let mut node = &self.root;
    let mut position = Vec::new();
    for component in path.components() {
      let (index, _, child) = node.children.get_full(component)?;
      position.push(index);
      node = child;
    }
    Some(position)
  }

  /// Every task, in list order.
  #[cfg(test)]
  pub fn tasks(&self) -> Vec<TaskId> {
    let mut result = Vec::new();
    self.root.collect_ids(&mut result);
    result
  }

  /// Recursively collect all tasks under a prefix path.
  pub fn descendants(&self, path: &TaskPath) -> Vec<(TaskPath, TaskId)> {
    let node = self.walk_to(path);
    let Some(node) = node else {
      return Vec::new();
    };
    let mut result = Vec::new();
    // Collect from children, not the node itself
    for (component, child) in &node.children {
      child.collect_all(&join(path.as_str(), component), &mut result);
    }
    result
  }

  /// Find all tasks whose paths match a glob pattern, sorted by path.
  /// Supports `*` (single component) and `**` (recursive).
  pub fn glob(&self, pattern: &str) -> Vec<(TaskPath, TaskId)> {
    let parts: Vec<&str> =
      pattern.split('/').filter(|c| !c.is_empty()).collect();
    let mut result = Vec::new();
    self.root.glob_walk("", &parts, &mut result);
    // A path can match through several `**` derivations.
    result.sort();
    result.dedup();
    result
  }

  fn walk_to(&self, path: &TaskPath) -> Option<&TrieNode> {
    let mut node = &self.root;
    for component in path.components() {
      node = node.children.get(component)?;
    }
    Some(node)
  }

  fn walk_to_mut(&mut self, path: &TaskPath) -> Option<&mut TrieNode> {
    let mut node = &mut self.root;
    for component in path.components() {
      node = node.children.get_mut(component)?;
    }
    Some(node)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn listed_paths(trie: &PathTrie) -> Vec<String> {
    let mut all = Vec::new();
    trie.root.collect_all("", &mut all);
    all.into_iter().map(|(path, _)| path.to_string()).collect()
  }

  #[test]
  fn moves_tasks_and_groups() {
    let mut trie = PathTrie::new();
    for (i, path) in ["db", "web/api", "web/ui", "cache"].iter().enumerate() {
      trie.insert(&p(path), TaskId(i + 1), &Place::Last).unwrap();
    }
    // Reorder beside the same paths.
    let moved = trie.move_node(&p("cache"), &p("cache"), &Place::First);
    assert_eq!(moved.unwrap().len(), 1);
    assert_eq!(listed_paths(&trie), ["cache", "db", "web/api", "web/ui"]);
    trie
      .move_node(&p("web"), &p("web"), &after("cache"))
      .unwrap();
    assert_eq!(listed_paths(&trie), ["cache", "web/api", "web/ui", "db"]);
    trie
      .move_node(&p("web/api"), &p("web/api"), &after("web/ui"))
      .unwrap();
    assert_eq!(listed_paths(&trie), ["cache", "web/ui", "web/api", "db"]);

    // Into another group, and a group under a new one.
    let moved = trie
      .move_node(&p("db"), &p("web/db"), &after("web/ui"))
      .unwrap();
    assert_eq!(
      moved,
      [Moved {
        id: TaskId(1),
        from: p("db"),
        to: p("web/db")
      }]
    );
    assert_eq!(
      listed_paths(&trie),
      ["cache", "web/ui", "web/db", "web/api"]
    );
    let moved = trie
      .move_node(&p("web"), &p("apps/web"), &Place::Last)
      .unwrap();
    assert_eq!(moved.len(), 3);
    assert_eq!(moved[0].from, p("web/ui"));
    assert_eq!(
      listed_paths(&trie),
      ["cache", "apps/web/ui", "apps/web/db", "apps/web/api"]
    );
    assert_eq!(trie.resolve(&p("apps/web/db")), Some(TaskId(1)));
    assert_eq!(trie.resolve(&p("web/db")), None);

    // A rename keeps the place.
    trie
      .move_node(&p("apps/web/db"), &p("apps/web/pg"), &Place::Keep)
      .unwrap();
    assert_eq!(
      listed_paths(&trie),
      ["cache", "apps/web/ui", "apps/web/pg", "apps/web/api"]
    );

    for (from, to, place, err) in [
      ("nope", "x", Place::Last, MoveError::NotFound(p("nope"))),
      (
        "cache",
        "apps/web/ui",
        Place::Last,
        MoveError::Taken(p("apps/web/ui")),
      ),
      ("apps", "apps/x/apps", Place::Last, MoveError::IntoItself),
      (
        "apps",
        "cache/apps",
        Place::Last,
        MoveError::UnderTask(p("cache")),
      ),
      (
        "cache",
        "cache",
        after("nope"),
        MoveError::BadAfter(p("nope")),
      ),
      (
        "cache",
        "cache",
        after("cache"),
        MoveError::BadAfter(p("cache")),
      ),
      (
        "cache",
        "cache",
        after("apps/web/ui"),
        MoveError::BadAfter(p("apps/web/ui")),
      ),
    ] {
      assert_eq!(trie.move_node(&p(from), &p(to), &place), Err(err));
    }
    assert_eq!(
      listed_paths(&trie),
      ["cache", "apps/web/ui", "apps/web/pg", "apps/web/api"]
    );
  }

  /// A group that a move into a group under it leaves empty for a
  /// moment keeps its place.
  #[test]
  fn a_move_deeper_into_its_group_keeps_the_group_in_place() {
    let mut trie = PathTrie::new();
    for (i, path) in ["g/a", "b"].iter().enumerate() {
      trie.insert(&p(path), TaskId(i + 1), &Place::Last).unwrap();
    }
    trie
      .move_node(&p("g/a"), &p("g/sub/a"), &Place::Last)
      .unwrap();
    assert_eq!(listed_paths(&trie), ["g/sub/a", "b"]);
  }

  /// A move up out of groups it leaves empty, into one of them: the
  /// group it goes into stays, and it goes where the one it left was.
  #[test]
  fn a_move_up_out_of_the_groups_it_empties() {
    let mut trie = PathTrie::new();
    for (i, path) in ["a/b/c", "x"].iter().enumerate() {
      trie.insert(&p(path), TaskId(i + 1), &Place::Last).unwrap();
    }
    trie
      .move_node(&p("a/b/c"), &p("a/c"), &Place::Last)
      .unwrap();
    assert_eq!(listed_paths(&trie), ["a/c", "x"]);

    let mut trie = PathTrie::new();
    for (i, path) in ["a/b/c", "a/d", "x"].iter().enumerate() {
      trie.insert(&p(path), TaskId(i + 1), &Place::Last).unwrap();
    }
    trie
      .move_node(&p("a/b/c"), &p("a/c"), &after("a/b"))
      .unwrap();
    assert_eq!(listed_paths(&trie), ["a/c", "a/d", "x"]);
  }

  /// A rename into a group not there yet makes it where the task was.
  #[test]
  fn a_rename_into_a_new_group_keeps_the_place() {
    let mut trie = PathTrie::new();
    for (i, path) in ["api", "db", "web/ui"].iter().enumerate() {
      trie.insert(&p(path), TaskId(i + 1), &Place::Last).unwrap();
    }
    trie
      .move_node(&p("api"), &p("srv/api"), &Place::Keep)
      .unwrap();
    assert_eq!(listed_paths(&trie), ["srv/api", "db", "web/ui"]);
    // Into a group that is there: last in it.
    trie
      .move_node(&p("db"), &p("web/db"), &Place::Keep)
      .unwrap();
    assert_eq!(listed_paths(&trie), ["srv/api", "web/ui", "web/db"]);
  }

  /// A task or group moved up onto the group that holds only it takes
  /// the group's place; a group that holds more is in the way.
  #[test]
  fn a_move_onto_the_group_it_empties_takes_its_place() {
    let mut trie = PathTrie::new();
    for (i, path) in ["x", "a/b/c", "y", "w/u", "w/v"].iter().enumerate() {
      trie.insert(&p(path), TaskId(i + 1), &Place::Last).unwrap();
    }
    let moved = trie.move_node(&p("a/b/c"), &p("a"), &Place::Keep).unwrap();
    assert_eq!(
      moved,
      [Moved {
        id: TaskId(2),
        from: p("a/b/c"),
        to: p("a")
      }]
    );
    assert_eq!(listed_paths(&trie), ["x", "a", "y", "w/u", "w/v"]);
    assert_eq!(trie.resolve(&p("a")), Some(TaskId(2)));
    assert_eq!(
      trie.move_node(&p("w/u"), &p("w"), &Place::Keep),
      Err(MoveError::Taken(p("w")))
    );
    assert_eq!(
      trie.move_node(&p("w"), &p("w/v"), &Place::Keep),
      Err(MoveError::IntoItself)
    );
  }

  fn p(path: &str) -> TaskPath {
    TaskPath::new(path).unwrap()
  }

  fn after(path: &str) -> Place {
    Place::After(p(path))
  }

  #[test]
  fn test_insert_and_resolve() {
    let mut trie = PathTrie::new();
    trie.insert(&p("web"), TaskId(1), &Place::Last).unwrap();
    trie
      .insert(&p("services/api"), TaskId(2), &Place::Last)
      .unwrap();
    trie
      .insert(&p("services/worker"), TaskId(3), &Place::Last)
      .unwrap();

    assert_eq!(trie.resolve(&p("web")), Some(TaskId(1)));
    assert_eq!(trie.resolve(&p("services/api")), Some(TaskId(2)));
    assert_eq!(trie.resolve(&p("services/worker")), Some(TaskId(3)));
    assert_eq!(trie.resolve(&p("missing")), None);
    assert_eq!(trie.resolve(&p("services")), None); // intermediate node
  }

  #[test]
  fn test_insert_conflict() {
    let mut trie = PathTrie::new();
    trie.insert(&p("web"), TaskId(1), &Place::Last).unwrap();
    let err = trie.insert(&p("web"), TaskId(2), &Place::Last).unwrap_err();
    assert_eq!(err, PathConflict::Taken(TaskId(1)));
  }

  #[test]
  fn refuses_a_task_above_or_under_another() {
    let mut trie = PathTrie::new();
    trie.insert(&p("web"), TaskId(1), &Place::Last).unwrap();
    trie
      .insert(&p("api/v1/http"), TaskId(2), &Place::Last)
      .unwrap();

    assert_eq!(
      trie.insert(&p("web/dev/a"), TaskId(3), &Place::Last),
      Err(PathConflict::Nested(p("web")))
    );
    assert_eq!(
      trie.insert(&p("api"), TaskId(3), &Place::Last),
      Err(PathConflict::Nested(p("api/v1/http")))
    );
    assert_eq!(
      trie.insert(&p("api/v1"), TaskId(3), &Place::Last),
      Err(PathConflict::Nested(p("api/v1/http")))
    );
    assert!(trie.descendants(&p("web")).is_empty(), "nothing was made");

    trie.remove(&p("web"));
    trie
      .insert(&p("web/dev/a"), TaskId(3), &Place::Last)
      .unwrap();
    trie.insert(&p("api/v2"), TaskId(4), &Place::Last).unwrap();
  }

  #[test]
  fn test_remove() {
    let mut trie = PathTrie::new();
    trie.insert(&p("a/b"), TaskId(1), &Place::Last).unwrap();
    trie.insert(&p("a/c"), TaskId(2), &Place::Last).unwrap();

    assert_eq!(trie.remove(&p("a/b")), Some(TaskId(1)));
    assert_eq!(trie.resolve(&p("a/b")), None);
    // /a/c still exists
    assert_eq!(trie.resolve(&p("a/c")), Some(TaskId(2)));

    // Remove remaining child; /a should be pruned
    assert_eq!(trie.remove(&p("a/c")), Some(TaskId(2)));
    assert_eq!(trie.resolve(&p("a/c")), None);
  }

  #[test]
  fn test_remove_nonexistent() {
    let mut trie = PathTrie::new();
    assert_eq!(trie.remove(&p("nope")), None);
  }

  #[test]
  fn test_descendants() {
    let mut trie = PathTrie::new();
    trie
      .insert(&p("services/api"), TaskId(1), &Place::Last)
      .unwrap();
    trie
      .insert(&p("services/web/v1"), TaskId(2), &Place::Last)
      .unwrap();
    trie
      .insert(&p("services/web/v2"), TaskId(3), &Place::Last)
      .unwrap();
    trie
      .insert(&p("tools/lint"), TaskId(4), &Place::Last)
      .unwrap();

    let desc = trie.descendants(&p("services"));
    assert_eq!(desc.len(), 3);
    assert_eq!(desc[0], (p("services/api"), TaskId(1)));
    assert_eq!(desc[1], (p("services/web/v1"), TaskId(2)));
    assert_eq!(desc[2], (p("services/web/v2"), TaskId(3)));
  }

  #[test]
  fn test_glob_star() {
    let mut trie = PathTrie::new();
    trie
      .insert(&p("services/api"), TaskId(1), &Place::Last)
      .unwrap();
    trie
      .insert(&p("services/web/v1"), TaskId(2), &Place::Last)
      .unwrap();
    trie
      .insert(&p("services/web/v2"), TaskId(3), &Place::Last)
      .unwrap();
    trie
      .insert(&p("tools/lint"), TaskId(4), &Place::Last)
      .unwrap();

    let results = trie.glob("services/*");
    assert_eq!(results, [(p("services/api"), TaskId(1))]);

    let results = trie.glob("services/web/*");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], (p("services/web/v1"), TaskId(2)));
    assert_eq!(results[1], (p("services/web/v2"), TaskId(3)));
  }

  #[test]
  fn test_glob_double_star() {
    let mut trie = PathTrie::new();
    trie
      .insert(&p("services/api"), TaskId(1), &Place::Last)
      .unwrap();
    trie
      .insert(&p("services/web/v1"), TaskId(2), &Place::Last)
      .unwrap();
    trie
      .insert(&p("services/web/v2"), TaskId(3), &Place::Last)
      .unwrap();
    trie
      .insert(&p("tools/lint"), TaskId(4), &Place::Last)
      .unwrap();

    let results = trie.glob("services/**");
    assert_eq!(results.len(), 3);

    // A trailing `**` matches the path itself too.
    let results = trie.glob("services/api/**");
    assert_eq!(results, [(p("services/api"), TaskId(1))]);

    let results = trie.glob("**");
    assert_eq!(results.len(), 4);

    // Several `**` derivations can match the same path; no duplicates.
    let results = trie.glob("**/**");
    assert_eq!(results.len(), 4);
  }

  #[test]
  fn test_glob_mixed() {
    let mut trie = PathTrie::new();
    trie.insert(&p("a/b/c"), TaskId(1), &Place::Last).unwrap();
    trie.insert(&p("a/x/c"), TaskId(2), &Place::Last).unwrap();
    trie.insert(&p("a/b/d"), TaskId(3), &Place::Last).unwrap();

    let results = trie.glob("a/*/c");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], (p("a/b/c"), TaskId(1)));
    assert_eq!(results[1], (p("a/x/c"), TaskId(2)));
  }

  fn listed(trie: &PathTrie) -> Vec<usize> {
    trie.tasks().iter().map(|id| id.0).collect()
  }

  #[test]
  fn lists_paths_as_added_with_a_group_at_its_first_task() {
    let mut trie = PathTrie::new();
    for (i, at) in ["web/dev", "db", "web/ui", "api/a/b"].iter().enumerate() {
      trie.insert(&p(at), TaskId(i + 1), &Place::Last).unwrap();
    }
    assert_eq!(listed(&trie), [1, 3, 2, 4]);
    assert_eq!(trie.before(&p("web/dev")), None);
    assert_eq!(trie.before(&p("web/ui")), Some(TaskId(1)));
    assert_eq!(trie.before(&p("db")), Some(TaskId(3)));
    assert_eq!(trie.before(&p("api/a/b")), Some(TaskId(2)));

    trie.remove(&p("web/dev"));
    assert_eq!(listed(&trie), [3, 2, 4], "the others keep their order");
    trie.remove(&p("web/ui"));
    trie.insert(&p("web/dev"), TaskId(5), &Place::Last).unwrap();
    assert_eq!(listed(&trie), [2, 4, 5], "an emptied group is gone");
  }

  #[test]
  fn lists_a_path_right_after_the_one_it_names() {
    let mut trie = PathTrie::new();
    trie.insert(&p("db"), TaskId(1), &Place::Last).unwrap();
    trie.insert(&p("web/api"), TaskId(2), &Place::Last).unwrap();
    trie.insert(&p("web/ui"), TaskId(3), &Place::Last).unwrap();
    trie
      .insert(&p("web/api-2"), TaskId(4), &Place::After(p("web/api")))
      .unwrap();
    trie
      .insert(&p("db-2"), TaskId(5), &Place::After(p("db")))
      .unwrap();
    assert_eq!(listed(&trie), [1, 5, 2, 4, 3]);

    // Not beside the one it names: listed last.
    trie
      .insert(&p("cache"), TaskId(6), &Place::After(p("web/api")))
      .unwrap();
    assert_eq!(listed(&trie), [1, 5, 2, 4, 3, 6]);
  }

  #[test]
  fn a_held_place_is_filled_or_dropped() {
    let mut trie = PathTrie::new();
    trie.insert(&p("a"), TaskId(1), &Place::Last).unwrap();
    for at in ["web/api", "db", "web/ui", "a/under"] {
      trie.place(&p(at));
    }
    assert_eq!(listed(&trie), [1]);
    assert_eq!(trie.insert(&p("web"), TaskId(9), &Place::Last), Ok(()));
    trie.remove(&p("web"));

    trie.insert(&p("db"), TaskId(2), &Place::Last).unwrap();
    trie.insert(&p("web/ui"), TaskId(3), &Place::Last).unwrap();
    assert_eq!(listed(&trie), [1, 3, 2]);
    assert_eq!(trie.before(&p("web/ui")), Some(TaskId(1)));
    assert_eq!(trie.before(&p("db")), Some(TaskId(3)));

    trie.prune(&p("web/api"));
    trie.prune(&p("a/under"));
    assert_eq!(listed(&trie), [1, 3, 2]);
    assert_eq!(trie.root.children.len(), 3);
    assert_eq!(trie.root.children["web"].children.len(), 1);
    assert!(trie.root.children["a"].children.is_empty());
  }
}
