//! Voice Activity Detection using Silero VAD via Python subprocess.
//!
//! Silero VAD is a 2.2 MB neural network with 99%+ accuracy.
//! Runs as a Python subprocess that reads a WAV file and outputs
//! the speech segments (start/end timestamps in ms).
//!
//! This approach avoids ONNX Runtime Rust FFI complexity while
//! delivering the same accuracy. The Python call adds ~200ms overhead
//! but runs AFTER recording is complete (not in the critical path).

use anyhow::Result;
use tokio::process::Command;

/// Detect speech segments in a WAV file using Silero VAD.
///
/// Returns (has_speech, speech_end_ms) — whether speech was found,
/// and the timestamp (in ms) where speech ends.
/// If speech_end_ms < total duration, the file can be trimmed.
pub async fn detect_speech(wav_path: &str) -> Result<(bool, u64)> {
    let output = Command::new("python3")
        .args([
            "-c",
            &format!(
                r#"
import sys, warnings
warnings.filterwarnings("ignore")
try:
    import torch
    model, utils = torch.hub.load(repo_or_dir='snakers4/silero-vad', model='silero_vad', trust_repo=True)
    (get_speech_timestamps, _, read_audio, _, _) = utils
    wav = read_audio('{}', sampling_rate=16000)
    timestamps = get_speech_timestamps(wav, model, sampling_rate=16000, threshold=0.5)
    if timestamps:
        last_end = timestamps[-1]['end']
        end_ms = int(last_end / 16)  # samples to ms at 16kHz
        print(f"SPEECH {{end_ms}}")
    else:
        print("SILENCE")
except Exception as e:
    print(f"ERROR {{e}}", file=sys.stderr)
    print("SILENCE")
"#,
                wav_path
            ),
        ])
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.trim();

    if line.starts_with("SPEECH") {
        let end_ms = line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        Ok((true, end_ms))
    } else {
        Ok((false, 0))
    }
}

/// Trim a WAV file to end at the specified millisecond.
///
/// Useful for removing trailing silence detected by VAD.
pub async fn trim_wav(wav_path: &str, end_ms: u64, sample_rate: u32) -> Result<()> {
    let data = tokio::fs::read(wav_path).await?;
    if data.len() <= 44 {
        return Ok(());
    }

    let bytes_per_ms = (sample_rate as u64 * 2) / 1000; // S16_LE mono
    let end_bytes = (end_ms * bytes_per_ms) as usize;

    // Add 500ms padding after speech end (don't cut too tight).
    let padding_bytes = (500 * bytes_per_ms) as usize;
    let trim_point = (end_bytes + padding_bytes).min(data.len() - 44);

    if trim_point >= data.len() - 44 {
        return Ok(()); // Nothing to trim.
    }

    // Rewrite WAV with trimmed data.
    let header = &data[..44];
    let pcm = &data[44..44 + trim_point];

    let data_size = pcm.len() as u32;
    let file_size = 36 + data_size;

    let mut output = header.to_vec();
    // Fix RIFF size.
    output[4..8].copy_from_slice(&file_size.to_le_bytes());
    // Fix data size.
    output[40..44].copy_from_slice(&data_size.to_le_bytes());
    output.extend_from_slice(pcm);

    tokio::fs::write(wav_path, &output).await?;

    tracing::info!(
        original_ms = (data.len() - 44) as u64 * 1000 / (sample_rate as u64 * 2),
        trimmed_ms = end_ms + 500,
        "VAD trimmed recording"
    );

    Ok(())
}

/// Check if Silero VAD is available (torch + silero-vad installed).
pub async fn is_available() -> bool {
    let output = Command::new("python3")
        .args(["-c", "import torch; print('OK')"])
        .output()
        .await;

    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("OK"),
        Err(_) => false,
    }
}
