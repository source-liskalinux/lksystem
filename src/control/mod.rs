//! This module provides the control access similar to systemctl.
//! It uses the jsonrpc 2.0 spec
mod control;
pub mod jsonrpc2;
pub use control::*;
