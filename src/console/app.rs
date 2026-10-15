use std::sync::Arc;

use tokio::sync::mpsc::{
  UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::{
  command::{Command, issue},
  config::{config::Config, task::CmdConfig},
  console::{
    action::{Action, CopyMove, RunnerTab, ScrollUnit},
    app_layout::AppLayout,
    keymap::{Keymap, KeymapGroup},
    modal::{
      add_task::AddTaskModal,
      commands_menu::CommandsMenuModal,
      main_menu::MainMenuModal,
      modal::{Modal, ModalResult},
      quit::QuitModal,
      remove_task::RemoveTaskModal,
      rename_task::RenameTaskModal,
      runner_select::RunnerSelectModal,
    },
    state::{Scope, State},
    task_tree::{Node, TaskTree},
    task_view::TaskView,
    theme::{BlockTheme, Theme},
    ui_header::{HeaderInfo, render_header},
    ui_keymap::render_keymap,
    ui_tasks::render_tasks,
    ui_term::render_term,
    ui_zoom_tip::render_zoom_tip,
  },
  kernel::{
    copy_mode::CopyMove as KernelCopyMove,
    kernel_message::{KernelCommand, TaskContext, TaskRegistration},
    sub_trie::SubMode,
    task::{
      ChannelTask, ExitInfo, TaskCmd, TaskDef, TaskId, TaskKind,
      TaskNotification, TaskNotify, TaskState,
    },
    task_key::TaskKey,
    task_path::{TaskPath, path_name, unique},
    task_screen::{
      DEFAULT_SIZE, ObserverId, ScreenNotify, ScrollUnit as KernelScrollUnit,
      TaskScreen, TaskScreenCmd, TaskScreenEffect,
    },
  },
  target::Target,
  term::{
    CursorStyle, Geometry, Grid, Screen, ScreenHandle, TermEvent,
    grid::Rect,
    key::{Key, KeyEventKind},
    mouse::{MouseButton, MouseEventKind},
    style::Style,
  },
};

use crate::upgrade::snapshot::TaskKind as TaskKindSnapshot;

fn kernel_copy_move(dir: CopyMove) -> KernelCopyMove {
  match dir {
    CopyMove::Up => KernelCopyMove::Up,
    CopyMove::Down => KernelCopyMove::Down,
    CopyMove::Left => KernelCopyMove::Left,
    CopyMove::Right => KernelCopyMove::Right,
  }
}

fn kernel_scroll_unit(unit: ScrollUnit) -> KernelScrollUnit {
  match unit {
    ScrollUnit::Line => KernelScrollUnit::Line,
    ScrollUnit::HalfScreen => KernelScrollUnit::HalfScreen,
    ScrollUnit::Screen => KernelScrollUnit::Screen,
  }
}

pub fn console_task_registration(
  task_id: TaskId,
  def: TaskDef,
  config: Arc<Config>,
  keymap: Keymap,
) -> TaskRegistration {
  let screen = Screen::new(DEFAULT_SIZE, 0);
  let handle = ScreenHandle::new(screen.frame());
  TaskRegistration {
    task_id,
    def: TaskDef {
      vt: Some(handle.clone()),
      ..def
    },
    factory: Box::new(move |pc| {
      log::debug!("Creating console task (id: {})", pc.task_id.0);
      // Subscribe at registration, so tasks registered later are seen
      // before any message addressed to the console.
      pc.subscribe_path(
        TaskKey::default_space(TaskPath::root()),
        SubMode::Subtree,
      );
      let (tx, rx) = unbounded_channel();
      tokio::spawn(App::new(config, keymap, rx, pc, screen, handle).run());
      Box::new(ChannelTask::new(tx))
    }),
  }
}

/// The built-in console: one view state shared by every attachment, drawn
/// into its own task screen. It observes the screens of the tasks it
/// shows only while something is attached to it.
pub struct App {
  config: Arc<Config>,
  keymap: Keymap,
  theme: Theme,
  state: State,
  modal: Option<Box<dyn Modal>>,
  receiver: UnboundedReceiver<TaskCmd>,
  pc: TaskContext,

  screen: TaskScreen,
  effects: Vec<TaskScreenEffect>,

  /// The console's identity as an observer of task screens, and where
  /// their notifications arrive.
  observer: ObserverId,
  sink: UnboundedSender<ScreenNotify>,
  notifies: UnboundedReceiver<ScreenNotify>,

  stop: bool,
  /// Something the picture depends on changed since the last render.
  dirty: bool,
}

impl App {
  fn new(
    config: Arc<Config>,
    keymap: Keymap,
    receiver: UnboundedReceiver<TaskCmd>,
    pc: TaskContext,
    screen: Screen,
    handle: ScreenHandle,
  ) -> Self {
    let (sink, notifies) = unbounded_channel();
    App {
      state: State {
        scope: Scope::Tasks,
        tasks: TaskTree::new(),
        hide_keymap_window: !config.tui.tips.show,
        quitting: false,
        header: Default::default(),
        hover: None,
      },
      config,
      keymap,
      theme: Theme::dark(),
      modal: None,
      receiver,
      screen: TaskScreen::new(pc.task_id, screen, handle, 1),
      pc,
      effects: Vec::new(),
      observer: ObserverId::new(),
      sink,
      notifies,
      stop: false,
      dirty: true,
    }
  }

  async fn run(mut self) {
    let mut geometry = self.task_geometry();
    let mut cmds = Vec::new();
    let mut notifies = Vec::new();
    while !self.stop {
      let now = self.task_geometry();
      if now != geometry {
        geometry = now;
        for task in self.state.tasks.iter() {
          self.pc.send_msg(
            task.id,
            TaskScreenCmd::Input {
              observer: self.observer,
              event: TermEvent::Resize(now),
            },
          );
        }
      }

      tokio::select! {
        n = self.receiver.recv_many(&mut cmds, 512) => {
          // Zero means the kernel is gone.
          if n == 0 {
            break;
          }
          for cmd in cmds.drain(..) {
            self.dirty = true;
            self.handle_task_cmd(cmd);
          }
        }
        _ = self.notifies.recv_many(&mut notifies, 512) => {
          for notify in notifies.drain(..) {
            self.handle_screen_notify(notify);
          }
        }
      }

      if self.dirty && self.screen.has_observers() {
        self.dirty = false;
        self.render();
      }
    }
    self.pc.unsubscribe_path(
      TaskKey::default_space(TaskPath::root()),
      SubMode::Subtree,
    );
    self.pc.send(KernelCommand::TaskStopped(ExitInfo::code(0)));
  }

  fn render(&mut self) {
    let layout = self.layout();
    let client_versions: Vec<String> =
      self.screen.client_versions().map(String::from).collect();
    let theme = &self.theme;
    let vt = self.screen.screen_mut();
    let grid = vt.grid_mut();
    let bg = Style::default()
      .bg(theme.block.bg.into())
      .fg(theme.block.text.into());
    let bg = grid.intern_style(bg);
    grid.erase_all(bg);
    grid.cursor_pos = None;
    grid.cursor_style = CursorStyle::Default;

    let header = render_header(
      layout.header,
      grid,
      &HeaderInfo {
        runner: self.config.runner.as_ref(),
        version: env!("CARGO_PKG_VERSION"),
        client_versions: client_versions.iter().map(String::as_str).collect(),
        hover: self.state.hover,
        held: self.modal.as_ref().and_then(|modal| modal.unshaded()),
      },
      theme,
    );
    self.state.header = header;
    render_tasks(layout.sidebar, grid, &mut self.state, &self.config, theme);
    render_term(layout.term, grid, &self.state, theme);
    render_keymap(layout.keymap, grid, &self.state, &self.keymap, theme);
    render_zoom_tip(layout.zoom_banner, grid, &self.keymap, theme);
    if let Some(modal) = &mut self.modal {
      shade(grid, &theme.block, modal.unshaded());
      grid.cursor_pos = None;
      grid.cursor_style = CursorStyle::Default;
      modal.render(grid, &self.keymap, theme);
    }

    match grid.cursor_pos {
      Some(pos) => {
        grid.set_pos(pos);
        vt.set_hide_cursor(false);
      }
      None => vt.set_hide_cursor(true),
    }
    let attached = self.screen.has_observers();
    self.screen.rendered(&mut self.effects);
    self.apply_screen(attached);
  }

  /// The whole screen.
  fn frame(&self) -> Rect {
    let size = self.screen.size();
    Rect::new(0, 0, size.width, size.height)
  }

  fn layout(&self) -> AppLayout {
    AppLayout::new(
      self.frame(),
      self.state.scope.is_zoomed(),
      self.state.hide_keymap_window,
      &self.config,
    )
  }

  fn add_task(
    &mut self,
    id: TaskId,
    label: Option<String>,
    path: Option<TaskPath>,
    after: Option<TaskId>,
    kind: TaskKind,
    status: TaskState,
    vt: Option<ScreenHandle>,
  ) {
    // Console tasks always have a path.
    let (Some(vt), Some(path)) = (vt, path) else {
      return;
    };
    self.state.tasks.add(
      TaskView {
        id,
        label,
        path: Arc::new(path),
        kind,
        status,
        vt,
        present: None,
      },
      after,
    );
    if self.screen.has_observers() {
      self.observe(id);
    }
  }

  fn observe(&self, id: TaskId) {
    self.pc.send_msg(
      id,
      TaskScreenCmd::Attach {
        observer: self.observer,
        geometry: self.task_geometry(),
        sink: self.sink.clone(),
        client_version: None,
      },
    );
  }

  /// What the task screens the console shows are told about their size:
  /// the terminal pane in cells, and the cells' size as the console's
  /// own clients report it.
  fn task_geometry(&self) -> Geometry {
    Geometry {
      size: self.layout().term_area().size(),
      cell_pixels: self.screen.reported_cell_pixels(),
    }
  }

  fn observe_all(&self) {
    for task in self.state.tasks.iter() {
      self.observe(task.id);
    }
  }

  fn unobserve_all(&mut self) {
    for task in self.state.tasks.iter_mut() {
      task.present = None;
      self.pc.send_msg(
        task.id,
        TaskScreenCmd::Detach {
          observer: self.observer,
        },
      );
    }
  }

  fn handle_task_cmd(&mut self, cmd: TaskCmd) {
    let msg = match cmd {
      TaskCmd::Start => {
        self.pc.send(KernelCommand::TaskStarted);
        self.pc.send(KernelCommand::TaskReady);
        return;
      }
      TaskCmd::Stop | TaskCmd::Kill => {
        self.stop = true;
        return;
      }
      TaskCmd::Duplicate(_) | TaskCmd::Thaw => return,
      // The console keeps no state worth carrying: attachments re-attach
      // and the task list is replayed on resume.
      TaskCmd::Freeze(number) => {
        self.pc.send(KernelCommand::TaskFrozen(
          number,
          TaskKindSnapshot::Console {},
        ));
        return;
      }
      TaskCmd::Msg(msg) => msg,
    };
    let msg = match msg.downcast::<Action>() {
      Ok(action) => return self.handle_action(None, *action),
      Err(msg) => msg,
    };
    let msg = match msg.downcast::<TaskScreenCmd>() {
      Ok(cmd) => return self.handle_screen_cmd(*cmd),
      Err(msg) => msg,
    };
    match msg.downcast::<TaskNotification>() {
      Ok(n) => self.handle_task_notify(n.from, n.notify),
      Err(_) => log::error!("Console received unknown message"),
    }
  }

  /// Commands addressed to the console's own screen. Keys, mouse, and
  /// pastes are the UI's input; attachments and their geometry go to the
  /// screen.
  fn handle_screen_cmd(&mut self, cmd: TaskScreenCmd) {
    let cmd = match cmd {
      TaskScreenCmd::Input {
        observer,
        event: TermEvent::Key(key),
      } => return self.handle_key(observer, key),
      TaskScreenCmd::Input {
        observer,
        event: TermEvent::Mouse(mouse),
      } => return self.handle_mouse(observer, mouse),
      TaskScreenCmd::Input {
        event: TermEvent::Paste(text),
        ..
      } => {
        if self.modal.is_none()
          && self.state.scope.is_term()
          && let Some(task) = self.state.current_task()
        {
          self.input_current(Some(task.id), TermEvent::Paste(text));
        }
        return;
      }
      cmd @ TaskScreenCmd::Detach { .. } => {
        // The pointer that left would otherwise stay lit for everyone.
        self.state.hover = None;
        cmd
      }
      // A console attachment is what makes a project recent.
      cmd @ TaskScreenCmd::Attach { .. } => {
        if let Some(runner) = self.config.runner.clone() {
          tokio::task::spawn_blocking(move || {
            crate::runner::recent::touch(&runner)
          });
        }
        cmd
      }
      cmd @ TaskScreenCmd::Input { .. } => cmd,
      // Copy mode and scrolling do not apply to a composed UI.
      TaskScreenCmd::CopyEnter
      | TaskScreenCmd::CopyLeave
      | TaskScreenCmd::CopyMove { .. }
      | TaskScreenCmd::CopyBeginSelection
      | TaskScreenCmd::Scroll { .. }
      | TaskScreenCmd::CopyYank => return,
    };
    let attached = self.screen.has_observers();
    self.screen.handle_cmd(cmd, &mut self.effects);
    self.apply_screen(attached);
  }

  /// Applies what the screen asked for, and starts or stops watching task
  /// screens when the first attachment arrives or the last one leaves.
  fn apply_screen(&mut self, attached: bool) {
    // The screen resizes the vt itself; nothing else applies to a UI.
    self.effects.clear();
    match (attached, self.screen.has_observers()) {
      (false, true) => self.observe_all(),
      (true, false) => self.unobserve_all(),
      (false, false) | (true, true) => (),
    }
  }

  fn handle_mouse(
    &mut self,
    observer: ObserverId,
    mouse: crate::term::mouse::MouseEvent,
  ) {
    let frame = self.frame();
    self.state.hover = Some((mouse.x as u16, mouse.y as u16));
    if let Some(modal) = &mut self.modal {
      // A click on the backdrop closes any modal.
      if mouse.kind == MouseEventKind::Down(MouseButton::Left)
        && !modal.area(frame).contains(mouse.x as u16, mouse.y as u16)
      {
        self.modal = None;
        return;
      }
      match modal.handle_mouse(&mouse) {
        ModalResult::Keep => (),
        ModalResult::Close => self.modal = None,
        ModalResult::Run(action) => {
          self.modal = None;
          self.handle_action(Some(observer), action);
        }
        ModalResult::Detach => {
          self.modal = None;
          self.handle_screen_cmd(TaskScreenCmd::Detach { observer });
        }
      }
      return;
    }
    let layout = self.layout();
    let (x, y) = (mouse.x as u16, mouse.y as u16);
    let term_area = layout.term_area();
    // A selection dragged out of the terminal keeps driving it from the
    // nearest edge cell, so the kernel can scroll the view under it. The
    // release has to reach the kernel too, or the drag never ends.
    let drag_outside =
      matches!(
        mouse.kind,
        MouseEventKind::Drag(MouseButton::Left)
          | MouseEventKind::Up(MouseButton::Left)
      ) && matches!(self.state.scope, Scope::Term | Scope::TermZoom)
        && !term_area.contains(x, y)
        && term_area.width > 0
        && term_area.height > 0;
    if drag_outside {
      if let Some(task) = self.state.current_task() {
        let mut event = mouse;
        event.x = x.clamp(term_area.x, term_area.right() - 1) as i32;
        event.y = y.clamp(term_area.y, term_area.bottom() - 1) as i32;
        let event = event.translate(term_area);
        self.input_current(Some(task.id), TermEvent::Mouse(event));
      }
      return;
    }
    if layout.header.contains(x, y) {
      if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
        let hits = self.state.header;
        if hits.logo.is_some_and(|hit| hit.contains(x, y)) {
          self.handle_action(Some(observer), Action::ShowMainMenu);
        } else if hits.runner.is_some_and(|hit| hit.contains(x, y)) {
          self.handle_action(
            Some(observer),
            Action::ShowRunnerSelect {
              tab: RunnerTab::Switch,
            },
          );
        }
      }
      return;
    }
    let pressed = match mouse.kind {
      MouseEventKind::Down(_) => true,
      MouseEventKind::Up(_)
      | MouseEventKind::Drag(_)
      | MouseEventKind::Moved
      | MouseEventKind::ScrollDown
      | MouseEventKind::ScrollUp
      | MouseEventKind::ScrollLeft
      | MouseEventKind::ScrollRight => false,
    };
    let term_area = layout.term_area();
    if term_area.contains(x, y) {
      if pressed && self.state.scope == Scope::Tasks {
        self.state.scope = Scope::Term;
      }
      if let Some(task) = self.state.current_task() {
        let event = mouse.translate(term_area);
        self.input_current(Some(task.id), TermEvent::Mouse(event));
      }
    } else if layout.sidebar.contains(x, y) {
      if pressed && self.state.scope == Scope::Term {
        self.state.scope = Scope::Tasks;
      }
      let tasks = &mut self.state.tasks;
      match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
          if let Some(row) = tasks.row_at(x, y) {
            tasks.click(row.index);
          }
        }
        MouseEventKind::ScrollDown => tasks.scroll_by(3),
        MouseEventKind::ScrollUp => tasks.scroll_by(-3),
        MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
        | MouseEventKind::Up(_)
        | MouseEventKind::Drag(_)
        | MouseEventKind::Moved
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => (),
      }
    }
  }

  fn handle_key(&mut self, observer: ObserverId, key: Key) {
    match key.kind {
      KeyEventKind::Press | KeyEventKind::Repeat => (),
      KeyEventKind::Release => return,
    }
    if let Some(modal) = &mut self.modal {
      match modal.handle_key(&key) {
        ModalResult::Keep => (),
        ModalResult::Close => self.modal = None,
        ModalResult::Run(action) => {
          self.modal = None;
          self.handle_action(Some(observer), action);
        }
        ModalResult::Detach => {
          self.modal = None;
          self.handle_screen_cmd(TaskScreenCmd::Detach { observer });
        }
      }
      return;
    }
    let group = self.state.keymap_group();
    if let Some(action) = self.keymap.action(group, &key.plain()) {
      self.handle_action(Some(observer), action.clone());
    } else if group == KeymapGroup::Term {
      // Unbound keys go to the process; in copy mode the keymap is the
      // whole vocabulary, so they are dropped rather than fed to the
      // screen's own copy-mode keys.
      self.handle_action(Some(observer), Action::SendKey { key });
    }
  }

  /// Runs an action, retaining the attachment that originated it when it
  /// came from terminal input. A quit from an attachment detaches only that
  /// observer; an out-of-band quit action still addresses the runner.
  fn handle_action(&mut self, observer: Option<ObserverId>, action: Action) {
    let current = self.state.current_task().map(|t| t.id);
    match action {
      Action::Batch { cmds } => {
        for cmd in cmds {
          self.handle_action(observer, cmd);
        }
      }

      Action::ShowQuit => self.modal = Some(Box::new(QuitModal::default())),
      Action::Detach => {
        if let Some(observer) = observer {
          self.handle_screen_cmd(TaskScreenCmd::Detach { observer });
        }
      }
      Action::Down => {
        self.state.quitting = true;
        self.issue(Command::Down);
      }
      Action::Quit => {
        self.state.quitting = true;
        self.issue(Command::Quit);
      }
      Action::Command { command } => self.issue(command),

      Action::ToggleFocus => self.state.scope = self.state.scope.toggle(),
      Action::FocusTasks => self.state.scope = Scope::Tasks,
      Action::FocusTerm => self.state.scope = Scope::Term,
      Action::Zoom => self.state.scope = Scope::TermZoom,

      Action::ShowCommandsMenu => {
        self.modal = Some(Box::new(CommandsMenuModal::new()));
      }
      Action::ShowMainMenu => {
        let anchor = self.state.header.logo.unwrap_or_default();
        self.modal = Some(Box::new(MainMenuModal::new(anchor)));
      }
      // Only a console inside a runner has somewhere to switch from.
      Action::ShowRunnerSelect { tab } => {
        if self.config.runner.is_some() {
          self.modal = Some(Box::new(RunnerSelectModal::new(
            self.config.runner.as_ref(),
            tab,
          )));
        }
      }
      Action::SwitchRunner { kind, root } => match observer {
        Some(observer) => {
          self
            .screen
            .notify(observer, ScreenNotify::Switch { kind, root });
        }
        None => log::warn!("switch-runner needs an attached client"),
      },
      Action::CloseCurrentModal => self.modal = None,
      Action::ToggleKeymapWindow => {
        self.state.hide_keymap_window = !self.state.hide_keymap_window;
      }
      Action::ToggleTheme => {
        self.theme = if self.theme.dark {
          Theme::light()
        } else {
          Theme::dark()
        };
      }

      Action::NextTask => self.state.tasks.next(),
      Action::PrevTask => self.state.tasks.prev(),
      Action::Expand => self.state.tasks.expand(),
      Action::Collapse => self.state.tasks.collapse(),
      Action::SelectTask { index } => self.state.tasks.select_task(index),

      Action::StartTask => {
        self.issue_selected(|target| Command::Start { target })
      }
      Action::StopTask => {
        self.issue_selected(|target| Command::Stop { target })
      }
      Action::KillTask => {
        self.issue_selected(|target| Command::Kill { target })
      }
      Action::VetoTask => {
        self.issue_selected(|target| Command::Veto { target })
      }
      Action::RestartTask => {
        self.issue_selected(|target| Command::Restart { target })
      }
      Action::ForceRestartTask => {
        self.issue_selected(|target| Command::ForceRestart { target })
      }
      Action::RestartAll => {
        self.issue_all(|target| Command::Restart { target })
      }
      Action::ForceRestartAll => {
        self.issue_all(|target| Command::ForceRestart { target })
      }

      Action::ShowAddTask => {
        self.modal = Some(Box::new(AddTaskModal::default()))
      }
      Action::AddTask { cmd, name } => {
        let label =
          self.unique_label(&name.unwrap_or_else(|| cmd.clone()), None);
        // Taken while a task is there or under it.
        let path = unique(&path_name(&label), |path| {
          self
            .state
            .tasks
            .iter()
            .any(|t| t.path.components().next() == Some(path))
        });
        let argv = if self.config.system_shell {
          Ok(crate::parse_shell::system_argv(&cmd))
        } else {
          crate::parse_shell::split_argv(&cmd)
        };
        match (path.parse::<Target>(), argv) {
          (Ok(target), Ok(argv)) => self.issue(Command::Add {
            target,
            label: Some(label),
            cmd: CmdConfig::Cmd { cmd: argv },
            cwd: None,
            env: None,
            deps: Vec::new(),
            tags: Vec::new(),
          }),
          // Back to the prompt with the line, so it can be fixed.
          (_, Err(err)) => {
            self.modal =
              Some(Box::new(AddTaskModal::with_error(cmd, err.to_string())))
          }
          (Err(err), Ok(_)) => log::warn!("Cannot add task '{path}': {err}"),
        }
      }
      Action::DuplicateTask => {
        if let Some(task) = self.state.current_task() {
          let name = self.unique_label(&task.name(), None);
          self.issue(Command::Duplicate {
            target: Target::Id(task.id),
            name: Some(name),
          });
        }
      }
      Action::ShowRenameTask => {
        self.modal = Some(Box::new(RenameTaskModal::default()));
      }
      Action::RenameTask { name } => {
        if let Some(id) = current {
          let name = self.unique_label(&name, Some(id));
          self.issue(Command::Rename {
            target: Target::Id(id),
            name,
          });
        }
      }
      Action::ShowRemoveTask => {
        if let Some(task) = self.state.current_task()
          && !task.is_up()
        {
          self.modal = Some(Box::new(RemoveTaskModal { id: task.id }));
        }
      }

      Action::ScrollUp { n, unit } => {
        self.send_current(
          current,
          TaskScreenCmd::Scroll {
            delta: n.min(i32::MAX as usize) as i32,
            unit: kernel_scroll_unit(unit),
          },
        );
      }
      Action::ScrollDown { n, unit } => {
        self.send_current(
          current,
          TaskScreenCmd::Scroll {
            delta: -(n.min(i32::MAX as usize) as i32),
            unit: kernel_scroll_unit(unit),
          },
        );
      }
      Action::CopyModeEnter => {
        if current.is_some() {
          self.state.scope = Scope::Term;
        }
        self.send_current(current, TaskScreenCmd::CopyEnter);
      }
      Action::CopyModeLeave => {
        self.send_current(current, TaskScreenCmd::CopyLeave)
      }
      Action::CopyModeMove { dir } => self.send_current(
        current,
        TaskScreenCmd::CopyMove {
          dir: kernel_copy_move(dir),
        },
      ),
      Action::CopyModeEnd => {
        self.send_current(current, TaskScreenCmd::CopyBeginSelection)
      }
      Action::CopyModeCopy => {
        self.send_current(current, TaskScreenCmd::CopyYank)
      }
      Action::SendKey { key } => {
        self.input_current(current, TermEvent::Key(key))
      }
    }
  }

  fn unique_label(&self, base: &str, exclude: Option<TaskId>) -> String {
    unique(base, |name| {
      self
        .state
        .tasks
        .iter()
        .any(|t| Some(t.id) != exclude && t.name() == name)
    })
  }

  fn send_current(&self, current: Option<TaskId>, cmd: TaskScreenCmd) {
    if let Some(id) = current {
      self.pc.send_msg(id, cmd);
    }
  }

  fn input_current(&self, current: Option<TaskId>, event: TermEvent) {
    self.send_current(
      current,
      TaskScreenCmd::Input {
        observer: self.observer,
        event,
      },
    );
  }

  fn issue(&self, command: Command) {
    issue(&self.pc, &self.config, command);
  }

  /// Runs a command on the selected task or every task in the selected group.
  fn issue_selected(&self, make: fn(Target) -> Command) {
    let target = match self.state.tasks.selected() {
      Some((_, Node::Task(task))) => Target::Id(task.id),
      Some((path, Node::Group(_))) => Target::glob(&format!("{path}/**")),
      None => return,
    };
    self.issue(make(target));
  }

  fn issue_all(&self, make: fn(Target) -> Command) {
    let commands = self
      .state
      .tasks
      .iter()
      .map(|t| make(Target::Id(t.id)))
      .collect();
    self.issue(Command::Batch { commands });
  }

  fn handle_screen_notify(&mut self, notify: ScreenNotify) {
    match notify {
      // Only the shown task's screen is part of the picture.
      ScreenNotify::Render { screen } => {
        if self.shown_task() == Some(screen) {
          self.dirty = true;
        }
      }
      ScreenNotify::Attached => self.dirty = true,
      // Addressed to attach sessions, never to a screen's observer.
      ScreenNotify::Bell | ScreenNotify::Switch { .. } => (),
      ScreenNotify::CopyPresent { screen, handle } => {
        if let Some(task) = self.state.tasks.task_mut(screen) {
          task.present = handle;
          self.dirty = true;
        }
      }
      // Copied on behalf of whoever is attached here.
      ScreenNotify::Yank { text } => {
        let attached = self.screen.has_observers();
        self.screen.yank(text, &mut self.effects);
        self.apply_screen(attached);
      }
    }
  }

  /// The task whose terminal the console shows.
  fn shown_task(&self) -> Option<TaskId> {
    match self.state.tasks.selected() {
      Some((_, Node::Task(task))) => Some(task.id),
      _ => None,
    }
  }

  fn handle_task_notify(&mut self, id: TaskId, notify: TaskNotify) {
    match notify {
      TaskNotify::Added {
        path,
        label,
        after,
        kind,
        state,
        vt,
      } => self.add_task(id, label, path, after, kind, state, vt),
      TaskNotify::StateChanged(state) => self.state.tasks.set_status(id, state),
      TaskNotify::Removed => self.state.tasks.remove(id),
      TaskNotify::LabelChanged(label) => {
        if let Some(task) = self.state.tasks.task_mut(id) {
          task.label = label;
        }
      }
    }
  }
}

/// Dims everything but `keep`.
fn shade(grid: &mut Grid, theme: &BlockTheme, keep: Option<Rect>) {
  let area = grid.area();
  let dim = |style: Style| Style {
    fg: style.fg.blend(theme.shade, theme.shade_amount),
    bg: style.bg.blend(theme.shade, theme.shade_amount),
    ..style
  };
  let Some(keep) = keep.map(|keep| keep.intersect(area)) else {
    grid.map_styles(area, dim);
    return;
  };
  // The four bands around `keep`.
  let above = Rect::new(area.x, area.y, area.width, keep.y - area.y);
  let below = Rect::new(
    area.x,
    keep.bottom(),
    area.width,
    area.bottom() - keep.bottom(),
  );
  let left = Rect::new(area.x, keep.y, keep.x - area.x, keep.height);
  let right = Rect::new(
    keep.right(),
    keep.y,
    area.right() - keep.right(),
    keep.height,
  );
  for band in [above, below, left, right] {
    if band.width > 0 && band.height > 0 {
      grid.map_styles(band, dim);
    }
  }
}
