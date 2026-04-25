use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// Whisper STT subprocess manager.
///
/// Spawns `whisper-server` (whisper.cpp HTTP server) or uses `whisper-cli`
/// for file-based transcription. Two modes:
///
/// 1. **Server mode** (production): whisper.cpp `--server` on localhost,
///    genie-core POSTs audio chunks via HTTP.
/// 2. **CLI mode** (prototype): pipes a WAV file to `whisper-cli`, reads text output.
///
/// On Jetson, whisper.cpp uses CUDA for GPU acceleration (~0.35x RTF).
pub struct SttEngine {
    mode: SttMode,
    model_path: String,
    /// Path to the whisper-cli binary.
    cli_path: String,
    /// Forced language or None for auto-detection.
    language_hint: Option<String>,
    /// Force CPU-only inference (--no-gpu). Required when LLM holds the GPU.
    no_gpu: bool,
    child: Option<Child>,
}

enum SttMode {
    /// whisper.cpp --server on a port, accepts audio via HTTP POST.
    Server { port: u16 },
    /// whisper CLI — transcribe individual WAV files.
    Cli,
}

/// Transcription result from STT.
#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    pub duration_ms: u64,
    pub language: Option<String>,
}

impl SttEngine {
    /// Create STT engine in server mode (production).
    pub fn server(model_path: &str, port: u16) -> Self {
        Self {
            mode: SttMode::Server { port },
            model_path: model_path.to_string(),
            cli_path: "whisper-server".to_string(),
            language_hint: None,
            no_gpu: false,
            child: None,
        }
    }

    /// Create STT engine in CLI mode (prototype/dev).
    pub fn cli(model_path: &str) -> Self {
        Self {
            mode: SttMode::Cli,
            model_path: model_path.to_string(),
            cli_path: "whisper-cli".to_string(),
            language_hint: None,
            no_gpu: false,
            child: None,
        }
    }

    /// Create STT engine in CLI mode with a custom binary path.
    pub fn cli_with_path(model_path: &str, cli_path: &str) -> Self {
        Self {
            mode: SttMode::Cli,
            model_path: model_path.to_string(),
            cli_path: cli_path.to_string(),
            language_hint: None,
            no_gpu: false,
            child: None,
        }
    }

    /// Create STT engine in CLI mode, CPU-only (for when LLM holds the GPU).
    pub fn cli_cpu(model_path: &str, cli_path: &str) -> Self {
        Self {
            mode: SttMode::Cli,
            model_path: model_path.to_string(),
            cli_path: cli_path.to_string(),
            language_hint: None,
            no_gpu: true,
            child: None,
        }
    }

    pub fn with_language_hint(mut self, language: Option<String>) -> Self {
        self.language_hint =
            language.and_then(|value| super::language::configured_language(&value));
        self
    }

