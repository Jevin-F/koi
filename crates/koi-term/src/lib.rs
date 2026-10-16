//! Terminal sessions for Koi: child processes in pseudo-terminals, their screen
//! state, and the encoding of keyboard, mouse and paste input.
//!
//! This crate has no UI dependency; the app renders the state it exposes.

mod keys;
mod links;
mod modifiers;
mod mouse;
mod palette;
mod paste;
mod program_status;
mod screen;
mod session;
mod signal;
mod summary;

pub use alacritty_terminal as state;
pub use keys::encode_key;
pub use links::link_at;
pub use modifiers::Modifiers;
pub use mouse::{MouseButton, MouseEvent, encode_mouse, reports};
pub use palette::Palette;
pub use paste::encode_paste;
pub use portable_pty::CommandBuilder;
pub use program_status::ProgramStatus;
pub use screen::{Cursor, Fill, Screen, TextSpan, TextStyle};
pub use session::{GridSize, Session, SessionEvent, Terminal};
pub use signal::Signal;
pub use summary::{recent_line, submitted_line};
