use std::sync::Arc;

use base64::Engine as _;

use super::*;
use crate::protocol::TermCaps;
use crate::term::{
  Painter,
  image::{Format, SrcRect, placeholder},
};

fn screen(width: u16, height: u16) -> Screen {
  let mut s = Screen::new(Size { height, width }, 20);
  s.set_cell_pixels(10, 20);
  s
}

fn feed(screen: &mut Screen, bytes: &[u8]) -> Vec<Vec<u8>> {
  screen.process(bytes);
  let mut events = Vec::new();
  screen.take_events(&mut events);
  events
    .into_iter()
    .filter_map(|e| match e {
      VtEvent::Reply(bytes) => Some(bytes),
      _ => None,
    })
    .collect()
}

fn b64(data: &[u8]) -> String {
  base64::engine::general_purpose::STANDARD.encode(data)
}

/// A 2x1 RGB image: a red and a green pixel.
fn rgb_2x1() -> Vec<u8> {
  vec![255, 0, 0, 0, 255, 0]
}

#[test]
fn transmit_and_display_places_at_the_cursor() {
  let mut s = screen(20, 5);
  feed(&mut s, b"\x1b[2;3H");
  let replies = feed(
    &mut s,
    format!("\x1b_Ga=T,f=24,s=2,v=1,i=1;{}\x1b\\", b64(&rgb_2x1())).as_bytes(),
  );
  assert_eq!(replies, vec![b"\x1b_Gi=1;OK\x1b\\".to_vec()]);
  let image = s.images().get(1).unwrap();
  assert_eq!(image.data.format, Format::Rgb);
  assert_eq!((image.data.width, image.data.height), (2, 1));
  let frame = s.frame();
  assert_eq!(frame.placements.len(), 1);
  let p = &frame.placements[0];
  assert_eq!((p.row, p.col), (1, 2));
  // 2x1 pixels in 10x20 cells: one cell.
  assert_eq!((p.cols, p.rows), (1, 1));
  assert_eq!(
    p.src,
    SrcRect {
      x: 0,
      y: 0,
      w: 2,
      h: 1
    }
  );
  assert!(frame.images.contains_key(&p.key));
  // The cursor moved past the image.
  assert_eq!(s.cursor_position(), (1, 3));

  // Sized in cells, placed again by id, the cursor staying put.
  let replies =
    feed(&mut s, b"\x1b[4;1H\x1b_Ga=p,i=1,c=4,r=2,p=9,C=1,q=1\x1b\\");
  assert!(replies.is_empty(), "q=1 suppresses OK");
  let frame = s.frame();
  assert_eq!(frame.placements.len(), 2);
  let p = &frame.placements[1];
  assert_eq!((p.row, p.col, p.cols, p.rows), (3, 0, 4, 2));
  assert_eq!(s.cursor_position(), (3, 0));
}

#[test]
fn chunked_transmission_replies_once() {
  let mut s = screen(10, 3);
  let data = b64(&rgb_2x1());
  let (first, second) = data.split_at(4);
  let replies = feed(
    &mut s,
    format!("\x1b_Ga=t,f=24,s=2,v=1,i=5,m=1;{first}\x1b\\").as_bytes(),
  );
  assert!(replies.is_empty());
  assert!(s.images().get(5).is_none());
  let replies = feed(&mut s, format!("\x1b_Gm=0;{second}\x1b\\").as_bytes());
  assert_eq!(replies, vec![b"\x1b_Gi=5;OK\x1b\\".to_vec()]);
  assert!(s.images().get(5).is_some());
  assert!(s.frame().placements.is_empty(), "a=t does not display");
}

