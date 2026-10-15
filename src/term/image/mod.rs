//! Images on screens: the kitty graphics protocol (and, through the
//! same store, sixel), placements, and what frames show of them.

pub mod kitty;
pub mod placeholder;
pub mod placement;
pub mod png;
pub mod sixel;
pub mod store;

pub use placement::{Placement, SrcRect, VisiblePlacement};
pub use store::{Format, Image, ImageData, ImageKey, ImageStore, ImageTable};
