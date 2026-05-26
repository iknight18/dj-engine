# DJ Engine — Design

**Date:** 2026-05-26
**Status:** Approved for planning

## Summary

A two-program system that takes a directory of arbitrary audio files and continuously DJs them — beatmatched and harmonically mixed — by streaming the live mix to an existing Icecast server. The system is split into:

1. **`analyzer`** — a Python CLI that walks a music directory, extracts BPM / key / beats / energy / cue points for every track, and writes one JSON sidecar file per track to an output directory. Idempotent and resumable.
2. **`engine`** — a Rust binary that loads those sidecars at startup, then runs forever: picks the next track using harmonic/BPM/energy constraints, time-stretches it to match the current track, beat-aligns it, crossfades over 16 bars, and streams the resulting PCM into ffmpeg, which encodes it and pushes it to a user-provided Icecast server. Listeners connect via any HTTP audio client (browser, VLC).

Both halves communicate only through files on disk (the sidecar JSONs). They share no process, no IPC, and no runtime state. Either can be re-run or redeployed without touching the other.

## Goals

- Pleasant, continuous, beatmatched mix from an arbitrary music folder with no human input
- Handle a heterogeneous library gracefully (some tracks won't be DJ-friendly — variable BPM, unclear downbeats, vocal-only intros)
- Run natively for development on Windows; ship as two Docker images for Rancher deployment
- Day-to-day dev requires only Python 3.11+, Rust toolchain, and `ffmpeg` on PATH — no native library installs, no vcpkg, no build-system gymnastics
- The two halves have a clear, file-based contract that's easy to inspect and easy to evolve

## Non-Goals (v1)

- No scratching, no looping, no manual cueing, no effects beyond a basic EQ swap during crossfade
- No GUI, no TUI, no web dashboard — engine is autonomous
- No music recommendation beyond the in-set harmonic/BPM/energy algorithm (no external metadata, no listening history across runs)
- No re-encoding into multiple bitrates; one Icecast mount, one format
- No pure-local-speakers playback mode in the engine (the engine *only* streams to Icecast; for local listening, run a local Icecast and point a browser at it)

## Architecture

```
┌────────────────────┐    sidecar JSONs     ┌────────────────────┐
│  analyzer          │ ───────────────────▶ │   dj engine        │
│  Python, Docker    │   (one per track,    │   Rust, Docker     │
│  Rancher Job       │    in output PVC)    │   Rancher          │
│  (run on demand)   │                      │   Deployment       │
└────────────────────┘                      └────────────────────┘
        ▲                                            │
        │ reads (PVC, read-only)                     │ raw PCM via pipe
        │                                            ▼
   ┌──────────┐                              ┌────────────────────┐
   │ music    │                              │  ffmpeg subprocess │
   │ library  │                              │  (encodes + pushes │
   │ (PVC)    │                              │   Icecast source)  │
   └──────────┘                              └────────────────────┘
                                                       │
                                                       ▼ Icecast source
                                            ┌────────────────────────┐
                                            │  External Icecast      │
                                            │  (user-managed)        │
                                            │  default :27897        │
                                            └────────────────────────┘
                                                       │
                                                       ▼ HTTP listeners
                                                  browser / VLC
```

### Boundaries

- **Schema is the contract.** `schema/sidecar.schema.json` is the single source of truth for what a sidecar looks like. Both halves validate against it.
- **No live communication.** The engine never talks back to the analyzer. The analyzer never reads engine state. Each side is independently testable.
- **`ffmpeg` is the only native runtime dep on both sides.** Python uses it via `audioread` for decoding. Rust uses it via subprocesses for decoding, time-stretching, and Icecast push.

## Component 1 — Analyzer (Python)

### CLI

```
dj-analyze SOURCE_DIR OUTPUT_DIR [--workers N] [--force]
```

- `SOURCE_DIR`: recursively walked for audio files
- `OUTPUT_DIR`: receives one `.json` sidecar per track, mirroring relative paths to avoid collisions
- `--workers N`: parallel workers (default = CPU count − 1)
- `--force`: re-analyze every track even if a fresh sidecar exists

### Supported formats

`mp3`, `flac`, `m4a`/`aac`, `wav`, `ogg`, `opus` — anything `audioread` can decode via ffmpeg.

### Sidecar JSON schema

```json
{
  "schema_version": 1,
  "analyzer_version": "0.1.0",
  "source_path": "C:/music/Artist/Track.mp3",
  "source_mtime": 1715000000.0,
  "source_size_bytes": 7234112,
  "duration_seconds": 218.4,
  "sample_rate": 44100,

  "bpm": 124.0,
  "bpm_confidence": 0.92,
  "beats_seconds": [0.41, 0.89, 1.37, "..."],
  "downbeats_seconds": [0.41, 2.35, 4.29, "..."],

  "key": "8A",
  "key_label": "A minor",
  "key_confidence": 0.81,

  "energy_curve": [0.12, 0.18, 0.31, "..."],
  "energy_mean": 0.42,

  "cue_in_seconds": 0.41,
  "cue_out_seconds": 215.8,
  "intro_end_seconds": 32.5,
  "outro_start_seconds": 192.3,

  "is_mixable": true,
  "mixable_reasons": []
}
```

If decoding fails entirely, the sidecar is still written but contains `"error": "<message>"` instead of analysis fields. The engine ignores erroring sidecars at load time.

### Detection libraries

| Field | Library | Notes |
|---|---|---|
| BPM, beats | `librosa.beat.beat_track` | Confidence derived from tempo stability across windows |
| Downbeats | `madmom.features.downbeats.RNNDownBeatProcessor` | Heavier dep but gold standard; falls back to assuming 4/4 from beats if `madmom` not installed |
| Key | `essentia.standard.KeyExtractor` | Output mapped to Camelot wheel notation (e.g., `8A`) |
| Energy | `librosa.feature.rms` | Per-second windows, normalized 0..1 |
| Cue points | Custom over the energy curve + downbeats | `cue_in`/`cue_out` skip leading/trailing silence; `intro_end`/`outro_start` are 16-bar windows around the first/last sustained energy plateau |

### Incremental behavior

Before analyzing a track, check if a sidecar already exists at the expected output path **and** its `source_mtime` matches **and** its `source_size_bytes` matches **and** its `analyzer_version` matches the running code. If all four match → skip. Otherwise re-analyze and overwrite.

This makes re-runs cheap, makes adding new tracks to the source dir cheap, and ensures schema/algorithm changes invalidate stale sidecars.

### Mixability classification

A track is marked `is_mixable: false` (with reasons added to `mixable_reasons`) if any of:

- BPM confidence < 0.6 → `"bpm_low_confidence"`
- BPM varies > 5% across the track → `"bpm_variable"`
- No consistent 4/4 downbeat grid → `"no_clear_downbeat"`
- Duration < 90 seconds → `"too_short"`
- Key confidence < 0.4 → `"key_low_confidence"` (does **not** disqualify; engine just won't use harmonic constraint)

The engine still considers non-mixable tracks but uses a simple fade instead of beatmatching when one is selected.

### Parallelism

`multiprocessing.Pool` over the file list. Workers are independent. Memory ceiling per worker ≈ 200 MB (one decoded track held at a time).

### Observability

- `rich` progress bar for local interactive runs
- Structured JSON logs to stdout when `--log-format=json` is passed (used inside the Docker image so Rancher/Loki can ingest)

## Component 2 — DJ Engine (Rust)

### CLI

```
dj-engine --sidecars DIR --music-root DIR [--seed-track FILE] [--log-format json]
```

### Configuration (env vars)

| Var | Required | Default | Description |
|---|---|---|---|
| `ICECAST_URL` | yes | — | e.g., `icecast://localhost:27897/dj.mp3` |
| `ICECAST_USER` | no | `source` | Icecast source username |
| `ICECAST_PASSWORD` | yes | — | Icecast source password |
| `ICECAST_FORMAT` | no | `mp3` | `mp3` or `opus` |
| `ICECAST_BITRATE` | no | `192k` | Bitrate string passed to ffmpeg |
| `ICECAST_ADMIN_URL` | no | derived from `ICECAST_URL` | Used for metadata updates |
| `ICECAST_ADMIN_PASSWORD` | no | falls back to `ICECAST_PASSWORD` | Admin password for metadata updates |

### Startup sequence

1. Load every `*.json` under `--sidecars DIR` into memory.
2. Drop entries whose `source_path` is missing on disk; log a warning per drop.
3. Drop entries that have `error` set.
4. Spawn the **encoder ffmpeg** subprocess and verify the Icecast handshake by polling its stderr for the "Icecast" connection line within 10s. If it fails, retry with exponential backoff up to 60s, then exit non-zero.
5. Pick the seed track: `--seed-track` if provided, else weighted-random among mixable tracks whose `energy_mean` falls in `[0.3, 0.6]` (avoids opening with the highest-energy banger or a near-silent ambient piece).
6. Begin the mixer loop.

### Track selection algorithm

Runs ~30 seconds before each transition, on a background task.

**Filter:**

- Not in the last 20 played tracks (in-memory ring of recent IDs)
- BPM within ±6% of the currently playing track's BPM (the time-stretch range we trust)
- Camelot key compatible: identical, adjacent on the wheel (±1 step), or relative major/minor (number same, letter flipped)
- `is_mixable: true` strongly preferred

**Score** (sum of weighted terms, all in `[0, 1]`):

- **BPM closeness** (weight 1.0): let `p = abs(candidate_bpm - current_bpm) / current_bpm`. Score = `max(0, 1 - p / 0.06)`. At 0% delta → 1.0; at 6% delta → 0.0.
- **Key match** (weight 1.0): identical = 1.0, adjacent on the Camelot wheel (±1 step) = 0.7, relative major/minor (same number, letter flipped) = 0.6, else = 0.0.
- **Energy fit** (weight 0.5): `max(0, 1 - abs(candidate_energy_mean - target_energy)/0.15)`. `target_energy` is a moving average of recently played tracks' `energy_mean` that drifts up by ~0.02 per track until 0.7, then drifts down — gives the set a rise-and-fall arc.

**Pick:** weighted-random among the top 5 by score. Prevents identical setlists on repeat runs.

**Fallback chain** if no candidate passes filter:

1. Relax key constraint to "any"
2. Relax BPM to ±10%
3. Allow `is_mixable: false`

The fallback chain always produces a track; the engine never deadlocks.

### Playback bounds

For every track the engine plays, the decoder reads only between `cue_in_seconds` and `cue_out_seconds` — leading/trailing silence is always skipped. The seed track begins playback at its `cue_in_seconds`. Every subsequent track is brought in via the crossfade pipeline below.

### Mixing pipeline

The mixer runs ~30s ahead of real time. For each upcoming transition:

1. **Decode the upcoming track's mix window:** spawn a short-lived ffmpeg subprocess with `-ss intro_end_seconds - 16_bars` to `-to intro_end_seconds + 32_bars` + `-af atempo=<ratio>` to time-stretch on the way out. Capture the resulting f32le stereo PCM into an in-memory buffer.
2. **Compute alignment:** find the downbeat in the outgoing track at `outro_start_seconds`. Place the upcoming track so its `intro_end_seconds` downbeat lands at the outgoing downbeat + N bars (N = 16 by default).
3. **Crossfade** over those 16 bars using equal-power volume curves (`sin(t·π/2)` in, `cos(t·π/2)` out). PCM samples from both tracks are mixed sample-by-sample.
4. **Optional EQ swap** during the middle 8 bars: low-pass the outgoing's highs (cutoff sweeps from 20 kHz → 200 Hz) and high-pass the incoming's lows (sweeps the other direction). Implemented as biquad filters on f32 samples. Default off in v1 — can be enabled via env var once the basic flow is solid.
5. The mixed PCM goes into the output **ring buffer** (`ringbuf` crate, ~10s capacity).
6. Once the crossfade region is complete, the upcoming track becomes "current" and decoding continues from `intro_end_seconds + 32_bars` using a long-running ffmpeg subprocess.

### Output pipeline

- **Mixer thread** = ring buffer producer (writes 44.1 kHz stereo f32 PCM)
- **Writer thread** = ring buffer consumer (reads 4096-sample chunks, writes to encoder ffmpeg's stdin)
- Ring buffer size (~10s) absorbs any mixer-thread hiccup without underrunning the stream.

### Encoder ffmpeg invocation

```
ffmpeg -hide_banner -loglevel info \
       -f f32le -ar 44100 -ac 2 -i pipe:0 \
       -c:a libmp3lame -b:a $ICECAST_BITRATE \
       -content_type audio/mpeg \
       -f mp3 icecast://$ICECAST_USER:$ICECAST_PASSWORD@<host>:<port>/<mount>
```

(For `ICECAST_FORMAT=opus`: `-c:a libopus -b:a $ICECAST_BITRATE -f ogg ...`.)

### Time-stretch quality note

ffmpeg's `atempo` filter is WSOLA-based and clean at the ±6% BPM range we use. If higher quality is ever needed, the time-stretch subprocess can be swapped to `rubberband.exe stretch.wav out.wav --tempo <ratio>` (ship the binary in `engine/vendor/`) without changing any other code. Architecture stays identical.

### Icecast metadata updates

At the midpoint of each crossfade, the engine fires a `GET` to:

```
$ICECAST_ADMIN_URL/admin/metadata?mount=/<mount>&mode=updinfo&song=<title>
```

with `Authorization: Basic` (admin user/password). Fire-and-forget; metadata failures never block the mix.

### Resilience

- **Encoder ffmpeg crash or Icecast disconnect:** supervisor task restarts ffmpeg with exponential backoff (1s → 60s cap). The mixer keeps producing PCM during the outage; the ring buffer absorbs up to ~10s. Listeners hear a brief silence and the stream resumes.
- **Decoder ffmpeg failure mid-track:** mark the track unplayable for this session, immediately select another via the fallback chain.
- **No mixable tracks left after recency filter:** the recency ring auto-trims (oldest IDs drop out as new tracks play); eventually the filter succeeds.
- **Empty library at startup:** log fatal, exit non-zero.

### Crates

| Crate | Purpose |
|---|---|
| `ringbuf` | SPSC ring buffer between mixer and writer threads |
| `serde`, `serde_json` | Sidecar parsing (types match `schema/sidecar.schema.json`) |
| `clap` | CLI parsing |
| `tracing`, `tracing-subscriber` | Structured logs (JSON in prod, pretty locally) |
| `tokio` | Async runtime for subprocess orchestration + metadata HTTP |
| `reqwest` | Icecast metadata updates |
| `anyhow`, `thiserror` | Error handling |

No native linking. `cargo build` works on a clean Windows box with just `rustup` and `ffmpeg.exe` on PATH.

## Project Layout

```
DJ-Engine/
  schema/
    sidecar.schema.json
  analyzer/
    pyproject.toml
    src/dj_analyze/
      __main__.py
      detect_bpm.py
      detect_key.py
      detect_beats.py
      detect_energy.py
      detect_cues.py
      pipeline.py
      sidecar.py
    tests/
      fixtures/*.wav
      test_*.py
    Dockerfile
  engine/
    Cargo.toml
    src/
      main.rs
      library.rs
      selector.rs
      align.rs
      crossfade.rs
      mixer.rs
      icecast.rs
      ffmpeg.rs
      sidecar.rs
    tests/
      integration_mixer.rs
    Dockerfile
  deploy/
    analyzer-job.yaml
    engine-deployment.yaml
    icecast-secret.example.yaml
  docs/superpowers/specs/
  README.md
```

## Testing strategy

### Analyzer

- **Unit tests** per detection module against short reference WAVs with known properties (120 BPM click track, sine in A-minor, etc.) committed under `tests/fixtures/`.
- **Schema validation in CI:** every emitted sidecar is validated against `schema/sidecar.schema.json`.
- **Idempotency test:** running analyzer twice on the same input produces zero changes the second time.

### Engine

- **Selector unit tests:** synthetic libraries with hand-crafted sidecars exercise filter, score, fallback chain, and recency.
- **Align unit tests:** given two beat lists, the computed sample offset places downbeats on the same sample (±1 sample tolerance).
- **Crossfade unit tests:** equal-power curves sum to constant gain; biquad EQ math doesn't clip.
- **Mixer integration test:** two synthetic sine-wave "tracks" → run the full mixer with a stub ffmpeg (a process that just echoes PCM through) → assert output is continuous and the crossfade window contains both frequencies.
- **No live-audio test in CI.** Manual smoke procedure documented in README: start local Icecast on `:27897`, run engine against a 5-track test library, listen via VLC.

### CI

- Python: `ruff` + `mypy --strict` + `pytest`
- Rust: `cargo fmt --check` + `cargo clippy -D warnings` + `cargo test`
- Schema validation step run by both

## Deployment (Rancher)

Docker artifacts ship in the repo for production deployment to Rancher, but day-to-day development is native: Python and Rust both run directly on the host.

- **Analyzer image:** `python:3.11-slim` + system `ffmpeg` + `pip install dj_analyze`. Deployed as a Kubernetes `Job`. Mounts: music PVC (read-only), sidecar PVC (read-write). Exits when done.
- **Engine image:** multi-stage Rust build → `debian:stable-slim` runtime + `ffmpeg`. Deployed as a Kubernetes `Deployment` (replicas: 1; this is inherently a singleton). Mounts: sidecar PVC (read-only), music PVC (read-only). Reads Icecast config from a `Secret`.

## Default port

The engine connects to Icecast on `27897` by default. Chosen to avoid common defaults (8000/8080/8888/3000/5000) and to stay below the OS ephemeral range (49152+).

## Dev quickstart

1. Install Python 3.11+, Rust toolchain (`rustup`), `ffmpeg`, and Icecast (configured to listen on `:27897`).
2. `cd analyzer && pip install -e . && dj-analyze C:/path/to/music ./analyzed`
3. Start your Icecast.
4. `cd engine && cargo run -- --sidecars ../analyzed --music-root C:/path/to/music`
5. Open `http://localhost:27897/dj.mp3` in VLC.

## Open questions / future work

- **EQ swap** during crossfade is designed but disabled by default in v1. Enable once basic flow is stable.
- **Higher-quality time-stretch** via rubberband CLI subprocess is a one-line swap if `atempo` quality is unsatisfactory.
- **Listening-history persistence** across runs (avoid replaying the same favorites): not in v1.
- **Multi-format output** (one engine producing both mp3 and opus mounts): not in v1.
- **Cover art / extended metadata** in Icecast stream: not in v1.
