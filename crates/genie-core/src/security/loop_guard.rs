/// Loop guard — circuit breaker for LLM tool call loops.
///
/// Adapted from OpenFang's loop_guard.rs — prevents the LLM from
/// calling the same tool infinitely or oscillating between tools.
///
/// Detection methods:
/// 1. Per-tool call counting (same tool+args = repeat)
/// 2. Global circuit breaker (max total calls per conversation turn)
/// 3. Ping-pong detection (A→B→A→B pattern)
///
/// RAM cost: ~0 (small Vec of recent call hashes, cleared each turn).
use std::collections::HashMap;

/// Loop guard state for a single conversation turn.
pub struct LoopGuard {
    /// Hash → call count for this turn.
    call_counts: HashMap<u64, u32>,
    /// Recent tool names for ping-pong detection.
    recent_tools: Vec<String>,
    /// Total calls this turn.
    total_calls: u32,
    /// Configuration.
    config: LoopGuardConfig,
}

#[derive(Debug, Clone)]
pub struct LoopGuardConfig {
    /// Max times the same tool+args can be called.
    pub max_repeat_calls: u32,
    /// Max total tool calls per conversation turn.
    pub max_total_calls: u32,
    /// Max ping-pong cycles (A→B→A→B) before blocking.
    pub max_pingpong_cycles: u32,
}

impl Default for LoopGuardConfig {
    fn default() -> Self {
        Self {
            max_repeat_calls: 3,
            max_total_calls: 20,
            max_pingpong_cycles: 3,
        }
    }
}

/// Result of checking a tool call against the loop guard.
#[derive(Debug, Clone, PartialEq)]
pub enum LoopCheck {
    /// Call is allowed.
    Allow,
    /// Call is warned (approaching limit).
    Warn(String),
    /// Call is blocked (limit exceeded).
    Block(String),
}

impl LoopGuard {
    pub fn new(config: LoopGuardConfig) -> Self {
        Self {
            call_counts: HashMap::new(),
            recent_tools: Vec::new(),
            total_calls: 0,
            config,
        }
    }

    /// Check if a tool call should be allowed.
    pub fn check(&mut self, tool_name: &str, args_json: &str) -> LoopCheck {
        self.total_calls += 1;

        // 1. Global circuit breaker.
        if self.total_calls > self.config.max_total_calls {
            return LoopCheck::Block(format!(
                "circuit breaker: {} total tool calls exceeded limit of {}",
                self.total_calls, self.config.max_total_calls
            ));
        }

        // 2. Per-tool repeat detection.
        let hash = hash_call(tool_name, args_json);
        let count = self.call_counts.entry(hash).or_insert(0);
        *count += 1;

        if *count > self.config.max_repeat_calls {
            return LoopCheck::Block(format!(
                "repeat blocked: {} called {} times with same args (limit: {})",
                tool_name, count, self.config.max_repeat_calls
            ));
        }

        if *count == self.config.max_repeat_calls {
            return LoopCheck::Warn(format!(
                "repeat warning: {} called {} times — next call will be blocked",
                tool_name, count
            ));
        }

        // 3. Ping-pong detection.
        self.recent_tools.push(tool_name.to_string());
        if let Some(msg) = self.detect_pingpong() {
            return LoopCheck::Block(msg);
        }

        LoopCheck::Allow
    }

    /// Reset for a new conversation turn.
    pub fn reset(&mut self) {
        self.call_counts.clear();
        self.recent_tools.clear();
        self.total_calls = 0;
    }

    /// Detect A→B→A→B ping-pong patterns.
    fn detect_pingpong(&self) -> Option<String> {
        let tools = &self.recent_tools;
        let len = tools.len();
        if len < 4 {
            return None;
        }

        // Count consecutive A→B→A→B cycles from the end.
        let mut cycles_2: u32 = 0;
        let mut pos = len;
        while pos >= 4 {
            if tools[pos - 4] == tools[pos - 2] && tools[pos - 3] == tools[pos - 1] {
                cycles_2 += 1;
                pos -= 2;
            } else {
                break;
            }
        }

        if cycles_2 >= self.config.max_pingpong_cycles {
            return Some(format!(
                "ping-pong blocked: {} ↔ {} repeated {} cycles",
                tools[len - 2],
                tools[len - 1],
                cycles_2
            ));
        }

        None
    }
}

fn hash_call(tool_name: &str, args_json: &str) -> u64 {
    let mut hash: u64 = 5381;
    for b in tool_name
        .bytes()
        .chain(b":".iter().copied())
        .chain(args_json.bytes())
    {
        hash = hash.wrapping_mul(33).wrapping_add(b as u64);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_first_call() {
        let mut guard = LoopGuard::new(LoopGuardConfig::default());
        assert_eq!(guard.check("get_time", "{}"), LoopCheck::Allow);
    }

    #[test]
    fn warns_on_repeat_threshold() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            max_repeat_calls: 3,
            ..Default::default()
        });
        assert_eq!(guard.check("get_time", "{}"), LoopCheck::Allow);
        assert_eq!(guard.check("get_time", "{}"), LoopCheck::Allow);
        assert!(matches!(guard.check("get_time", "{}"), LoopCheck::Warn(_)));
        assert!(matches!(guard.check("get_time", "{}"), LoopCheck::Block(_)));
    }

    #[test]
    fn different_args_are_different_calls() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            max_repeat_calls: 5,
            ..Default::default()
        });
        assert_eq!(
            guard.check("get_weather", r#"{"location":"Denver"}"#),
            LoopCheck::Allow
        );
        assert_eq!(
            guard.check("get_weather", r#"{"location":"Tokyo"}"#),
            LoopCheck::Allow
        );
        assert_eq!(
            guard.check("get_weather", r#"{"location":"London"}"#),
            LoopCheck::Allow
        );
        // Same tool, different args = different calls. None should warn yet.
        assert_eq!(
            guard.check("get_weather", r#"{"location":"Denver"}"#),
            LoopCheck::Allow
        );
    }

    #[test]
    fn global_circuit_breaker() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            max_total_calls: 3,
            max_repeat_calls: 100,
            ..Default::default()
        });
        assert_eq!(guard.check("a", "1"), LoopCheck::Allow);
        assert_eq!(guard.check("b", "2"), LoopCheck::Allow);
        assert_eq!(guard.check("c", "3"), LoopCheck::Allow);
        assert!(matches!(guard.check("d", "4"), LoopCheck::Block(_)));
    }

    #[test]
    fn reset_clears_state() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            max_repeat_calls: 2,
            ..Default::default()
        });
        guard.check("get_time", "{}");
        guard.check("get_time", "{}");
        guard.reset();
        // After reset, counts start fresh.
        assert_eq!(guard.check("get_time", "{}"), LoopCheck::Allow);
    }

    #[test]
    fn pingpong_detection() {
        let mut guard = LoopGuard::new(LoopGuardConfig {
            max_pingpong_cycles: 2,
            max_repeat_calls: 100,
            max_total_calls: 100,
        });
        // A→B→A→B→A→B pattern.
        guard.check("tool_a", "{}");
        guard.check("tool_b", "{}");
        guard.check("tool_a", "{}");
        guard.check("tool_b", "{}");
        guard.check("tool_a", "{}");
        let result = guard.check("tool_b", "{}");
        assert!(matches!(result, LoopCheck::Block(_)));
    }
}