    /// Start the whisper server subprocess (server mode only).
    pub async fn start_server(&mut self) -> Result<()> {
        if let SttMode::Server { port } = self.mode {
            tracing::info!(port, model = %self.model_path, "starting whisper server");

            let child = Command::new(&self.cli_path)
                .args([
                    "--model",
                    &self.model_path,
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &port.to_string(),
                    "--threads",
                    "2",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()?;

            self.child = Some(child);

            // Wait briefly for server to start.
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            tracing::info!(port, "whisper server started");
        }
        Ok(())
    }

    /// Transcribe a WAV file (works in both modes).
    pub async fn transcribe_file(&self, wav_path: &str) -> Result<Transcript> {
        let start = std::time::Instant::now();

        match &self.mode {
            SttMode::Server { port } => self.transcribe_via_server(*port, wav_path).await,
            SttMode::Cli => self.transcribe_via_cli(wav_path).await,
        }
        .map(|mut t| {
            t.duration_ms = start.elapsed().as_millis() as u64;
            t
        })
    }

    /// Transcribe raw PCM audio bytes (16kHz, 16-bit, mono).
    /// Writes to a temp WAV file, then transcribes.
    pub async fn transcribe_pcm(&self, pcm_data: &[u8], sample_rate: u32) -> Result<Transcript> {
        let tmp_path = format!("/tmp/geniepod-stt-{}.wav", std::process::id());
        write_wav(&tmp_path, pcm_data, sample_rate).await?;
        let result = self.transcribe_file(&tmp_path).await;
        let _ = tokio::fs::remove_file(&tmp_path).await;
        result
    }

    async fn transcribe_via_server(&self, port: u16, wav_path: &str) -> Result<Transcript> {
        // POST the WAV file to whisper server's /inference endpoint.
        let wav_data = tokio::fs::read(wav_path).await?;
        let addr = format!("127.0.0.1:{}", port);

        let boundary = "----GeniePodBoundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        );
        let body_end = format!("\r\n--{boundary}--\r\n");

        let content_length = body.len() + wav_data.len() + body_end.len();

        let stream = tokio::net::TcpStream::connect(&addr).await?;
        let (reader, mut writer) = stream.into_split();

        let request = format!(
            "POST /inference HTTP/1.1\r\nHost: {addr}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n"
        );

        writer.write_all(request.as_bytes()).await?;
        writer.write_all(body.as_bytes()).await?;
        writer.write_all(&wav_data).await?;
        writer.write_all(body_end.as_bytes()).await?;

        // Read response.
        let mut buf_reader = BufReader::new(reader);
        let mut response_body = String::new();
        let mut in_body = false;

        loop {
            let mut line = String::new();
            let n = buf_reader.read_line(&mut line).await?;
            if n == 0 {
                break;
            }
            if in_body {
                response_body.push_str(&line);
            } else if line.trim().is_empty() {
                in_body = true;
            }
        }

        // Parse whisper server JSON response.
        // Format: {"text": " Hello, turn on the lights."}
        let parsed: serde_json::Value = serde_json::from_str(response_body.trim())
            .unwrap_or_else(|_| serde_json::json!({"text": response_body.trim()}));

        let text = Self::clean_hallucinations(
            parsed
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim(),
        );
        let detected_language = super::language::detect_language_from_text(&text);

        Ok(Transcript {
            text,
            duration_ms: 0,
            language: parsed
                .get("language")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| self.language_hint.clone())
                .or(detected_language),
        })
    }

    async fn transcribe_via_cli(&self, wav_path: &str) -> Result<Transcript> {
        tracing::info!(cli = %self.cli_path, model = %self.model_path, file = wav_path, "running whisper-cli");

        // Drop page cache before CUDA allocation — NvMap needs contiguous blocks.
        let _ = Command::new("sh")
            .args([
                "-c",
                "sync && echo 3 > /proc/sys/vm/drop_caches 2>/dev/null",
            ])
            .output()
            .await;

        let mut args = vec![
            "-m".to_string(),
            self.model_path.clone(),
            "-f".to_string(),
            wav_path.to_string(),
            "--no-timestamps".to_string(),
            "--no-prints".to_string(),
            "--threads".to_string(),
            "4".to_string(),
            // Suppress non-speech tokens: prevents hallucinations like
            // [GUNFIRE], [coughing], (music), etc. on noisy/bleed audio.
            "--suppress-nst".to_string(),
            // Higher no-speech threshold: if confidence is low, output nothing.
            "--no-speech-thold".to_string(),
            "0.8".to_string(),
        ];

        if let Some(language) = &self.language_hint {
            args.push("--language".to_string());
            args.push(language.clone());
        }

        if self.no_gpu {
            args.push("--no-gpu".to_string());
        }

        let output = Command::new(&self.cli_path).args(&args).output().await?;

        // If GPU allocation fails (NvMap error), retry with --no-gpu.
        if !output.status.success() && !self.no_gpu {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("NvMap")
                || stderr.contains("cuda")
                || stderr.contains("failed to initialize")
            {
                tracing::warn!(
                    "GPU STT failed, retrying on CPU: {}",
                    stderr.lines().next().unwrap_or("")
                );
                args.push("--no-gpu".to_string());
                let retry = Command::new(&self.cli_path).args(&args).output().await?;
                if !retry.status.success() {
                    let stderr2 = String::from_utf8_lossy(&retry.stderr);
                    anyhow::bail!("whisper-cli failed (CPU retry): {}", stderr2);
                }
                let raw = String::from_utf8_lossy(&retry.stdout);
                let text = Self::clean_hallucinations(raw.trim());
                let language = self
                    .language_hint
                    .clone()
                    .or_else(|| super::language::detect_language_from_text(&text));
                tracing::info!(text = %text, mode = "cpu-fallback", "whisper transcription complete");
                return Ok(Transcript {
                    text,
                    duration_ms: 0,
                    language,
                });
            }
            anyhow::bail!("whisper-cli failed: {}", stderr);
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("whisper-cli failed: {}", stderr);
        }

        let raw = String::from_utf8_lossy(&output.stdout);
        let text = Self::clean_hallucinations(raw.trim());
        let language = self
            .language_hint
            .clone()
            .or_else(|| super::language::detect_language_from_text(&text));
        tracing::info!(text = %text, "whisper transcription complete");

        Ok(Transcript {
            text,
            duration_ms: 0,
            language,
        })
    }

    /// Strip common Whisper tiny hallucination artifacts from transcription.
    ///
    /// The tiny model hallucinates bracketed sound effects, parenthetical labels,
    /// and ghost phrases when it hears bleed audio or background noise.
    fn clean_hallucinations(text: &str) -> String {
        let mut result = text.to_string();

        // Strip [ANYTHING] and (ANYTHING) markers — regex-free.
        loop {
            if let Some(start) = result.find('[')
                && let Some(end) = result[start..].find(']')
            {
                result = format!("{}{}", &result[..start], &result[start + end + 1..]);
                continue;
            }
            if let Some(start) = result.find('(')
                && let Some(end) = result[start..].find(')')
            {
                result = format!("{}{}", &result[..start], &result[start + end + 1..]);
                continue;
            }
            break;
        }

        // Known ghost phrases the tiny model produces on near-silence or bleed.
        let ghosts = [
            "thank you",
            "thanks for watching",
            "good night",
            "goodbye",
            "i'm sorry",
            "you're welcome",
            "subscribe",
            "like and subscribe",
            "see you next time",
            "bye bye",
            "the end",
            "thank you for watching",
            "please subscribe",
            "thanks for listening",
        ];
        let lower = result.trim().to_lowercase();
        for ghost in &ghosts {
            if lower == *ghost {
                tracing::debug!(ghost = ghost, "filtered ghost phrase from tiny model");
                return String::new();
            }
        }

        // Collapse whitespace from removals.
        let result = result.split_whitespace().collect::<Vec<_>>().join(" ");
        result.trim().to_string()
    }

    /// Stop the server subprocess.
    pub async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            tracing::info!("whisper server stopped");
        }
    }
}

