//! Platform-independent core of DNS Switcher: the resolver list and the
//! measurement code. The Windows app (`src/main.rs`) is built on top of it.
//!
//! Tests live here rather than in the binary because release builds of the
//! binary embed a manifest that requires administrator rights, which would
//! stop an unelevated `cargo test --release` from launching it.

pub mod ipc;
pub mod probe;
pub mod providers;
