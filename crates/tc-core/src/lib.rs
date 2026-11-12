//! Core of terminal-conductor. Deliberately free of Tauri types so it can be
//! moved into a separate host process later.

pub mod history;
pub mod hook;
pub mod launch;
pub mod osc;
pub mod project;
pub mod pty;
pub mod status;
pub mod store;