/// Flush the mic buffer by recording and discarding a short burst.
/// Prevents TTS playback residue from appearing in the next recording.
async fn flush_mic_buffer(device: &str, sample_rate: u32) {
    let flush_path = format!("/tmp/geniepod-flush-{}.wav", std::process::id());
    let _ = Command::new("arecord")
        .args([
            "-D",
            device,
            "-f",
            "S16_LE",
            "-r",
            &sample_rate.to_string(),
            "-c",
            "1",
            "-d",
            "2",
            &flush_path,
        ])
        .output()
        .await;
    let _ = tokio::fs::remove_file(&flush_path).await;
}

/// Record audio with fixed duration.
///
/// Returns the path to the recorded WAV file.
pub async fn record_audio(device: &str, sample_rate: u32, duration_secs: u32) -> Result<String> {
    let wav_path = format!("/tmp/geniepod-rec-{}.wav", std::process::id());

    tracing::info!(
        device,
        sample_rate,
        duration_secs,
        "recording audio via arecord"
    );

    let output = Command::new("arecord")
        .args([
            "-D",
            device,
            "-f",
            "S16_LE",
            "-r",
            &sample_rate.to_string(),
            "-c",
            "1",
            "-d",
            &duration_secs.to_string(),
            &wav_path,
        ])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("arecord failed: {}", stderr);
    }

    // Verify file has actual audio data (not just a 44-byte header).
    let metadata = tokio::fs::metadata(&wav_path).await?;
    if metadata.len() <= 44 {
        anyhow::bail!(
            "recording produced empty audio ({} bytes) — check mic device {}",
            metadata.len(),
            device
        );
    }

    tracing::info!(
        path = %wav_path,
        size_bytes = metadata.len(),
        "recording complete"
    );

    Ok(wav_path)
}

