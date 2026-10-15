//! The client's own terminal: raw mode, the modes dekit needs, the probe
//! of what the terminal can show, and the stream of events it sends.

use std::collections::VecDeque;
use std::io::Write;
use std::pin::Pin;
use std::time::Duration;

mod input;
pub(crate) mod internal;
#[cfg(windows)]
mod windows;

use crate::protocol::caps::{GraphicsCaps, TermCaps, WidthPolicy};
use crate::term::modes::{KITTY_ALL_FLAGS, KITTY_TEXT};
use crate::{
  error::ResultLogger,
  term::{Geometry, Size, TermEvent, color::ColorDepth, vt::emit},
};

use self::{
  input::EventDecoder,
  internal::{InternalTermEvent, KeyboardMode},
};

/// How long the probe waits for the terminal's answers before deciding
/// it is a terminal that answers nothing.
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Kitty keyboard flags pushed on a terminal that has the protocol:
/// everything the runner's programs may ask for except associated text,
/// which legacy programs cannot use.
const KITTY_FLAGS_PUSHED: u8 = KITTY_ALL_FLAGS & !KITTY_TEXT;

/// The id of the image the graphics probe transmits; never shown.
const PROBE_IMAGE_ID: u32 = 31;

pub struct TermDriver {
  #[cfg(unix)]
  stdin_fd: rustix::fd::BorrowedFd<'static>,
  #[cfg(unix)]
  orig_termios: rustix::termios::Termios,
  #[cfg(unix)]
  stdin_thread: Option<std::thread::JoinHandle<()>>,
  #[cfg(unix)]
  stdin_wakeup: std::os::fd::OwnedFd,
  #[cfg(unix)]
  sigwinch: tokio::signal::unix::Signal,

  #[cfg(windows)]
  win_vt: windows::WinVt,

  events:
    tokio::sync::mpsc::UnboundedReceiver<std::io::Result<InternalTermEvent>>,

  stdout: std::io::Stdout,

  pending: VecDeque<InternalTermEvent>,
  /// Set while the probe's answers are awaited; a terminal that never
  /// answers is given up on when it fires.
  init_timeout: Option<Pin<Box<tokio::time::Sleep>>>,
  keyboard: KeyboardMode,
  probe: Probe,
  caps: TermCaps,
  /// The size of a cell in pixels: from the probe, then from each resize
  /// whose winsize says, since a zoom changes it with the window.
  cell_pixels: Option<(u16, u16)>,
  /// The probe finished and nobody asked yet (`take_probe_finished`).
  probe_finished: bool,
}

/// What the terminal answered so far.
#[derive(Default)]
struct Probe {
  done: bool,
  kitty_keyboard: bool,
  /// Columns the three width probes took, in order: VS16 emoji, flag,
  /// ZWJ pair.
  widths: Vec<u16>,
  cell_pixels: Option<(u16, u16)>,
  text_pixels: Option<(u16, u16)>,
  sync_output: Option<bool>,
  graphics: bool,
  da1: Option<Vec<u32>>,
  /// XTVERSION's answer: the terminal's name and version.
  terminal: Option<String>,
}

