//! Rendering. One module per view; `ui::ticket` is the original ticket pane,
//! `ui::todo` the checklist. Both draw through the same theme and obey the
//! no-backgrounds rule (see theme.rs).

pub mod ticket;
pub mod todo;

// The ticket pane's API predates the split — keep it reachable as `ui::*` so
// app.rs and main.rs read the way they always did.
pub use ticket::{build, compose_height, draw, line_text, plain_dump, Segments};