#[test]
fn queries_numbers_and_errors() {
  let mut s = screen(10, 3);
  let replies = feed(
    &mut s,
    format!("\x1b_Ga=q,f=24,s=2,v=1,i=31;{}\x1b\\", b64(&rgb_2x1())).as_bytes(),
  );
  assert_eq!(replies, vec![b"\x1b_Gi=31;OK\x1b\\".to_vec()]);
  assert!(s.images().get(31).is_none());

  let replies = feed(&mut s, b"\x1b_Ga=p,i=77\x1b\\");
  assert_eq!(
    replies,
    vec![b"\x1b_Gi=77;ENOENT:no such image\x1b\\".to_vec()]
  );
  let replies = feed(
    &mut s,
    format!("\x1b_Ga=t,f=24,s=3,v=1,i=2;{}\x1b\\", b64(&rgb_2x1())).as_bytes(),
  );
  assert_eq!(
    replies,
    vec![b"\x1b_Gi=2;EINVAL:data size does not match\x1b\\".to_vec()]
  );
  // By number: the terminal picks the id and says it.
  let replies = feed(
    &mut s,
    format!("\x1b_Ga=t,f=24,s=2,v=1,I=4;{}\x1b\\", b64(&rgb_2x1())).as_bytes(),
  );
  let reply = String::from_utf8(replies[0].clone()).unwrap();
  assert!(reply.starts_with("\x1b_Gi="), "{reply:?}");
  assert!(reply.contains(",I=4;OK"), "{reply:?}");
  let replies = feed(&mut s, b"\x1b_Ga=p,I=4,q=1\x1b\\");
  assert!(replies.is_empty());
  assert_eq!(s.frame().placements.len(), 1);

  // A PNG header is enough to size a PNG.
  let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
  png.extend_from_slice(&13u32.to_be_bytes());
  png.extend_from_slice(b"IHDR");
  png.extend_from_slice(&30u32.to_be_bytes());
  png.extend_from_slice(&40u32.to_be_bytes());
  png.extend_from_slice(&[8, 6, 0, 0, 0]);
  let replies = feed(
    &mut s,
    format!("\x1b_Ga=T,f=100,i=8;{}\x1b\\", b64(&png)).as_bytes(),
  );
  assert_eq!(replies, vec![b"\x1b_Gi=8;OK\x1b\\".to_vec()]);
  let image = s.images().get(8).unwrap();
  assert_eq!((image.data.width, image.data.height), (30, 40));
  let p = s.frame().placements.last().cloned().unwrap();
  assert_eq!((p.cols, p.rows), (3, 2));
}

#[test]
fn deletions() {
  let mut s = screen(20, 6);
  let data = b64(&rgb_2x1());
  feed(
    &mut s,
    format!("\x1b_Ga=T,f=24,s=2,v=1,i=1,p=1,q=2;{data}\x1b\\").as_bytes(),
  );
  feed(
    &mut s,
    format!("\x1b[3;5H\x1b_Ga=T,f=24,s=2,v=1,i=2,q=2;{data}\x1b\\").as_bytes(),
  );
  assert_eq!(s.frame().placements.len(), 2);
  // At a cell.
  feed(&mut s, b"\x1b_Ga=d,d=p,x=5,y=3\x1b\\");
  assert_eq!(s.frame().placements.len(), 1);
  assert!(s.images().get(2).is_some(), "lowercase keeps the data");
  // By id, freeing it.
  feed(&mut s, b"\x1b_Ga=d,d=I,i=1\x1b\\");
  assert!(s.frame().placements.is_empty());
  assert!(s.images().get(1).is_none());
  // Everything.
  feed(
    &mut s,
    format!("\x1b_Ga=T,f=24,s=2,v=1,i=3,q=2;{data}\x1b\\").as_bytes(),
  );
  feed(&mut s, b"\x1b_Ga=d,d=A\x1b\\");
  assert!(s.frame().placements.is_empty());
  assert!(s.images().get(3).is_none());
  // Clearing the screen takes the images with it; the data stays.
  feed(
    &mut s,
    format!("\x1b_Ga=T,f=24,s=2,v=1,i=4,q=2;{data}\x1b\\").as_bytes(),
  );
  feed(&mut s, b"\x1b[2J");
  assert!(s.frame().placements.is_empty());
  assert!(s.images().get(4).is_some());
}

