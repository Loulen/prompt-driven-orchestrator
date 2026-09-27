//! The session context's adapters: the HTTP and WebSocket routes (driving), and
//! tmux, the PTY, the OS terminal window and the environment (driven).

pub mod env;
pub mod http;
pub(crate) mod pty;
pub mod terminal_window;
pub mod tmux;