impl TermDriver {
  pub fn create() -> anyhow::Result<Self> {
    #[cfg(unix)]
    let stdin_fd = rustix::stdio::stdin();
    #[cfg(unix)]
    if !rustix::termios::isatty(stdin_fd) {
      anyhow::bail!("Stdin is not a tty.");
    }

    #[cfg(windows)]
    let win_vt = windows::WinVt::enable()?;

    let mut stdout = std::io::stdout();

    #[cfg(unix)]
    let orig_termios = rustix::termios::tcgetattr(stdin_fd)?;
    #[cfg(unix)]
    let mut termios = orig_termios.clone();
    #[cfg(unix)]
    termios.make_raw();
    #[cfg(unix)]
    rustix::termios::tcsetattr(
      stdin_fd,
      rustix::termios::OptionalActions::Now,
      &termios,
    )?;

    let mut seq: Vec<u8> = Vec::new();
    seq.extend_from_slice(emit::SAVE_CURSOR.as_bytes());
    emit::dec_set(&mut seq, emit::DecMode::AltScreen);
    seq.extend_from_slice(emit::CLEAR_ALL.as_bytes());
    // Mouse: press/release, drag motion, all motion, and the rxvt/SGR
    // encodings that allow coordinates over 223 (SGR preferred).
    emit::dec_set(&mut seq, emit::DecMode::MousePressRelease);
    emit::dec_set(&mut seq, emit::DecMode::MouseButtonMotion);
    emit::dec_set(&mut seq, emit::DecMode::MouseAnyMotion);
    emit::dec_set(&mut seq, emit::DecMode::MouseRxvt);
    emit::dec_set(&mut seq, emit::DecMode::MouseSgr);
    emit::dec_set(&mut seq, emit::DecMode::BracketedPaste);
    emit::dec_set(&mut seq, emit::DecMode::FocusEvents);
    write_probes(&mut seq);
    stdout.write_all(&seq)?;
    stdout.flush()?;

    #[cfg(unix)]
    let sigwinch = tokio::signal::unix::signal(
      tokio::signal::unix::SignalKind::window_change(),
    )
    .expect("Failed to register SIGWINCH handler");

    #[cfg(unix)]
    let (events, stdin_thread, stdin_wakeup) = {
      use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

      let (sender, events) = tokio::sync::mpsc::unbounded_channel();
      let mut pipe_fds = [0; 2];
      unsafe {
        if libc::pipe(pipe_fds.as_mut_ptr()) < 0 {
          return Err(std::io::Error::last_os_error().into());
        }
      }

      let wake_read = unsafe { OwnedFd::from_raw_fd(pipe_fds[0]) };
      let wake_write = unsafe { OwnedFd::from_raw_fd(pipe_fds[1]) };
      let stdin_raw = stdin_fd.as_raw_fd();

      let stdin_thread = std::thread::spawn(move || {
        let mut decoder = EventDecoder::new();
        let mut read_buf = [0u8; 4096];
        // A lone ESC is only an Esc key press if nothing follows it
        // shortly; escape sequences may be split across reads.
        const ESC_TIMEOUT_MS: i32 = 25;

        loop {
          let mut poll_fds = [
            libc::pollfd {
              fd: stdin_raw,
              events: libc::POLLIN,
              revents: 0,
            },
            libc::pollfd {
              fd: wake_read.as_raw_fd(),
              events: libc::POLLIN,
              revents: 0,
            },
          ];

          let timeout = if decoder.esc_pending() {
            ESC_TIMEOUT_MS
          } else {
            -1
          };
          // Note: tty stdin can only be awaited with select/poll on Macos.
          let poll_result =
            unsafe { libc::poll(poll_fds.as_mut_ptr(), 2, timeout) };
          if poll_result == 0 {
            decoder.flush(|event| {
              let _ = sender.send(Ok(event));
            });
            continue;
          }
          if poll_result < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
              continue;
            }
            let _ = sender.send(Err(err));
            break;
          }

          if (poll_fds[1].revents & libc::POLLIN) != 0 {
            break;
          }

          if (poll_fds[0].revents & (libc::POLLIN | libc::POLLHUP)) != 0 {
            let n = match rustix::io::read(stdin_fd, &mut read_buf) {
              Ok(n) => n,
              Err(err) => {
                let io_err = std::io::Error::from(err);
                if io_err.kind() == std::io::ErrorKind::Interrupted {
                  continue;
                }
                let _ = sender.send(Err(io_err));
                break;
              }
            };

            if n == 0 {
              break;
            }

            decoder.feed(&read_buf[..n], |event| {
              let _ = sender.send(Ok(event));
            });
          }
        }
      });
      (events, Some(stdin_thread), wake_write)
    };

    #[cfg(windows)]
    let events = {
      let (sender, events) = tokio::sync::mpsc::unbounded_channel();
      unsafe {
        std::thread::spawn(move || {
          let stdin = match ::windows::Win32::System::Console::GetStdHandle(
            ::windows::Win32::System::Console::STD_INPUT_HANDLE,
          ) {
            Ok(stdin) => stdin,
            Err(err) => {
              log::error!("GetStdHandle error: {}", err);
              return;
            }
          };
          let mut decoder = EventDecoder::new();
          let mut buf =
            [::windows::Win32::System::Console::INPUT_RECORD::default(); 128];
          loop {
            let mut count = 0;
            match ::windows::Win32::System::Console::ReadConsoleInputA(
              stdin, &mut buf, &mut count,
            ) {
              Ok(()) => (),
              Err(err) => {
                log::error!("ReadConsoleInputA error: {}", err);
                break;
              }
            };

            windows::decode_input_records(
              &mut decoder,
              &buf[..count as usize],
              &mut |event| {
                let _ = sender.send(Ok(event));
              },
            );
          }
        });
      };
      events
    };

    Ok(Self {
      #[cfg(unix)]
      stdin_fd,
      #[cfg(unix)]
      orig_termios,
      #[cfg(unix)]
      stdin_thread,
      #[cfg(unix)]
      stdin_wakeup,
      #[cfg(unix)]
      sigwinch,

      #[cfg(windows)]
      win_vt,

      events,

      stdout,
      pending: VecDeque::new(),
      init_timeout: Some(Box::pin(tokio::time::sleep(PROBE_TIMEOUT))),
      keyboard: KeyboardMode::Unknown,
      probe: Probe::default(),
      probe_finished: false,
      caps: TermCaps::default(),
      cell_pixels: None,
    })
  }

  /// What the terminal can show, once `probe` has run (or the probe's
  /// answers arrived through `input`); the assumed defaults before.
  pub fn caps(&self) -> &TermCaps {
    &self.caps
  }

  /// The terminal's size, and the size of a cell in pixels when it said:
  /// measured by the probe, and again by each resize whose winsize
  /// carries pixels.
  pub fn geometry(&self) -> std::io::Result<Geometry> {
    Ok(Geometry {
      size: self.size()?,
      cell_pixels: self.cell_pixels,
    })
  }

  /// Waits for the terminal's answers to the probes written at creation,
  /// at most `PROBE_TIMEOUT`. Events that are not answers wait their
  /// turn for `input`.
  pub async fn probe(&mut self) -> std::io::Result<()> {
    while !self.probe.done {
      let timeout = async {
        match &mut self.init_timeout {
          Some(sleep) => sleep.await,
          None => std::future::pending().await,
        }
      };
      tokio::select! {
        event = self.events.recv() => match event {
          Some(Ok(event)) => {
            if !self.probe_reply(&event)? {
              self.pending.push_back(event);
            }
          }
          Some(Err(err)) => return Err(err),
          None => {
            self.finish_probe()?;
            return Ok(());
          }
        },
        _ = timeout => {
          self.finish_probe()?;
        }
      }
    }
    Ok(())
  }

  /// Whether the probe finished since this was last asked: the caps are
  /// known now, and the first row was cleared behind the painter's back.
  pub fn take_probe_finished(&mut self) -> bool {
    std::mem::take(&mut self.probe_finished)
  }

  /// Takes a probe reply; false when the event is something else.
  fn probe_reply(
    &mut self,
    event: &InternalTermEvent,
  ) -> std::io::Result<bool> {
    if self.probe.done {
      return Ok(false);
    }
    match event {
      InternalTermEvent::CursorPos(x, _) => self.probe.widths.push(*x),
      InternalTermEvent::PrimaryDeviceAttributes(params) => {
        self.probe.da1 = Some(params.clone());
        self.finish_probe()?;
      }
      InternalTermEvent::ReplyKittyKeyboard(_) => {
        self.probe.kitty_keyboard = true;
      }
      InternalTermEvent::WindowReport(kind @ (4 | 6), h, w) => {
        if *w > 0 && *h > 0 {
          let pixels = Some((
            (*w).min(u16::MAX as u32) as u16,
            (*h).min(u16::MAX as u32) as u16,
          ));
          if *kind == 6 {
            self.probe.cell_pixels = pixels;
          } else {
            self.probe.text_pixels = pixels;
          }
        }
      }
      InternalTermEvent::WindowReport(..) => (),
      InternalTermEvent::ModeReport {
        dec: true,
        mode: 2026,
        state,
      } => {
        self.probe.sync_output = Some(matches!(state, 1 | 2));
      }
      InternalTermEvent::ModeReport { .. } => (),
      InternalTermEvent::GraphicsReply(body) => {
        let text = String::from_utf8_lossy(body);
        if text.contains("OK") && text.contains(&format!("i={PROBE_IMAGE_ID}"))
        {
          self.probe.graphics = true;
        }
      }
      InternalTermEvent::DcsReply(body) => {
        if let Some(name) = body.strip_prefix(b">|") {
          self.probe.terminal =
            Some(String::from_utf8_lossy(name).into_owned());
        }
      }
      _ => return Ok(false),
    }
    Ok(true)
  }

  /// Decides the capabilities from the answers so far and sets the
  /// keyboard protocol up.
  fn finish_probe(&mut self) -> std::io::Result<()> {
    if self.probe.done {
      return Ok(());
    }
    self.probe.done = true;
    self.probe_finished = true;
    self.init_timeout = None;
    let fallback = Size {
      width: 80,
      height: 24,
    };
    #[cfg(unix)]
    let winsize = self.winsize().ok();
    #[cfg(unix)]
    let size = winsize.as_ref().map(size_of).unwrap_or(fallback);
    #[cfg(windows)]
    let size = self.size().unwrap_or(fallback);
    let cell_pixels = self.probe.cell_pixels.or_else(|| {
      self
        .probe
        .text_pixels
        .map(|(w, h)| (w / size.width.max(1), h / size.height.max(1)))
    });
    #[cfg(unix)]
    let cell_pixels =
      cell_pixels.or_else(|| winsize.as_ref().and_then(cell_pixels_of));
    self.cell_pixels = cell_pixels;
    self.caps = caps_from_probe(&self.probe, &Env::read());

    let mut seq = Vec::new();
    if self.probe.kitty_keyboard {
      self.keyboard = KeyboardMode::Kitty;
      emit::kitty_push(&mut seq, KITTY_FLAGS_PUSHED);
    } else {
      #[cfg(unix)]
      {
        self.keyboard = KeyboardMode::ModifyOtherKeys;
        emit::modify_other_keys(&mut seq, 2);
      }
      #[cfg(windows)]
      {
        self.keyboard = KeyboardMode::Win32;
        emit::dec_set(&mut seq, emit::DecMode::Win32Input);
      }
    }
    // Whatever the width probes left on the first row.
    emit::cup(&mut seq, 0, 0);
    seq.extend_from_slice(b"\x1b[2K");
    self.stdout.write_all(&seq)?;
    self.stdout.flush()?;
    Ok(())
  }

  fn handle_internal(
    &mut self,
    event: InternalTermEvent,
  ) -> std::io::Result<Option<TermEvent>> {
    if !self.probe.done && self.probe_reply(&event)? {
      return Ok(None);
    }
    match event {
      InternalTermEvent::Key(key_event) => {
        return Ok(Some(TermEvent::Key(key_event)));
      }
      InternalTermEvent::Mouse(mouse_event) => {
        return Ok(Some(TermEvent::Mouse(mouse_event)));
      }
      InternalTermEvent::Paste(text) => {
        return Ok(Some(TermEvent::Paste(text)));
      }
      InternalTermEvent::Resize(cols, rows) => {
        return Ok(Some(TermEvent::Resize(Geometry {
          size: Size {
            width: cols,
            height: rows,
          },
          cell_pixels: self.cell_pixels,
        })));
      }
      InternalTermEvent::FocusGained => {
        return Ok(Some(TermEvent::FocusGained));
      }
      InternalTermEvent::FocusLost => return Ok(Some(TermEvent::FocusLost)),
      // Late answers to probes, and reports nobody asked for.
      InternalTermEvent::CursorPos(..)
      | InternalTermEvent::PrimaryDeviceAttributes(_)
      | InternalTermEvent::ReplyKittyKeyboard(_)
      | InternalTermEvent::WindowReport(..)
      | InternalTermEvent::ModeReport { .. }
      | InternalTermEvent::GraphicsReply(_)
      | InternalTermEvent::DcsReply(_) => (),
    };
    Ok(None)
  }

  #[cfg(unix)]
  pub async fn input(&mut self) -> std::io::Result<Option<TermEvent>> {
    loop {
      // Drain buffered events first.
      while let Some(event) = self.pending.pop_front() {
        if let Some(term_event) = self.handle_internal(event)? {
          return Ok(Some(term_event));
        }
      }

      tokio::select! {
        event = self.events.recv() => {
          match event {
            Some(Ok(event)) => {
              if let Some(term_event) = self.handle_internal(event)? {
                return Ok(Some(term_event));
              }
            }
            Some(Err(err)) => return Err(err),
            None => return Ok(None),
          }
        }
        _ = self.sigwinch.recv() => {
          let winsize = self.winsize()?;
          // A zoom changes the cell size with the window; a terminal
          // that fills in the pixel fields says so here.
          if let Some(cell) = cell_pixels_of(&winsize) {
            self.cell_pixels = Some(cell);
          }
          return Ok(Some(TermEvent::Resize(Geometry {
            size: size_of(&winsize),
            cell_pixels: self.cell_pixels,
          })));
        }
        _ = async {
          match &mut self.init_timeout {
            Some(sleep) => sleep.await,
            None => std::future::pending().await,
          }
        } => {
          self.finish_probe()?;
        }
      }
    }
  }

  #[cfg(windows)]
  pub async fn input(&mut self) -> std::io::Result<Option<TermEvent>> {
    loop {
      // Drain buffered events first.
      while let Some(event) = self.pending.pop_front() {
        if let Some(term_event) = self.handle_internal(event)? {
          return Ok(Some(term_event));
        }
      }

      tokio::select! {
        event = self.events.recv() => {
          match event {
            Some(Ok(event)) => {
              if let Some(term_event) = self.handle_internal(event)? {
                return Ok(Some(term_event));
              }
            }
            Some(Err(err)) => return Err(err),
            None => return Ok(None),
          }
        }
        _ = async {
          match &mut self.init_timeout {
            Some(sleep) => sleep.await,
            None => std::future::pending().await,
          }
        } => {
          self.finish_probe()?;
        }
      }
    }
  }

  #[cfg(unix)]
  fn winsize(&self) -> std::io::Result<rustix::termios::Winsize> {
    Ok(rustix::termios::tcgetwinsize(self.stdin_fd)?)
  }

  #[cfg(unix)]
  pub fn size(&self) -> std::io::Result<Size> {
    self.winsize().map(|ws| size_of(&ws))
  }

  #[cfg(windows)]
  pub fn size(&self) -> std::io::Result<Size> {
    unsafe {
      use std::os::windows::io::AsRawHandle;

      use ::windows::Win32::{
        Foundation::HANDLE,
        System::Console::{
          CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo,
        },
      };

      let mut csbi: CONSOLE_SCREEN_BUFFER_INFO = Default::default();

      GetConsoleScreenBufferInfo(
        HANDLE(self.stdout.as_raw_handle()),
        &mut csbi,
      )?;

      Ok(Size {
        height: (csbi.srWindow.Bottom - csbi.srWindow.Top + 1) as u16,
        width: (csbi.srWindow.Right - csbi.srWindow.Left + 1) as u16,
      })
    }
  }
}