#[test]
fn placements_scroll_with_their_rows() {
  let mut s = screen(10, 3);
  let data = b64(&rgb_2x1());
  feed(
    &mut s,
    format!("\x1b[2;1H\x1b_Ga=T,f=24,s=2,v=1,i=1,q=2;{data}\x1b\\").as_bytes(),
  );
  assert_eq!(s.frame().placements[0].row, 1);
  feed(&mut s, b"\x1b[3;1H\r\n");
  assert_eq!(s.frame().placements[0].row, 0);
  feed(&mut s, b"\r\n");
  // Off the top, but in history: back when scrolled back.
  assert!(s.frame().placements.is_empty());
  s.scroll_screen_up(1);
  assert_eq!(s.frame().placements[0].row, 0);
  s.set_scrollback(0);
  // An image taller than the room below the cursor scrolls to fit.
  feed(&mut s, b"\x1b[3;1H");
  feed(
    &mut s,
    format!("\x1b_Ga=T,f=24,s=2,v=1,i=2,r=2,q=2;{data}\x1b\\").as_bytes(),
  );
  let frame = s.frame();
  let p = frame.placements.iter().find(|p| p.rows == 2).unwrap();
  assert_eq!(p.row, 1);
}

#[test]
fn tall_images_scroll_as_far_as_they_can() {
  let mut s = screen(10, 3);
  let data = b64(&rgb_2x1());
  feed(&mut s, b"\x1b[2;1H");
  feed(
    &mut s,
    format!("\x1b_Ga=T,f=24,s=2,v=1,i=1,r=65535,q=2;{data}\x1b\\").as_bytes(),
  );
  let frame = s.frame();
  assert_eq!(frame.placements.len(), 1);
  assert_eq!(frame.placements[0].row, 0);
  // The cursor lands on the last row, after the (huge) image.
  assert_eq!(s.cursor_position().0, 2);
}

/// Yazi's way: one image id, placeholders, `a=d,d=A` before the next.
#[test]
fn replacing_a_placeholder_image_frees_the_old_one() {
  let mut s = screen(20, 6);
  let data = b64(&rgb_2x1());
  let marks = "\u{305}\u{305}";
  let put = |s: &mut Screen, phase: u8| {
    // Chunked, as yazi sends it.
    let bytes = rgb_2x1()
      .iter()
      .map(|b| b.wrapping_add(phase))
      .collect::<Vec<_>>();
    let payload = b64(&bytes);
    let (head, tail) = payload.split_at(4);
    feed(
      s,
      format!(
        "\x1b_Gq=2,a=T,C=1,U=1,f=24,s=2,v=1,i=84171,m=1;{head}\x1b\\\x1b_Gq=2,m=0;{tail}\x1b\\"
      )
      .as_bytes(),
    );
    // The image id in the 24-bit foreground: 84171 = 1, 72, 203.
    feed(
      s,
      format!("\x1b[2;2H\x1b[38;2;1;72;203m\u{10EEEE}{marks}\x1b[m").as_bytes(),
    );
  };
  let _ = data;
  put(&mut s, 0);
  let first = s.frame();
  assert_eq!(first.placements.len(), 1);
  assert_eq!(first.images.len(), 1);
  let old_key = first.placements[0].key;
  // Erase, then the next image under the same id, in one chunk.
  feed(&mut s, b"\x1b_Gq=2,a=d,d=A\x1b\\");
  put(&mut s, 1);
  let second = s.frame();
  assert_eq!(second.images.len(), 1, "the erased image is freed");
  assert_eq!(second.placements.len(), 1);
  assert_ne!(second.placements[0].key, old_key);
  // The painter deletes the old one on the client.
  let mut painter = Painter::new(TermCaps {
    sync_output: false,
    ..TermCaps::full()
  });
  let mut out = Vec::new();
  painter.paint(&mut out, &Arc::new(first));
  out.clear();
  painter.paint(&mut out, &Arc::new(second));
  let text = String::from_utf8_lossy(&out);
  assert!(text.contains("a=d,d=i"), "old placement deleted: {text:?}");
  assert!(text.contains("a=d,d=I"), "old image freed: {text:?}");
}

