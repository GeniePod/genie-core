use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
struct Timer {
    label: String,
    end_ms: u64,
}

/// Simple in-memory timer manager.
///
/// Timers are checked by the voice orchestrator on each tick.
/// When a timer fires, the orchestrator speaks the notification.
pub struct TimerManager {
    timers: Arc<Mutex<Vec<Timer>>>,
}

impl Default for TimerManager {
    fn default() -> Self {
        Self {
            timers: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl TimerManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, seconds: u64, label: &str) {
        let end_ms = now_ms() + seconds * 1000;
        let timer = Timer {
            label: label.to_string(),
            end_ms,
        };
        let mut timers = self.timers.lock().unwrap();
        timers.push(timer);
        tracing::info!(seconds, label, "timer set");
    }

    /// Check and drain any fired timers.
    pub fn check_fired(&self) -> Vec<String> {
        let now = now_ms();
        let mut timers = self.timers.lock().unwrap();
        let mut fired = Vec::new();

        timers.retain(|t| {
            if t.end_ms <= now {
                fired.push(t.label.clone());
                false
            } else {
                true
            }
        });

        fired
    }

    pub fn count(&self) -> usize {
        self.timers.lock().unwrap().len()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