/// The questions put to the terminal at startup. Every answer comes
/// before DA1's, which every terminal gives, so DA1 ends the probe.
fn write_probes(seq: &mut Vec<u8>) {
  #[cfg(unix)]
  {
    // Kitty keyboard protocol (not queried on Windows, issue #215).
    seq.extend_from_slice(emit::KITTY_QUERY.as_bytes());
    // Who the terminal is.
    seq.extend_from_slice(b"\x1b[>0q");
    // Synchronized output.
    seq.extend_from_slice(b"\x1b[?2026$p");
    // Cell and text area size in pixels.
    seq.extend_from_slice(b"\x1b[16t\x1b[14t");
    // Kitty graphics: a 1x1 RGB query image.
    emit::kitty_graphics(
      seq,
      &format!("i={PROBE_IMAGE_ID},s=1,v=1,a=q,t=d,f=24"),
      b"AAAA",
    );
    // Widths the terminal gives text dekit measures itself: an emoji
    // with VS16, a flag, a ZWJ pair. The cursor column after each tells.
    for probe in [
      "\u{2764}\u{FE0F}",
      "\u{1F1FA}\u{1F1F8}",
      "\u{1F468}\u{200D}\u{1F469}",
    ] {
      seq.extend_from_slice(b"\x1b[1;1H\x1b[2K");
      seq.extend_from_slice(probe.as_bytes());
      seq.extend_from_slice(b"\x1b[6n");
    }
    seq.extend_from_slice(b"\x1b[1;1H\x1b[2K");
  }
  seq.extend_from_slice(emit::DA1_QUERY.as_bytes());
}

