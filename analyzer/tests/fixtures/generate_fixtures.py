"""One-shot script to (re)generate synthetic audio fixtures with known properties.

Run from analyzer/ with the dev venv active:
    python tests/fixtures/generate_fixtures.py
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import soundfile as sf

SR = 44100
HERE = Path(__file__).parent


def _save(name: str, audio: np.ndarray) -> None:
    audio = np.clip(audio, -1.0, 1.0).astype(np.float32)
    sf.write(HERE / name, audio, SR, subtype="PCM_16")


def click_120bpm(duration_s: float = 8.0) -> None:
    """A click track at exactly 120 BPM (one click every 0.5s), 8 seconds.

    16 beats total → 4 downbeats at indices 0, 4, 8, 12.
    """
    n = int(duration_s * SR)
    audio = np.zeros(n, dtype=np.float32)
    click_len = int(0.005 * SR)  # 5ms click
    beat_interval_samples = SR // 2  # 0.5s
    envelope = np.linspace(1.0, 0.0, click_len, dtype=np.float32)
    for beat in range(int(duration_s * 2)):
        start = beat * beat_interval_samples
        end = start + click_len
        if end > n:
            break
        audio[start:end] += envelope
    _save("click_120bpm.wav", audio)


def sine_a_minor(duration_s: float = 8.0) -> None:
    """A-minor triad sustained tone: A3 + C4 + E4. Known key = 8A (Camelot)."""
    n = int(duration_s * SR)
    t = np.arange(n, dtype=np.float32) / SR
    a3 = 220.0
    c4 = 261.63
    e4 = 329.63
    audio = (
        0.25 * np.sin(2 * np.pi * a3 * t)
        + 0.25 * np.sin(2 * np.pi * c4 * t)
        + 0.25 * np.sin(2 * np.pi * e4 * t)
    )
    # Apply a 50ms fade-in and fade-out so the start/end aren't pure clicks.
    fade = int(0.05 * SR)
    audio[:fade] *= np.linspace(0, 1, fade)
    audio[-fade:] *= np.linspace(1, 0, fade)
    _save("sine_a_minor.wav", audio.astype(np.float32))


def silence_then_beep(duration_s: float = 10.0) -> None:
    """3s silence, 4s of a 1 kHz tone, 3s silence. Tests cue_in / cue_out trimming."""
    n = int(duration_s * SR)
    audio = np.zeros(n, dtype=np.float32)
    start = int(3.0 * SR)
    end = int(7.0 * SR)
    t = np.arange(end - start, dtype=np.float32) / SR
    audio[start:end] = 0.5 * np.sin(2 * np.pi * 1000.0 * t)
    _save("silence_then_beep.wav", audio)


def bass_only_80hz(duration_s: float = 6.0) -> None:
    """80 Hz sine — purely bass-band content. Energy should sit in `bass` band only."""
    n = int(duration_s * SR)
    t = np.arange(n, dtype=np.float32) / SR
    audio = 0.5 * np.sin(2 * np.pi * 80.0 * t)
    fade = int(0.05 * SR)
    audio[:fade] *= np.linspace(0, 1, fade)
    audio[-fade:] *= np.linspace(1, 0, fade)
    _save("bass_only_80hz.wav", audio.astype(np.float32))


def high_only_8khz(duration_s: float = 6.0) -> None:
    """8 kHz sine — purely high-band content. Energy should sit in `high` band only."""
    n = int(duration_s * SR)
    t = np.arange(n, dtype=np.float32) / SR
    audio = 0.5 * np.sin(2 * np.pi * 8000.0 * t)
    fade = int(0.05 * SR)
    audio[:fade] *= np.linspace(0, 1, fade)
    audio[-fade:] *= np.linspace(1, 0, fade)
    _save("high_only_8khz.wav", audio.astype(np.float32))


def corrupt_mp3() -> None:
    """A 1-byte 'mp3' file — guarantees a decode failure."""
    (HERE / "corrupt.mp3").write_bytes(b"\x00")


if __name__ == "__main__":
    click_120bpm()
    sine_a_minor()
    silence_then_beep()
    bass_only_80hz()
    high_only_8khz()
    corrupt_mp3()
    print("Fixtures regenerated in", HERE)
