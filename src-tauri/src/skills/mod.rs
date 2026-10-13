//! Skills are self-contained capability modules invoked by the command
//! executor. New skills (calendar, email, browser, …) can be added here and
//! registered in `commands::registry` without touching the agent core.

pub mod todo;
pub mod windows;
