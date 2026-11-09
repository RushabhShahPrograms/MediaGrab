//! mediagrab-core: the engine behind the MediaGrab app.
//!
//! Layers (each one only knows about the one below it):
//!
//!   engine    - orchestrates: resolve links -> queue -> parallel downloads -> events
//!   resolve   - turns a URL (video, playlist, SoundCloud set...) into a list of `Item`s
//!   download  - runs ONE yt-dlp process for ONE item and parses its output
//!   parse     - pure functions (no I/O) that parse yt-dlp output; easy to unit test
//!   types     - plain data shared by everything, including the UI

pub mod download;
pub mod engine;
pub mod parse;
pub mod resolve;
pub mod types;

pub use engine::{CancelHandle, Engine};
pub use types::{AudioFormat, Event, Item, ItemState, Mode, Options};