#[test]
fn placeholders_draw_virtual_placements() {
  let mut s = screen(10, 4);
  let data = b64(&rgb_2x1());
  feed(
    &mut s,
    format!("\x1b_Ga=T,U=1,f=24,s=2,v=1,i=1,c=4,r=2,q=2;{data}\x1b\\")
      .as_bytes(),
  );
  assert!(
    s.frame().placements.is_empty(),
    "virtual placements show through cells"
  );
  // Two rows of four placeholder cells, image id 1 in the foreground.
  let (r0, r1) = (placeholder::mark(0).unwrap(), placeholder::mark(1).unwrap());
  let cols: Vec<char> = (0..4).map(|c| placeholder::mark(c).unwrap()).collect();
  let mut text = String::from("\x1b[38;5;1m");
  for row_mark in [r0, r1] {
    for col_mark in &cols {
      text.push('\u{10EEEE}');
      text.push(row_mark);
      text.push(*col_mark);
    }
    text.push_str("\r\n");
  }
  feed(&mut s, text.as_bytes());
  let frame = s.frame();
  assert_eq!(frame.placements.len(), 1, "{:?}", frame.placements);
  let p = &frame.placements[0];
  assert_eq!((p.row, p.col, p.cols, p.rows), (0, 0, 4, 2));
  assert_eq!(
    p.src,
    SrcRect {
      x: 0,
      y: 0,
      w: 2,
      h: 1
    }
  );
  // A placeholder without marks continues the one to its left.
  let mut s = screen(10, 4);
  feed(
    &mut s,
    format!("\x1b_Ga=T,U=1,f=24,s=2,v=1,i=1,c=3,r=1,q=2;{data}\x1b\\")
      .as_bytes(),
  );
  feed(
    &mut s,
    "\x1b[38;5;1m\u{10EEEE}\u{305}\u{305}\u{10EEEE}\u{10EEEE}".as_bytes(),
  );
  let frame = s.frame();
  assert_eq!(frame.placements.len(), 1);
  assert_eq!(frame.placements[0].cols, 3);
  // Text cells and the cursor are untouched by the image layer.
  assert_eq!(s.cursor_position(), (0, 3));
}

/// A sixel image is drawn into its cells: a program erases it by writing
/// over them, and nothing else does.
#[test]
fn sixel_images_go_with_their_cells() {
  let mut s = screen(20, 6);
  // A 2x1 sixel: one row of two pixels, so a one-cell placement.
  let sixel = "\x1bP0;0;0q\"1;1;2;1#0;2;100;0;0#0~~\x1b\\";
  feed(&mut s, format!("\x1b[2;3H{sixel}").as_bytes());
  let first = s.frame();
  assert_eq!(first.placements.len(), 1);
  assert_eq!(s.images().len(), 1);
  let (row, col) = (first.placements[0].row, first.placements[0].col);
  assert_eq!((row, col), (1, 2));
  // Text elsewhere on the row leaves it; the cursor is below the image.
  feed(&mut s, b"\x1b[2;10Hhello");
  assert_eq!(s.frame().placements.len(), 1);
  // Writing over its cells erases it, and frees the pixels.
  feed(&mut s, b"\x1b[2;3H ");
  let after = s.frame();
  assert!(after.placements.is_empty());
  assert_eq!(s.images().len(), 0);
  // The client is told to drop it.
  let mut painter = Painter::new(TermCaps {
    sync_output: false,
    ..TermCaps::full()
  });
  let mut out = Vec::new();
  painter.paint(&mut out, &Arc::new(first));
  assert!(String::from_utf8_lossy(&out).contains("a=p,"));
  out.clear();
  painter.paint(&mut out, &Arc::new(after));
  let text = String::from_utf8_lossy(&out);
  assert!(text.contains("a=d,d=i"), "{text:?}");
  assert!(text.contains("a=d,d=I"), "{text:?}");
  // A kitty placement is not erased by text; only deletes remove it.
  let data = b64(&rgb_2x1());
  feed(
    &mut s,
    format!("\x1b[4;1H\x1b_Ga=T,f=24,s=2,v=1,i=1,q=2;{data}\x1b\\").as_bytes(),
  );
  feed(&mut s, b"\x1b[4;1Hxx");
  assert_eq!(s.frame().placements.len(), 1);
}

#[test]
fn sixel_images_land_in_the_store() {
  let mut s = screen(10, 4);
  feed(&mut s, b"\x1b[2;2H\x1bP0;1;0q\"1;1;2;6#1~~\x1b\\");
  let frame = s.frame();
  assert_eq!(frame.placements.len(), 1);
  let p = &frame.placements[0];
  assert_eq!((p.row, p.col), (1, 1));
  let image = frame.images.get(&p.key).unwrap();
  assert_eq!(image.data.format, Format::Rgba);
  assert_eq!((image.data.width, image.data.height), (2, 6));
  assert_eq!(s.cursor_position(), (2, 0));
}

