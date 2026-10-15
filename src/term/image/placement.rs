//! Where images show: placements on a grid, and what of them a frame
//! shows.

use super::store::ImageKey;
use crate::term::grid::Rect;

/// A rectangle of an image's pixels; `w == 0 || h == 0` is the whole
/// image.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SrcRect {
  pub x: u32,
  pub y: u32,
  pub w: u32,
  pub h: u32,
}

impl SrcRect {
  /// The part of this rectangle that `ncols × nrows` cells from
  /// `(col, row)` show, of a placement `cols × rows` cells large: the cut
  /// in proportion to the cells, at least a pixel.
  pub fn part(
    self,
    cols: u16,
    rows: u16,
    col: u32,
    row: u32,
    ncols: u32,
    nrows: u32,
  ) -> SrcRect {
    let (pc, pr) = (u64::from(cols.max(1)), u64::from(rows.max(1)));
    SrcRect {
      x: self.x + (u64::from(self.w) * u64::from(col) / pc) as u32,
      y: self.y + (u64::from(self.h) * u64::from(row) / pr) as u32,
      w: ((u64::from(self.w) * u64::from(ncols) / pc) as u32).max(1),
      h: ((u64::from(self.h) * u64::from(nrows) / pr) as u32).max(1),
    }
  }

  /// Resolved against the image size.
  pub fn resolve(self, width: u32, height: u32) -> SrcRect {
    let x = self.x.min(width);
    let y = self.y.min(height);
    let w = if self.w == 0 {
      width - x
    } else {
      self.w.min(width - x)
    };
    let h = if self.h == 0 {
      height - y
    } else {
      self.h.min(height - y)
    };
    SrcRect { x, y, w, h }
  }
}

/// A classic placement on a grid, anchored to an absolute row (rows
/// count from the first the grid ever had, so it scrolls with its line).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Placement {
  pub key: ImageKey,
  /// The program's placement id (`p`), 0 when none.
  pub id: u32,
  pub row: u64,
  pub col: u16,
  pub cols: u16,
  pub rows: u16,
  pub src: SrcRect,
  /// Pixel offsets of the top-left corner within the first cell.
  pub offset_x: u16,
  pub offset_y: u16,
  pub z: i32,
  /// `U=1`: shown through placeholder cells, not on its own.
  pub virtual_: bool,
  /// Drawn into its cells, as a sixel image is: it goes once any of them
  /// is written over, the way a program erases such an image.
  pub cells: bool,
}

/// A placement as a frame shows it: in view coordinates, clipped.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VisiblePlacement {
  pub key: ImageKey,
  pub row: u16,
  pub col: u16,
  pub cols: u16,
  pub rows: u16,
  pub src: SrcRect,
  pub offset_x: u16,
  pub offset_y: u16,
  pub z: i32,
}

impl VisiblePlacement {
  /// The part inside `area`, moved by `(dx, dy)`: how a frame composed
  /// into another grid keeps its images. The source rectangle shrinks
  /// in proportion to the cells cut off.
  pub fn clipped(
    &self,
    area: Rect,
    dx: u16,
    dy: u16,
  ) -> Option<VisiblePlacement> {
    let mine = Rect::new(
      self.col.saturating_add(dx),
      self.row.saturating_add(dy),
      self.cols,
      self.rows,
    );
    let shown = mine.intersect(area);
    if shown.width == 0 || shown.height == 0 {
      return None;
    }
    let cut_left = u32::from(shown.x - mine.x);
    let cut_top = u32::from(shown.y - mine.y);
    Some(VisiblePlacement {
      key: self.key,
      row: shown.y,
      col: shown.x,
      cols: shown.width,
      rows: shown.height,
      src: self.src.part(
        self.cols,
        self.rows,
        cut_left,
        cut_top,
        u32::from(shown.width),
        u32::from(shown.height),
      ),
      offset_x: if cut_left == 0 { self.offset_x } else { 0 },
      offset_y: if cut_top == 0 { self.offset_y } else { 0 },
      z: self.z,
    })
  }
}

/// How many cells an image takes, from the program's request and the
/// cell size: both given, one given (the other keeps the aspect ratio),
/// or neither (its natural pixel size).
pub fn cell_size(
  src: SrcRect,
  cols: u32,
  rows: u32,
  cell_px: (u16, u16),
) -> (u16, u16) {
  let (cw, ch) = (u32::from(cell_px.0.max(1)), u32::from(cell_px.1.max(1)));
  let (w, h) = (src.w.max(1) as u64, src.h.max(1) as u64);
  let clamp = |n: u64| n.clamp(1, u16::MAX as u64) as u16;
  match (cols, rows) {
    (c, r) if c > 0 && r > 0 => (clamp(c as u64), clamp(r as u64)),
    (c, 0) if c > 0 => {
      // Scale so the width is c cells; the height follows.
      let px_w = c as u64 * cw as u64;
      let px_h = h * px_w / w;
      (clamp(c as u64), clamp(px_h.div_ceil(ch as u64)))
    }
    (0, r) if r > 0 => {
      let px_h = r as u64 * ch as u64;
      let px_w = w * px_h / h;
      (clamp(px_w.div_ceil(cw as u64)), clamp(r as u64))
    }
    _ => (clamp(w.div_ceil(cw as u64)), clamp(h.div_ceil(ch as u64))),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sizes() {
    let src = SrcRect {
      x: 0,
      y: 0,
      w: 100,
      h: 50,
    };
    assert_eq!(cell_size(src, 0, 0, (10, 20)), (10, 3));
    assert_eq!(cell_size(src, 5, 0, (10, 20)), (5, 2));
    assert_eq!(cell_size(src, 0, 5, (10, 20)), (20, 5));
    assert_eq!(cell_size(src, 3, 7, (10, 20)), (3, 7));
    assert_eq!(
      SrcRect::default().resolve(40, 30),
      SrcRect {
        x: 0,
        y: 0,
        w: 40,
        h: 30
      }
    );
  }

  #[test]
  fn clipping_cuts_the_source_in_proportion() {
    let shown = VisiblePlacement {
      key: 1,
      row: 2,
      col: 4,
      cols: 10,
      rows: 4,
      src: SrcRect {
        x: 0,
        y: 0,
        w: 200,
        h: 80,
      },
      offset_x: 3,
      offset_y: 0,
      z: 0,
    };
    // Whole, moved.
    let moved = shown.clipped(Rect::new(0, 0, 40, 20), 1, 1).unwrap();
    assert_eq!(
      (moved.row, moved.col, moved.cols, moved.rows),
      (3, 5, 10, 4)
    );
    assert_eq!(moved.src, shown.src);
    assert_eq!(moved.offset_x, 3);
    // Cut on the left and bottom.
    let cut = shown.clipped(Rect::new(7, 0, 40, 4), 0, 0).unwrap();
    assert_eq!((cut.row, cut.col, cut.cols, cut.rows), (2, 7, 7, 2));
    assert_eq!(
      cut.src,
      SrcRect {
        x: 60,
        y: 0,
        w: 140,
        h: 40
      }
    );
    assert_eq!(cut.offset_x, 0);
    // Outside.
    assert!(shown.clipped(Rect::new(20, 20, 5, 5), 0, 0).is_none());
  }
}
