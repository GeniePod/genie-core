use serde::{Deserialize, Serialize};
use std::fmt;

/// Operating modes for the GeniePod governor.
///
/// Each mode defines which services are running and at what capacity.
/// The governor transitions between modes based on time, user commands,
/// and memory pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Full voice pipeline + LLM 4B + HA + opt-in services.
    /// Default 06:00-23:00. Free RAM: ~2.2-3.2 GB.
    Day,

    /// Same as Day but with background tasks (summarize, analyze, morning briefing).
    /// Default 23:00-06:00.
    NightA,

    /// Unload 4B, load 9B for deeper reasoning. HA + opt-ins stopped.
    /// Free RAM: ~0.7-1.2 GB. Opt-in only.
    NightB,

    /// LLM unloaded, mpv launched for HDMI playback via NVDEC.
    /// Wake word + rules engine remain active for playback control.
    /// Free RAM: +2.8 GB from LLM unload.
    Media,

    /// Emergency mode: memory pressure critical.
    /// Opt-ins stopped, context capped, STT downgraded.
    Pressure,
}

impl Mode {
    /// Services that must be running in this mode.
    pub fn required_services(&self) -> &'static [&'static str] {
        match self {
            Mode::Day | Mode::NightA => &[
                "genie-wakeword",
                "genie-core",
                "genie-llm",
                "genie-mqtt",
                "homeassistant",
            ],
            Mode::NightB => &["genie-wakeword", "genie-core", "genie-llm", "genie-mqtt"],
            Mode::Media => &["genie-wakeword", "genie-core", "genie-mqtt"],
            Mode::Pressure => &["genie-wakeword", "genie-core", "genie-llm", "genie-mqtt"],
        }
    }

    /// Services that must be stopped in this mode.
    pub fn stopped_services(&self) -> &'static [&'static str] {
        match self {
            Mode::Day | Mode::NightA => &[],
            Mode::NightB => &["homeassistant", "nextcloud", "jellyfin"],
            Mode::Media => &["genie-llm", "nextcloud", "jellyfin"],
            Mode::Pressure => &["nextcloud", "jellyfin"],
        }
    }

    /// LLM model to use in this mode.
    pub fn llm_model(&self) -> Option<&'static str> {
        match self {
            Mode::Day | Mode::NightA | Mode::Pressure => Some("nemotron-4b-q4_k_m.gguf"),
            Mode::NightB => Some("nemotron-9b-q4.gguf"),
            Mode::Media => None,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mode::Day => write!(f, "day"),
            Mode::NightA => write!(f, "night-a"),
            Mode::NightB => write!(f, "night-b"),
            Mode::Media => write!(f, "media"),
            Mode::Pressure => write!(f, "pressure"),
        }
    }
}