/// Write raw PCM data as a WAV file.
async fn write_wav(path: &str, pcm: &[u8], sample_rate: u32) -> Result<()> {
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_size = pcm.len() as u32;
    let file_size = 36 + data_size;

    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&file_size.to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&bits_per_sample.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_size.to_le_bytes());

    let mut file_data = header;
    file_data.extend_from_slice(pcm);
    tokio::fs::write(path, &file_data).await?;
    Ok(())
}

/// Spawn a background task that captures audio via ALSA and sends
/// transcripts through a channel. This is the production audio pipeline.
///
/// Not yet implemented — requires ALSA bindings (cpal or alsa-rs crate).
/// For now, the orchestrator uses stdin as input source.
pub fn spawn_audio_pipeline(_stt: Arc<SttEngine>) -> mpsc::Receiver<Transcript> {
    let (_tx, rx) = mpsc::channel(16);
    // TODO: ALSA capture → VAD → chunk → STT → tx.send(transcript)
    tracing::warn!("audio pipeline not yet implemented — using stdin mode");
    rx
}

use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn write_wav_header() {
        let pcm = vec![0u8; 32000]; // 1 second of 16kHz 16-bit mono
        let path = format!("/tmp/geniepod-stt-test-{}.wav", std::process::id());
        write_wav(&path, &pcm, 16000).await.unwrap();

        let data = tokio::fs::read(&path).await.unwrap();
        assert_eq!(&data[0..4], b"RIFF");
        assert_eq!(&data[8..12], b"WAVE");
        assert_eq!(&data[12..16], b"fmt ");

        // Sample rate at offset 24.
        let sr = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
        assert_eq!(sr, 16000);

        let _ = tokio::fs::remove_file(&path).await;
    }

    #[test]
    fn create_cli_engine() {
        let engine = SttEngine::cli("/opt/geniepod/models/whisper-small.bin");
        assert_eq!(engine.model_path, "/opt/geniepod/models/whisper-small.bin");
        assert_eq!(engine.language_hint, None);
    }

    #[test]
    fn create_cli_engine_with_path() {
        let engine =
            SttEngine::cli_with_path("/opt/geniepod/models/whisper-small.bin", "/usr/bin/whisper");
        assert_eq!(engine.cli_path, "/usr/bin/whisper");
    }

    #[test]
    fn create_cli_engine_with_language_hint() {
        let engine = SttEngine::cli("/opt/geniepod/models/whisper-small.bin")
            .with_language_hint(Some("de-DE".into()));
        assert_eq!(engine.language_hint.as_deref(), Some("de"));
    }

    #[test]
    fn create_server_engine() {
        let engine = SttEngine::server("/opt/geniepod/models/whisper-small.bin", 8178);
        if let SttMode::Server { port } = engine.mode {
            assert_eq!(port, 8178);
        } else {
            panic!("expected server mode");
        }
    }

    #[test]
    fn clean_hallucinations_brackets() {
        assert_eq!(SttEngine::clean_hallucinations("[GUNFIRE] hello"), "hello");
        assert_eq!(
            SttEngine::clean_hallucinations("hi [coughing] there"),
            "hi there"
        );
        assert_eq!(SttEngine::clean_hallucinations("(music) test"), "test");
        assert_eq!(SttEngine::clean_hallucinations("[BLANK_AUDIO]"), "");
    }

    #[test]
    fn clean_hallucinations_ghost_phrases() {
        assert_eq!(SttEngine::clean_hallucinations("Thank you"), "");
        assert_eq!(SttEngine::clean_hallucinations("good night"), "");
        assert_eq!(SttEngine::clean_hallucinations("Thanks for watching"), "");
        assert_eq!(SttEngine::clean_hallucinations("I'm sorry"), "");
        assert_eq!(SttEngine::clean_hallucinations("Goodbye"), "");
    }

    #[test]
    fn clean_hallucinations_preserves_real_speech() {
        assert_eq!(
            SttEngine::clean_hallucinations("turn on the lights"),
            "turn on the lights"
        );
        assert_eq!(
            SttEngine::clean_hallucinations("what's the weather like"),
            "what's the weather like"
        );
    }
}
