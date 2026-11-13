//! Chess rules, position evaluation, bounded search, and UCI analysis.
pub mod board;
pub mod eval;
mod form;
mod game;
#[cfg(not(target_arch = "wasm32"))]
pub mod gui;
pub mod search;
#[cfg(not(target_arch = "wasm32"))]
pub mod uci;
#[cfg(target_arch = "wasm32")]
mod wasm;
