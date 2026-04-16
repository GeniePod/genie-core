//! Skill argument parser — safely extract typed values from JSON.

/// Parsed skill arguments from the LLM's tool call.
pub struct SkillArgs {
    inner: serde_json::Value,
}

impl SkillArgs {
    /// Parse from a JSON string. Returns empty args on parse failure.
    pub fn from_json(json: &str) -> Self {
        Self {
            inner: serde_json::from_str(json)
                .unwrap_or(serde_json::Value::Object(Default::default())),
        }
    }

    /// Get a string argument.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.inner.get(key).and_then(|v| v.as_str())
    }

    /// Get an integer argument.
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.inner.get(key).and_then(|v| v.as_i64())
    }

    /// Get a float argument.
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.inner.get(key).and_then(|v| v.as_f64())
    }

    /// Get a boolean argument.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.inner.get(key).and_then(|v| v.as_bool())
    }

    /// Get the raw JSON value for an argument.
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.inner.get(key)
    }

    /// Get all arguments as a JSON object.
    pub fn as_value(&self) -> &serde_json::Value {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args() {
        let args = SkillArgs::from_json(r#"{"name":"Jared","age":25,"active":true}"#);
        assert_eq!(args.get_str("name"), Some("Jared"));
        assert_eq!(args.get_i64("age"), Some(25));
        assert_eq!(args.get_bool("active"), Some(true));
    }

    #[test]
    fn missing_args() {
        let args = SkillArgs::from_json("{}");
        assert_eq!(args.get_str("name"), None);
    }

    #[test]
    fn invalid_json() {
        let args = SkillArgs::from_json("not json");
        assert_eq!(args.get_str("name"), None);
    }
}