/// Environment the terminal describes itself with.
struct Env {
  term: String,
  term_program: String,
  colorterm: String,
}

impl Env {
  fn read() -> Env {
    Env {
      term: std::env::var("TERM").unwrap_or_default(),
      term_program: std::env::var("TERM_PROGRAM").unwrap_or_default(),
      colorterm: std::env::var("COLORTERM").unwrap_or_default(),
    }
  }
}

#[cfg(unix)]
fn size_of(ws: &rustix::termios::Winsize) -> Size {
  Size {
    width: ws.ws_col,
    height: ws.ws_row,
  }
}

/// The cell size a winsize implies, when the terminal fills in pixels.
#[cfg(unix)]
fn cell_pixels_of(ws: &rustix::termios::Winsize) -> Option<(u16, u16)> {
  (ws.ws_xpixel > 0 && ws.ws_ypixel > 0 && ws.ws_col > 0 && ws.ws_row > 0)
    .then(|| (ws.ws_xpixel / ws.ws_col, ws.ws_ypixel / ws.ws_row))
}

fn caps_from_probe(probe: &Probe, env: &Env) -> TermCaps {
  let name = probe
    .terminal
    .as_deref()
    .unwrap_or(&env.term_program)
    .to_ascii_lowercase();
  let is = |needle: &str| name.contains(needle);
  let modern = probe.kitty_keyboard
    || probe.sync_output == Some(true)
    || probe.graphics
    || is("kitty")
    || is("ghostty")
    || is("wezterm")
    || is("foot")
    || is("contour")
    || is("iterm")
    || is("rio")
    || is("alacritty")
    || is("konsole")
    || is("vscode");
  let dumb_term = matches!(
    env.term.as_str(),
    "linux" | "dumb" | "vt100" | "vt102" | "vt220" | "ansi"
  );
  let colors = if dumb_term && env.colorterm.is_empty() {
    ColorDepth::Ansi16
  } else {
    ColorDepth::TrueColor
  };
  let apple = is("apple_terminal") || env.term_program == "Apple_Terminal";
  let widths = if probe.widths.len() >= 3 {
    WidthPolicy {
      vs16_wide: probe.widths[0] == 2,
      flags_wide: probe.widths[1] == 2,
      zwj_joins: probe.widths[2] == 2,
    }
  } else {
    WidthPolicy::default()
  };
  let kitty_graphics = probe.graphics.then(|| GraphicsCaps {
    placeholders: is("kitty") || is("ghostty") || is("konsole"),
    animation: is("kitty"),
  });
  TermCaps {
    colors,
    underline_styles: modern && !apple,
    strikethrough: true,
    hyperlinks: !dumb_term && !apple,
    sync_output: probe.sync_output == Some(true),
    kitty_keyboard: if probe.kitty_keyboard {
      KITTY_FLAGS_PUSHED
    } else {
      0
    },
    kitty_graphics,
    sixel: probe
      .da1
      .as_ref()
      .is_some_and(|params| params.iter().skip(1).any(|p| *p == 4)),
    focus_events: true,
    widths,
  }
}

