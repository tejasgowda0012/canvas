//! Canvas Studio whiteboard core.
//!
//! Platform independent: the document model, undo history, hit testing, shape
//! recognition, board files, the tool state machine and a software renderer.
//! The Android shell (`rust/android`) feeds it touch samples and copies the
//! rectangles it repaints into the window.

pub mod document;
pub mod editor;
pub mod geom;
pub mod math;
mod mindmap;
pub mod import;
pub mod recognize;
pub mod refine;
pub mod render;
pub mod shape;
pub mod store;
pub mod sys;
pub mod table;
pub mod text;
pub mod ui;

pub use document::Document;
pub use editor::{Contact, Editor, InkRequest, PenMode, TextRequest};
pub use geom::{IRect, Pt, Rect};
pub use recognize::recognize_shape;
pub use shape::{Shape, Tool};
