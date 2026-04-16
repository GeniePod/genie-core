/// Environment variable sanitization.
///
/// Blocks sensitive environment variables from leaking through tool execution
/// (subprocess spawning, system_info, etc.).
///
/// Adapted from OpenClaw's sanitize-env-vars.ts (200+ blocked variables).
/// Clean-room Rust implementation.

/// Variables that must NEVER be exposed to tools or subprocesses.
const BLOCKED_EXACT: &[&str] = &[
    // API keys and tokens.
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GOOGLE_API_KEY",
    "GEMINI_API_KEY",
    "HF_TOKEN",
    "HUGGING_FACE_HUB_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "GITLAB_TOKEN",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AZURE_OPENAI_API_KEY",
    "AZURE_API_KEY",
    "SLACK_TOKEN",
    "SLACK_BOT_TOKEN",
    "DISCORD_TOKEN",
    "TELEGRAM_BOT_TOKEN",
    "TWILIO_AUTH_TOKEN",
    "SENDGRID_API_KEY",
    "STRIPE_SECRET_KEY",
    "STRIPE_API_KEY",
    "DATADOG_API_KEY",
    "SENTRY_DSN",
    "NEW_RELIC_LICENSE_KEY",
    "DOCKER_AUTH_CONFIG",
    "NPM_TOKEN",
    "PYPI_TOKEN",
    "CARGO_REGISTRY_TOKEN",
    "HOMEBREW_GITHUB_API_TOKEN",
    // Database credentials.
    "DATABASE_URL",
    "REDIS_URL",
    "MONGODB_URI",
    "POSTGRES_PASSWORD",
    "MYSQL_PASSWORD",
    "DB_PASSWORD",
    // SSH and signing.
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "GPG_AGENT_INFO",
    "GNUPGHOME",
    // GeniePod's own secrets.
    "HA_TOKEN",
    "GENIEPOD_SECRET",
    // Cloud provider credentials.
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_CLOUD_PROJECT",
    "AZURE_SUBSCRIPTION_ID",
    "AZURE_TENANT_ID",
    "AZURE_CLIENT_SECRET",
    // Proxy credentials (may contain passwords).
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    // Shell history (contains command history with secrets).
    "HISTFILE",
    "HISTSIZE",
    "SAVEHIST",
    // Process injection vectors.
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "PYTHONSTARTUP",
    "PYTHONPATH",
    "NODE_OPTIONS",
    "PERL5OPT",
    "RUBYOPT",
    "JAVA_TOOL_OPTIONS",
];

/// Suffix patterns that indicate sensitive variables.
const BLOCKED_SUFFIXES: &[&str] = &[
    "_KEY",
    "_SECRET",
    "_TOKEN",
    "_PASSWORD",
    "_CREDENTIALS",
    "_AUTH",
    "_API_KEY",
    "_PRIVATE_KEY",
    "_PASSPHRASE",
    "_CONNECTION_STRING",
];

/// Prefix patterns that indicate sensitive variables.
const BLOCKED_PREFIXES: &[&str] = &[
    "AWS_", "AZURE_", "GCP_", "GOOGLE_", "VAULT_", "CONSUL_", "NOMAD_",
];

/// Check if an environment variable name should be blocked.
pub fn is_sensitive(name: &str) -> bool {
    let upper = name.to_uppercase();

    // Exact match.
    if BLOCKED_EXACT.iter().any(|&blocked| blocked == upper) {
        return true;
    }

    // Suffix match.
    if BLOCKED_SUFFIXES
        .iter()
        .any(|&suffix| upper.ends_with(suffix))
    {
        return true;
    }

    // Prefix match (only for known cloud/infra prefixes).
    if BLOCKED_PREFIXES
        .iter()
        .any(|&prefix| upper.starts_with(prefix))
    {
        return true;
    }

    false
}

/// Get a sanitized copy of the current environment (for subprocess spawning).
/// Removes all sensitive variables.
pub fn sanitized_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(name, _)| !is_sensitive(name))
        .collect()
}

/// Count how many current env vars would be blocked.
pub fn count_blocked() -> usize {
    std::env::vars()
        .filter(|(name, _)| is_sensitive(name))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_exact_match() {
        assert!(is_sensitive("OPENAI_API_KEY"));
        assert!(is_sensitive("HA_TOKEN"));
        assert!(is_sensitive("AWS_SECRET_ACCESS_KEY"));
        assert!(is_sensitive("LD_PRELOAD"));
    }

    #[test]
    fn blocks_suffix_match() {
        assert!(is_sensitive("MY_CUSTOM_API_KEY"));
        assert!(is_sensitive("WEBHOOK_SECRET"));
        assert!(is_sensitive("DB_PASSWORD"));
        assert!(is_sensitive("SERVICE_TOKEN"));
    }

    #[test]
    fn blocks_prefix_match() {
        assert!(is_sensitive("AWS_REGION"));
        assert!(is_sensitive("AZURE_RESOURCE_GROUP"));
        assert!(is_sensitive("GOOGLE_CLOUD_REGION"));
        assert!(is_sensitive("VAULT_ADDR"));
    }

    #[test]
    fn allows_safe_vars() {
        assert!(!is_sensitive("PATH"));
        assert!(!is_sensitive("HOME"));
        assert!(!is_sensitive("USER"));
        assert!(!is_sensitive("LANG"));
        assert!(!is_sensitive("TERM"));
        assert!(!is_sensitive("SHELL"));
        assert!(!is_sensitive("RUST_LOG"));
        assert!(!is_sensitive("GENIEPOD_CONFIG"));
    }

    #[test]
    fn case_insensitive() {
        assert!(is_sensitive("openai_api_key"));
        assert!(is_sensitive("Ha_Token"));
        assert!(is_sensitive("ld_preload"));
    }

    #[test]
    fn sanitized_env_excludes_blocked() {
        // Set a test sensitive var.
        unsafe { std::env::set_var("TEST_GENIEPOD_SECRET_KEY", "should-be-blocked") };

        let env = sanitized_env();
        assert!(!env.iter().any(|(k, _)| k == "TEST_GENIEPOD_SECRET_KEY"));

        unsafe { std::env::remove_var("TEST_GENIEPOD_SECRET_KEY") };
    }
}