#[test]
fn painter_sends_images_once_and_tracks_placements() {
  let mut s = screen(10, 3);
  let data = b64(&rgb_2x1());
  feed(
    &mut s,
    format!("\x1b[2;1H\x1b_Ga=T,f=24,s=2,v=1,i=1,q=2;{data}\x1b\\").as_bytes(),
  );
  let mut painter = Painter::new(TermCaps {
    sync_output: false,
    ..TermCaps::full()
  });
  let mut out = Vec::new();
  painter.paint(&mut out, &Arc::new(s.frame()));
  let text = String::from_utf8_lossy(&out);
  assert!(
    text.contains("\x1b_Ga=t,q=2,i=1073741824,f=24,s=2,v=1,m=0;"),
    "{text:?}"
  );
  assert!(text.contains("\x1b[2;1H\x1b_Ga=p,q=2,C=1,i=1073741824,p=1,x=0,y=0,w=2,h=1,c=1,r=1,X=0,Y=0,z=0\x1b\\"), "{text:?}");
  // Nothing changed: nothing sent.
  out.clear();
  painter.paint(&mut out, &Arc::new(s.frame()));
  assert_eq!(out, b"");
  // The placement moves: deleted and placed again, the image kept.
  feed(&mut s, b"\x1b[3;1H\r\n");
  out.clear();
  painter.paint(&mut out, &Arc::new(s.frame()));
  let text = String::from_utf8_lossy(&out);
  assert!(text.contains("a=d,d=i,q=2,i=1073741824,p=1"), "{text:?}");
  assert!(!text.contains("a=t,"), "{text:?}");
  assert!(text.contains("a=p,q=2,C=1,i=1073741824,p=2"), "{text:?}");
  // The image goes: freed on the terminal.
  feed(&mut s, b"\x1b_Ga=d,d=I,i=1\x1b\\");
  out.clear();
  painter.paint(&mut out, &Arc::new(s.frame()));
  let text = String::from_utf8_lossy(&out);
  assert!(text.contains("a=d,d=I,q=2,i=1073741824"), "{text:?}");
  // A terminal without graphics gets no image commands at all.
  let mut plain = Painter::new(TermCaps::default());
  feed(
    &mut s,
    format!("\x1b_Ga=T,f=24,s=2,v=1,i=2,q=2;{data}\x1b\\").as_bytes(),
  );
  out.clear();
  plain.paint(&mut out, &Arc::new(s.frame()));
  assert!(!out.windows(3).any(|w| w == b"\x1b_G"));
}

#[test]
fn composed_frames_carry_clipped_images() {
  use crate::console::ui_term::render_frame;
  use crate::term::{Grid, Palette, grid::Rect};
  let mut s = screen(10, 3);
  let data = b64(&rgb_2x1());
  feed(
    &mut s,
    format!("\x1b_Ga=T,f=24,s=2,v=1,i=1,c=6,r=2,q=2;{data}\x1b\\").as_bytes(),
  );
  let frame = s.frame();
  let mut grid = Grid::new(
    Size {
      width: 20,
      height: 10,
    },
    0,
  );
  // A pane four columns wide at (5, 2): the image is cut to it.
  render_frame(&frame, Rect::new(5, 2, 4, 3), &mut grid, &Palette::dark());
  let composed = crate::term::Frame::of_grid(&grid);
  assert_eq!(composed.placements.len(), 1);
  let p = &composed.placements[0];
  assert_eq!((p.row, p.col, p.cols, p.rows), (2, 5, 4, 2));
  assert_eq!(p.src.w, 1, "two thirds of two pixels, at least one");
  assert!(composed.images.contains_key(&p.key));
}