impl Drop for TermDriver {
  fn drop(&mut self) {
    let mut seq: Vec<u8> = Vec::new();
    match self.keyboard {
      KeyboardMode::Unknown => (),
      KeyboardMode::ModifyOtherKeys => emit::modify_other_keys(&mut seq, 0),
      KeyboardMode::Kitty => seq.extend_from_slice(emit::KITTY_POP.as_bytes()),
      KeyboardMode::Win32 => {
        emit::dec_reset(&mut seq, emit::DecMode::Win32Input)
      }
    }
    // Images the runner placed on this screen go with it.
    if self.caps.kitty_graphics.is_some() {
      emit::kitty_graphics(&mut seq, "a=d,d=A,q=2", b"");
    }

    emit::dec_reset(&mut seq, emit::DecMode::FocusEvents);
    emit::dec_reset(&mut seq, emit::DecMode::BracketedPaste);
    emit::dec_reset(&mut seq, emit::DecMode::MouseSgr);
    emit::dec_reset(&mut seq, emit::DecMode::MouseRxvt);
    emit::dec_reset(&mut seq, emit::DecMode::MouseAnyMotion);
    emit::dec_reset(&mut seq, emit::DecMode::MouseButtonMotion);
    emit::dec_reset(&mut seq, emit::DecMode::MousePressRelease);
    emit::dec_reset(&mut seq, emit::DecMode::AltScreen);

    // Save/Restore does not work on tmux. So we just show cursor.
    emit::dec_set(&mut seq, emit::DecMode::ShowCursor);
    seq.extend_from_slice(emit::RESTORE_CURSOR.as_bytes());

    self.stdout.write_all(&seq).log_ignore();
    self.stdout.flush().log_ignore();

    #[cfg(unix)]
    rustix::io::write(&self.stdin_wakeup, &[0]).log_ignore();

    #[cfg(unix)]
    if let Some(stdin_thread) = self.stdin_thread.take() {
      stdin_thread.join().ok();
    }

    #[cfg(unix)]
    rustix::termios::tcsetattr(
      self.stdin_fd,
      rustix::termios::OptionalActions::Now,
      &self.orig_termios,
    )
    .log_ignore();

    #[cfg(windows)]
    self.win_vt.disable();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn env(term: &str, program: &str) -> Env {
    Env {
      term: term.to_string(),
      term_program: program.to_string(),
      colorterm: String::new(),
    }
  }

  #[test]
  fn caps_follow_the_answers() {
    let probe = Probe {
      done: true,
      kitty_keyboard: true,
      widths: vec![2, 2, 2],
      cell_pixels: Some((10, 20)),
      text_pixels: None,
      sync_output: Some(true),
      graphics: true,
      da1: Some(vec![62, 4, 22]),
      terminal: Some("kitty(0.40.0)".to_string()),
    };
    let caps = caps_from_probe(&probe, &env("xterm-kitty", ""));
    assert_eq!(caps.kitty_keyboard, 15);
    assert!(caps.sync_output);
    assert!(caps.underline_styles);
    assert!(caps.sixel);
    assert_eq!(caps.widths, WidthPolicy::agreed());
    let graphics = caps.kitty_graphics.unwrap();
    assert!(graphics.placeholders && graphics.animation);

    // A terminal that answered nothing: safe assumptions.
    let silent = Probe::default();
    let caps =
      caps_from_probe(&silent, &env("xterm-256color", "Apple_Terminal"));
    assert_eq!(caps.kitty_keyboard, 0);
    assert!(!caps.sync_output);
    assert!(!caps.underline_styles);
    assert!(!caps.hyperlinks);
    assert!(caps.kitty_graphics.is_none());
    assert_eq!(caps.widths, WidthPolicy::default());
    assert_eq!(caps.colors, ColorDepth::TrueColor);

    let caps = caps_from_probe(&silent, &env("linux", ""));
    assert_eq!(caps.colors, ColorDepth::Ansi16);
    assert!(!caps.hyperlinks);
  }

  #[test]
  fn probes_end_with_da1() {
    let mut seq = Vec::new();
    write_probes(&mut seq);
    assert!(seq.ends_with(emit::DA1_QUERY.as_bytes()));
  }
}
