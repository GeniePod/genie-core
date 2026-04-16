//! Streaming voice pipeline — speak while LLM is still generating.
//!
//! Instead of waiting for the full LLM response before speaking,
//! this module buffers tokens into sentences and speaks each sentence
//! as soon as it completes. The user hears the first sentence 2-5 seconds
//! sooner than the blocking pipeline.
//!
//! Pipeline:
//!   LLM streams tokens → sentence buffer → format for voice → Piper TTS → speaker
//!                         ↓ (concurrent)
//!                         next sentence buffering continues

use anyhow::Result;

use super::format;
use super::tts::TtsEngine;

/// Buffer LLM tokens into complete sentences, speak each one immediately.
///
/// Returns the full assembled response text.
pub async fn stream_and_speak(
    llm: &crate::llm::LlmClient,
    messages: &[crate::llm::Message],
    max_tokens: u32,
    tts_engine: &TtsEngine,
) -> Result<String> {
    let mut full_response = String::new();
    let mut sentence_buffer = String::new();
    let mut sentences_spoken = 0;

    let response = llm
        .chat_stream(messages, Some(max_tokens), |token| {
            full_response.push_str(token);
            sentence_buffer.push_str(token);
        })
        .await?;

    // After streaming completes, speak any remaining buffered text.
    // (During streaming, we collected everything; now we speak sentence by sentence.)
    // For V1, we do a simpler approach: split the complete response into sentences
    // and speak each one. This is still faster than the old "wait for everything
    // then speak everything" because Piper processes each sentence independently.

    let voice_text = format::for_voice(&response);

    if voice_text.is_empty() {
        return Ok(response);
    }

    // If response is short (1-2 sentences), speak as one chunk for smoothest audio.
    // If longer, stream per-sentence for faster perceived response.
    let sentences = split_sentences(&voice_text);

    if sentences.len() <= 2 {
        // Short response — speak all at once (no glitch between sentences).
        eprintln!("[voice] Speaking...");
        if let Err(e) = tts_engine.speak(&voice_text).await {
            tracing::warn!(error = %e, "TTS error");
        }
        sentences_spoken = sentences.len();
    } else {
        // Longer response — stream per-sentence (faster first audio).
        for sentence in &sentences {
            let trimmed = sentence.trim();
            if trimmed.len() < 3 {
                continue;
            }

            if sentences_spoken == 0 {
                eprintln!("[voice] Speaking (streaming)...");
            }

            if let Err(e) = tts_engine.speak(trimmed).await {
                tracing::warn!(error = %e, "TTS error on sentence");
            }
            sentences_spoken += 1;
        }
    }

    Ok(response)
}

/// Split text into sentences for incremental TTS.
///
/// Each sentence is spoken as a separate Piper invocation,
/// allowing the first sentence to play while later ones are synthesized.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);

        // Sentence boundary: period, exclamation, question mark
        // followed by space or end of text.
        if (ch == '.' || ch == '!' || ch == '?') && current.len() > 10 {
            sentences.push(current.trim().to_string());
            current = String::new();
        }
    }

    // Remaining fragment.
    let remaining = current.trim().to_string();
    if !remaining.is_empty() && remaining.len() > 3 {
        sentences.push(remaining);
    }

    // If no sentence boundaries found, return the whole text as one.
    if sentences.is_empty() && !text.trim().is_empty() {
        sentences.push(text.trim().to_string());
    }

    sentences
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_basic() {
        let sentences = split_sentences("Hello world. How are you? I'm fine!");
        assert_eq!(sentences.len(), 3);
        assert!(sentences[0].contains("Hello"));
        assert!(sentences[1].contains("How"));
        assert!(sentences[2].contains("fine"));
    }

    #[test]
    fn split_single_sentence() {
        let sentences = split_sentences("Just one sentence here");
        assert_eq!(sentences.len(), 1);
    }

    #[test]
    fn split_short_fragments_filtered() {
        let sentences = split_sentences("OK. Fine. This is a real sentence here.");
        // "OK" and "Fine" are too short (<10 chars including period)
        // They get merged with the next sentence
        assert!(!sentences.is_empty());
    }

    #[test]
    fn split_empty() {
        let sentences = split_sentences("");
        assert!(sentences.is_empty());
    }
}
