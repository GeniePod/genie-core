#!/usr/bin/env python3
"""GeniePod wake word listener — default development wake phrase detection.

Continuously listens on the USB mic (stereo 48kHz), resamples to 16kHz mono,
and detects the default development wake phrase ("Hey Jarvis")
using OpenWakeWord's TFLite model.

Usage:
    python3 genie-wakeword.py                    # standalone test
    python3 genie-wakeword.py --threshold 0.5    # adjust sensitivity
"""

import argparse
import sys
import socket
import numpy as np
import pyaudio
from scipy.signal import resample
from openwakeword.model import Model

# USB headphone mic is stereo-only at 48kHz. OpenWakeWord expects 16kHz mono.
MIC_RATE = 48000
MIC_CHANNELS = 2
TARGET_RATE = 16000
# 7680 stereo samples at 48kHz = 3840 mono = 1280 at 16kHz (80ms chunk)
FRAMES_PER_BUFFER = 7680
COOLDOWN_CHUNKS = 20  # ~1.6s cooldown after detection


def main():
    parser = argparse.ArgumentParser(description="GeniePod wake word listener")
    parser.add_argument("--threshold", type=float, default=0.5,
                        help="Detection confidence threshold (0.0-1.0, default 0.5)")
    parser.add_argument("--device-index", type=int, default=0,
                        help="PyAudio input device index (default 0 = USB headphone)")
    parser.add_argument("--socket", type=str, default="/run/geniepod/wakeword.sock",
                        help="Unix socket path to notify genie-core")
    parser.add_argument("--standalone", action="store_true",
                        help="Standalone mode — just print detections, no socket")
    args = parser.parse_args()

    # Load wake word model.
    print("[wakeword] Loading hey_jarvis model...", flush=True)
    model = Model(wakeword_models=["hey_jarvis"], inference_framework="tflite")
    print("[wakeword] Model loaded", flush=True)

    # Open microphone (stereo 48kHz — USB headphone hardware constraint).
    pa = pyaudio.PyAudio()
    stream = pa.open(
        format=pyaudio.paInt16,
        channels=MIC_CHANNELS,
        rate=MIC_RATE,
        input=True,
        input_device_index=args.device_index,
        frames_per_buffer=FRAMES_PER_BUFFER,
    )

    print(f"[wakeword] Listening for default dev wake phrase 'Hey Jarvis' (threshold={args.threshold})...",
          flush=True)

    cooldown = 0

    try:
        while True:
            raw = np.frombuffer(
                stream.read(FRAMES_PER_BUFFER, exception_on_overflow=False),
                dtype=np.int16,
            )

            # Stereo to mono (left channel).
            mono = raw[0::2].astype(np.float32)

            # Resample 48kHz -> 16kHz (proper anti-alias filter).
            audio_16k = resample(mono, len(mono) // 3).astype(np.int16)

            prediction = model.predict(audio_16k)
            score = prediction.get("hey_jarvis", 0.0)

            if cooldown > 0:
                cooldown -= 1
                continue

            if score > args.threshold:
                print(f"[wakeword] WAKE DETECTED (score={score:.3f})", flush=True)
                cooldown = COOLDOWN_CHUNKS
                model.reset()

                if not args.standalone:
                    notify_core(args.socket, score)

    except KeyboardInterrupt:
        print("\n[wakeword] Stopped", flush=True)
    finally:
        stream.stop_stream()
        stream.close()
        pa.terminate()


def notify_core(socket_path: str, score: float):
    """Send wake event to genie-core via Unix datagram socket."""
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        payload = f"WAKE {score:.3f}\n".encode()
        sock.sendto(payload, socket_path)
        sock.close()
    except Exception as e:
        print(f"[wakeword] notify: {e}", file=sys.stderr, flush=True)


if __name__ == "__main__":
    main()
