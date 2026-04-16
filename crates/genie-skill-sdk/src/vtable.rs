//! Skill VTable — the C ABI interface between core and skills.
//!
//! Equivalent to Linux's `struct file_operations`.

use std::ffi::c_char;

/// The skill interface. Every `.so` skill exports one of these
/// via the `genie_skill_init()` entry point.
///
/// All string pointers must be valid for the lifetime of the loaded `.so`.
/// The `execute` function's return value must be freed via `destroy`.
#[repr(C)]
pub struct SkillVTable {
    /// ABI version. Core checks `abi_version == ABI_VERSION` before use.
    pub abi_version: u32,

    /// Skill name (tool name for LLM dispatch). Null-terminated C string.
    /// Example: "spotify_control\0"
    pub name: *const c_char,

    /// Human-readable description for LLM tool selection. Null-terminated.
    /// Example: "Control Spotify playback — play, pause, skip\0"
    pub description: *const c_char,

    /// Semantic version string. Null-terminated.
    /// Example: "0.1.0\0"
    pub version: *const c_char,

    /// JSON Schema for parameters (OpenAI function calling format). Null-terminated.
    /// Example: '{"type":"object","properties":{"action":{"type":"string"}}}\0'
    pub parameters_json: *const c_char,

    /// Execute the skill. Takes JSON args, returns JSON result.
    ///
    /// The returned `*mut c_char` is a CString that must be freed via `destroy`.
    /// If execution fails, the JSON will contain `{"success": false, "output": "error msg"}`.
    pub execute: extern "C" fn(args_json: *const c_char) -> *mut c_char,

    /// Free a string returned by `execute`. Must be called after reading the result.
    pub destroy: extern "C" fn(ptr: *mut c_char),
}

// Safety: SkillVTable contains function pointers and static string pointers.
// The .so is loaded once and stays in memory, so pointers remain valid.
unsafe impl Send for SkillVTable {}
unsafe impl Sync for SkillVTable {}
