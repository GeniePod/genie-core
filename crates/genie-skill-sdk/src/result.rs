//! Skill result type.

/// Result type for skill execution.
/// Ok(String) = success with output text.
/// Err(String) = failure with error message.
pub type SkillResult = Result<String, String>;
