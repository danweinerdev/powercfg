//! Data acquisition layer.
//!
//! Each submodule reads from one transport (sysfs, procfs, D-Bus, journal,
//! subprocess) and returns plain model structs. Source functions return
//! `Result<T, SourceError>`; the command layer applies
//! `.unwrap_or_default()` / `.ok()` per call site to choose presentation
//! behavior. See `Designs/RustRewrite/README.md` Decision 7.

pub mod dbus;
pub mod error;
pub mod exec;
pub mod procfs;
pub mod sysfs;
pub mod userspace;

pub use error::SourceError;
