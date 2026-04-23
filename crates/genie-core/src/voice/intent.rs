//! Conservative shared-room intent gating for voice transcripts.
//!
//! The goal is not to classify every utterance perfectly. It is to reject
//! obvious ambient chatter and low-signal transcripts before they consume
//! LLM/tool budget in wake-word and follow-up flows.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceIntentDecision {
    Accept,
    Reject(&'static str),
}

pub fn assess_transcript(text: &str) -> VoiceIntentDecision {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return VoiceIntentDecision::Reject("empty transcript");
    }

    let lower = trimmed.to_ascii_lowercase();
    let words = word_count(&lower);

    if is_low_signal_filler(&lower) {
        return VoiceIntentDecision::Reject("low-signal filler");
    }

    if looks_like_direct_request(&lower) {
        return VoiceIntentDecision::Accept;
    }

    if looks_like_ambient_narration(&lower, words) {
        return VoiceIntentDecision::Reject("ambient narration");
    }

    VoiceIntentDecision::Accept
}

fn looks_like_direct_request(text: &str) -> bool {
    text.ends_with('?')
        || starts_with_any(
            text,
            &[
                "what ",
                "what's ",
                "whats ",
                "who ",
                "when ",
                "where ",
                "why ",
                "how ",
                "can you ",
                "could you ",
                "would you ",
                "will you ",
                "please ",
                "turn ",
                "set ",
                "play ",
                "search ",
                "look up ",
                "remember ",
                "forget ",
                "open ",
                "close ",
                "lock ",
                "unlock ",
                "dim ",
                "brighten ",
                "check ",
                "tell me ",
                "show me ",
                "is ",
                "are ",
                "do ",
                "did ",
                "weather ",
                "timer ",
                "remind ",
                "calculate ",
                "call ",
                "text ",
            ],
        )
        || contains_any(
            text,
            &[
                " genie",
                " jarvis",
                " assistant",
                " lights",
                " light ",
                " thermostat",
                " temperature",
                " home assistant",
                " music",
                " tv",
                " volume",
                " alarm",
                " reminder",
                " kitchen",
                " bedroom",
                " living room",
                " garage",
                " front door",
                " weather",
                " time is it",
                " status",
                " search the web",
            ],
        )
}

fn looks_like_ambient_narration(text: &str, words: usize) -> bool {
    words >= 9
        && starts_with_any(
            text,
            &[
                "the ", "a ", "an ", "he ", "she ", "they ", "it ", "we ", "this ", "that ",
            ],
        )
        && !text.ends_with('?')
        && !contains_any(
            text,
            &[
                "please",
                "can you",
                "could you",
                "would you",
                "turn",
                "set",
                "play",
                "search",
                "remember",
                "forget",
                "weather",
                "timer",
                "remind",
                "assistant",
                "genie",
                "jarvis",
            ],
        )
}

fn is_low_signal_filler(text: &str) -> bool {
    matches!(
        text,
        "okay"
            | "ok"
            | "hmm"
            | "uh"
            | "um"
            | "mm"
            | "huh"
            | "right"
            | "yeah"
            | "yep"
            | "nope"
            | "thanks"
            | "thank you"
            | "good night"
            | "goodbye"
    )
}

fn starts_with_any(text: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| text.starts_with(prefix))
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_direct_home_command() {
        assert_eq!(
            assess_transcript("turn on the kitchen light"),
            VoiceIntentDecision::Accept
        );
    }

    #[test]
    fn accepts_question() {
        assert_eq!(
            assess_transcript("what time is it?"),
            VoiceIntentDecision::Accept
        );
    }

    #[test]
    fn rejects_low_signal_filler() {
        assert_eq!(
            assess_transcript("thank you"),
            VoiceIntentDecision::Reject("low-signal filler")
        );
    }

    #[test]
    fn rejects_ambient_narration() {
        assert_eq!(
            assess_transcript("the old house stood alone at the end of the road"),
            VoiceIntentDecision::Reject("ambient narration")
        );
    }

    #[test]
    fn does_not_reject_short_status_style_request() {
        assert_eq!(
            assess_transcript("weather in Tokyo"),
            VoiceIntentDecision::Accept
        );
    }
}