#[test]
fn images_survive_a_snapshot() {
  let mut s = screen(10, 4);
  let data = b64(&rgb_2x1());
  feed(
    &mut s,
    format!("\x1b[2;2H\x1b_Ga=T,f=24,s=2,v=1,i=1,p=3,c=2,r=1,q=2;{data}\x1b\\")
      .as_bytes(),
  );
  feed(
    &mut s,
    format!("\x1b_Ga=T,U=1,f=24,s=2,v=1,i=2,c=1,r=1,q=2;{data}\x1b\\")
      .as_bytes(),
  );
  feed(
    &mut s,
    "\x1b[3;1H\x1b[38;5;2m\u{10EEEE}\u{305}\u{305}".as_bytes(),
  );
  let before = s.frame();
  let json = serde_json::to_vec(&s.snapshot()).unwrap();
  let decoded = serde_json::from_slice(&json).unwrap();
  let restored = Screen::from_snapshot(&decoded).unwrap();
  let after = restored.frame();
  assert_eq!(after.placements.len(), before.placements.len());
  assert_eq!(after.placements.len(), 2);
  for (a, b) in before.placements.iter().zip(after.placements.iter()) {
    assert_eq!(
      (a.row, a.col, a.cols, a.rows, a.src),
      (b.row, b.col, b.cols, b.rows, b.src)
    );
    assert!(after.images.contains_key(&b.key));
  }
  assert_eq!(restored.images().len(), 2);
  assert_eq!(restored.images().get(1).unwrap().data.bytes, rgb_2x1());
}

#[test]
fn animation_edits_and_compositions_replay_in_arrival_order() {
  let base = b"\x1b_Ga=T,f=24,s=1,v=1,i=1,q=2;AAAA\x1b\\";
  let updates = b"\x1b_Ga=f,f=24,s=1,v=1,i=1,q=2;/wAA\x1b\\\x1b_Ga=c,i=1,r=2,c=1,C=1,q=2\x1b\\\x1b_Ga=f,f=24,s=1,v=1,i=1,r=2,q=2;AP8A\x1b\\";
  let assert_order = |out: &[u8]| {
    let text = String::from_utf8_lossy(out);
    let red = text.find("/wAA").unwrap();
    let copy = text.find("a=c").unwrap();
    let green = text.find("AP8A").unwrap();
    // Copy frame 2's red pixels to frame 1 before frame 2 turns green.
    assert!(red < copy && copy < green, "{text:?}");
  };
  // A newly attached client and an existing client receiving a batch.
  for attached in [false, true] {
    let mut s = screen(10, 3);
    feed(&mut s, base);
    let mut painter = Painter::new(TermCaps::full());
    let mut out = Vec::new();
    if attached {
      painter.paint(&mut out, &Arc::new(s.frame()));
      out.clear();
    }
    feed(&mut s, updates);
    painter.paint(&mut out, &Arc::new(s.frame()));
    assert_order(&out);
    out.clear();
    painter.paint(&mut out, &Arc::new(s.frame()));
    assert!(out.is_empty(), "commands must not be replayed twice");

    let json = serde_json::to_vec(&s.snapshot()).unwrap();
    let decoded = serde_json::from_slice(&json).unwrap();
    let restored = Screen::from_snapshot(&decoded).unwrap();
    assert_eq!(
      s.images().get(1).unwrap().commands,
      restored.images().get(1).unwrap().commands
    );
    let mut resumed = Painter::new(TermCaps::full());
    resumed.paint(&mut out, &Arc::new(restored.frame()));
    assert_order(&out);
  }
}

#[test]
fn animation_snapshots_accept_legacy_order_and_reject_missing_commands() {
  let mut s = screen(10, 3);
  feed(&mut s, b"\x1b_Ga=T,f=24,s=1,v=1,i=1,q=2;AAAA\x1b\\\x1b_Ga=f,f=24,s=1,v=1,i=1,q=2;/wAA\x1b\\\x1b_Ga=a,i=1,s=3,q=2\x1b\\");
  let saved = s.snapshot();
  assert!(saved.images[0].order.is_empty());
  let json = serde_json::to_value(&saved).unwrap();
  assert!(json["images"][0].get("order").is_none());
  let restored =
    Screen::from_snapshot(&serde_json::from_value(json).unwrap()).unwrap();
  assert_eq!(
    s.images().get(1).unwrap().commands,
    restored.images().get(1).unwrap().commands
  );

  feed(&mut s, b"\x1b_Ga=f,f=24,s=1,v=1,i=1,r=2,q=2;AP8A\x1b\\");
  let mut saved = s.snapshot();
  saved.images[0].order.pop();
  assert!(Screen::from_snapshot(&saved).is_err());
}
