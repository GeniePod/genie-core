//! Loadable Skill Modules (LSM) — Linux-inspired dynamic skill loading.
//!
//! Scans `/opt/geniepod/skills/` for `.so` files, loads them via `dlopen`,
//! and registers their tools with the dispatcher.

pub mod loader;

pub use loader::{LoadedSkill, SkillLoader};
