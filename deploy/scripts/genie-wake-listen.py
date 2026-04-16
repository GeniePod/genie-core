#!/usr/bin/env python3
"""GeniePod wake word listener — persistent process, releases mic on detection.

Protocol:
  1. Opens mic, loads model, prints "LISTENING"
  2. On detection: CLOSES mic, prints "WAKE <score>"
  3. Waits for "READY\n" on stdin
  4. Re-opens mic, resumes listening
"""

# Suppress all warnings BEFORE imports (scipy/numpy version warnings go to stdout)
import warnings
warnings.filterwarnings("ignore")

import os
import sys

# Suppress ALSA warnings (C-level output to stderr)
# Redirect stderr to /dev/null during PyAudio init
_stderr_fd = os.dup(2)
_devnull = os.open(os.devnull, os.O_WRONLY)
os.dup2(_devnull, 2)

import argparse
import numpy as np
import pyaudio
from scipy.signal import resample
from openwakeword.model import Model

# Restore stderr after PyAudio loaded
os.dup2(_stderr_fd, 2)
os.close(_devnull)
os.close(_stderr_fd)

MIC_RATE = 48000
MIC_CHANNELS = 2
FRAMES_PER_BUFFER = 7680


def open_mic(pa, device_index):
    # Suppress ALSA warnings during mic open
    stderr_fd = os.dup(2)
    devnull = os.open(os.devnull, os.O_WRONLY)
    os.dup2(devnull, 2)
    try:
        stream = pa.open(
            format=pyaudio.paInt16,
            channels=MIC_CHANNELS,
            rate=MIC_RATE,
            input=True,
            input_device_index=device_index,
            frames_per_buffer=FRAMES_PER_BUFFER,
        )
    finally:
        os.dup2(stderr_fd, 2)
        os.close(devnull)
        os.close(stderr_fd)
    return stream


def find_usb_mic(pa):
    """Auto-detect USB microphone device index."""
    usb_keywords = ["usb", "lenovo", "headphone", "headset", "microphone"]
    for i in range(pa.get_device_count()):
        try:
            info = pa.get_device_info_by_index(i)
            if info["maxInputChannels"] >= 2:
                name = info["name"].lower()
                if any(kw in name for kw in usb_keywords):
                    return i
        except Exception:
            continue
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--threshold", type=float, default=0.3)
    parser.add_argument("--device-index", type=int, default=-1)
    args = parser.parse_args()

    model = Model(wakeword_models=["hey_jarvis"], inference_framework="tflite")
    pa = pyaudio.PyAudio()

    # Auto-detect USB mic if device index not specified
    device_index = args.device_index
    if device_index < 0:
        device_index = find_usb_mic(pa)
        if device_index is None:
            print("ERROR: no USB microphone found", flush=True)
            sys.exit(1)

    stream = open_mic(pa, device_index)

    print("LISTENING", flush=True)

    try:
        while True:
            # Listen for wake word.
            while True:
                raw = np.frombuffer(
                    stream.read(FRAMES_PER_BUFFER, exception_on_overflow=False),
                    dtype=np.int16,
                )
                mono = raw[0::2].astype(np.float32)
                audio_16k = resample(mono, len(mono) // 3).astype(np.int16)

                pred = model.predict(audio_16k)
                score = pred.get("hey_jarvis", 0.0)

                if score > args.threshold:
                    # Release mic so arecord can use it.
                    stream.stop_stream()
                    stream.close()
                    print(f"WAKE {score:.3f}", flush=True)
                    model.reset()
                    break

            # Wait for parent to send READY.
            try:
                line = sys.stdin.readline().strip()
                if not line or line == "QUIT":
                    break
            except EOFError:
                break

            # Re-open mic for next cycle.
            stream = open_mic(pa, device_index)

    except KeyboardInterrupt:
        pass
    finally:
        try:
            stream.stop_stream()
            stream.close()
        except Exception:
            pass
        pa.terminate()


if __name__ == "__main__":
    main()
