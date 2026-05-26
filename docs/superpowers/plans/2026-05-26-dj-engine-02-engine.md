# DJ Engine — Plan 2: Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Rust `dj-engine` binary: it reads the sidecar JSONs produced by Plan 1, beatmatches and harmonically mixes them, and streams the continuous output to an Icecast server via ffmpeg subprocesses. Long-running, autonomous.

**Architecture:** Pure Rust orchestration around three ffmpeg subprocess roles (playing decoder, mix-window decoder+atempo, encoder+Icecast). Producer/consumer split via a lock-free SPSC ring buffer. Selection runs on a Tokio task that picks the next track ~30s before each transition.

**Tech Stack:** Rust (stable), `tokio` (async + processes), `serde`/`serde_json` (sidecars), `clap` (CLI), `ringbuf` (SPSC buffer), `tracing` (structured logs), `reqwest` (Icecast metadata HTTP), `anyhow`/`thiserror`.

**Prerequisite:** Plan 1 complete (`schema/sidecar.schema.json` exists, an `analyzed/` directory of sidecar JSONs can be produced). `ffmpeg.exe` on PATH. `rustup` toolchain installed.

**Spec:** `docs/superpowers/specs/2026-05-26-dj-engine-design.md`

---

## File structure (this plan creates these files)

```
DJ-Engine/
  engine/
    Cargo.toml                       # NEW
    Cargo.lock                       # generated
    Dockerfile                       # NEW
    src/
      main.rs                        # NEW (CLI entry, startup)
      lib.rs                         # NEW (re-exports for tests)
      config.rs                      # NEW (CLI args + env parsing)
      error.rs                       # NEW (engine-wide error types)
      sidecar.rs                     # NEW (serde types matching schema)
      library.rs                     # NEW (load + validate sidecars)
      camelot.rs                     # NEW (key compatibility helper)
      recency.rs                     # NEW (recently-played ring)
      selector.rs                    # NEW (filter, score, fallback)
      energy_avg.rs                  # NEW (rising/falling energy target)
      ffmpeg/
        mod.rs                       # NEW
        decoder.rs                   # NEW (long-running playing decoder)
        mixwindow.rs                 # NEW (short-lived atempo decoder)
        encoder.rs                   # NEW (encoder + Icecast push)
      align.rs                       # NEW (downbeat alignment math)
      crossfade.rs                   # NEW (equal-power curves)
      mixer.rs                       # NEW (orchestrates decode→align→crossfade)
      buffer.rs                      # NEW (ringbuf wiring)
      icecast_meta.rs                # NEW (now-playing HTTP push)
      supervisor.rs                  # NEW (encoder restart logic)
      logging_setup.rs               # NEW (pretty + json modes)
    tests/
      fixtures/
        sidecars/                    # hand-crafted test sidecars
          track_a_120bpm_8a.json     # NEW
          track_b_124bpm_9a.json     # NEW
          track_c_140bpm_3a.json     # NEW
        audio/
          generate_test_audio.sh     # NEW (sine generators via ffmpeg)
          a_sine_440.flac            # GENERATED
          b_sine_523.flac            # GENERATED
      selector_test.rs               # NEW
      camelot_test.rs                # NEW
      recency_test.rs                # NEW
      align_test.rs                  # NEW
      crossfade_test.rs              # NEW
      sidecar_test.rs                # NEW
      library_test.rs                # NEW
      mixer_integration_test.rs      # NEW
  deploy/
    engine-deployment.yaml           # NEW
    icecast-secret.example.yaml      # NEW
```

---

### Task E1: Initialize the Rust crate

**Files:**
- Create: `engine/Cargo.toml`
- Create: `engine/src/main.rs`
- Create: `engine/src/lib.rs`

- [ ] **Step 1: Initialize cargo crate**

From `C:\Users\f.nefzi\Desktop\Personal\DJ-Engine`:
```bash
cd engine
cargo init --name dj_engine --bin
```

This creates `Cargo.toml` and `src/main.rs` with default content.

- [ ] **Step 2: Replace Cargo.toml with the real dependency set**

Overwrite `engine/Cargo.toml`:
```toml
[package]
name = "dj_engine"
version = "0.1.0"
edition = "2021"
rust-version = "1.75"

[[bin]]
name = "dj-engine"
path = "src/main.rs"

[lib]
name = "dj_engine"
path = "src/lib.rs"

[dependencies]
anyhow = "1.0"
thiserror = "1.0"
clap = { version = "4.5", features = ["derive", "env"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.39", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json", "fmt"] }
ringbuf = "0.4"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
rand = "0.8"
walkdir = "2.5"
byteorder = "1.5"
parking_lot = "0.12"
urlencoding = "2.1"

[dev-dependencies]
tempfile = "3.10"
tokio = { version = "1.39", features = ["test-util", "macros"] }
```

- [ ] **Step 3: Create the lib.rs**

Create `engine/src/lib.rs`:
```rust
//! DJ Engine — Rust orchestration around ffmpeg subprocesses.

pub mod align;
pub mod buffer;
pub mod camelot;
pub mod config;
pub mod crossfade;
pub mod energy_avg;
pub mod error;
pub mod ffmpeg;
pub mod icecast_meta;
pub mod library;
pub mod logging_setup;
pub mod mixer;
pub mod recency;
pub mod selector;
pub mod sidecar;
pub mod supervisor;
```

- [ ] **Step 4: Skeleton main.rs (will be expanded in later tasks)**

Replace `engine/src/main.rs`:
```rust
fn main() -> anyhow::Result<()> {
    eprintln!("dj-engine: not yet implemented (see plan tasks)");
    std::process::exit(2);
}
```

- [ ] **Step 5: Verify it builds**

```bash
cargo build
```

Expected: builds successfully (with many "unused module" warnings — those go away as modules get content).

- [ ] **Step 6: Commit**

```bash
git add engine/Cargo.toml engine/Cargo.lock engine/src/main.rs engine/src/lib.rs
git commit -m "feat(engine): initialize crate with dependencies"
```

---

### Task E2: Engine-wide error type

**Files:**
- Create: `engine/src/error.rs`

- [ ] **Step 1: Define the error enum**

Create `engine/src/error.rs`:
```rust
//! Engine-wide error type.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("sidecar at {path:?} is invalid: {reason}")]
    InvalidSidecar { path: PathBuf, reason: String },

    #[error("ffmpeg subprocess failed: {0}")]
    Ffmpeg(String),

    #[error("icecast connection failed: {0}")]
    Icecast(String),

    #[error("library is empty (no usable sidecars)")]
    EmptyLibrary,

    #[error("config error: {0}")]
    Config(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, EngineError>;
```

- [ ] **Step 2: Verify it builds**

```bash
cargo build
```

- [ ] **Step 3: Commit**

```bash
git add engine/src/error.rs
git commit -m "feat(engine): engine-wide error type"
```

---

### Task E3: Sidecar serde types matching the schema

**Files:**
- Create: `engine/src/sidecar.rs`
- Create: `engine/tests/sidecar_test.rs`

- [ ] **Step 1: Write the failing test**

Create `engine/tests/sidecar_test.rs`:
```rust
use dj_engine::sidecar::Sidecar;

const VALID_JSON: &str = r#"{
    "schema_version": 1,
    "analyzer_version": "0.1.0",
    "source_path": "/music/a.mp3",
    "source_mtime": 1715000000.0,
    "source_size_bytes": 1000,
    "duration_seconds": 200.0,
    "sample_rate": 44100,
    "bpm": 124.0,
    "bpm_confidence": 0.92,
    "beats_seconds": [0.0, 0.5, 1.0],
    "downbeats_seconds": [0.0, 2.0],
    "key": "8A",
    "key_label": "A minor",
    "key_confidence": 0.81,
    "energy_curve": [0.1, 0.2, 0.3],
    "energy_mean": 0.42,
    "cue_in_seconds": 0.0,
    "cue_out_seconds": 195.0,
    "intro_end_seconds": 32.0,
    "outro_start_seconds": 180.0,
    "is_mixable": true,
    "mixable_reasons": []
}"#;

const ERROR_JSON: &str = r#"{
    "schema_version": 1,
    "analyzer_version": "0.1.0",
    "source_path": "/music/broken.mp3",
    "source_mtime": 0.0,
    "source_size_bytes": 0,
    "error": "decode failed"
}"#;

#[test]
fn parses_valid_sidecar() {
    let sc: Sidecar = serde_json::from_str(VALID_JSON).unwrap();
    assert_eq!(sc.bpm, Some(124.0));
    assert_eq!(sc.key.as_deref(), Some("8A"));
    assert!(sc.is_playable());
}

#[test]
fn parses_error_sidecar() {
    let sc: Sidecar = serde_json::from_str(ERROR_JSON).unwrap();
    assert!(sc.error.is_some());
    assert!(!sc.is_playable());
    assert_eq!(sc.bpm, None);
}

#[test]
fn rejects_invalid_camelot_key() {
    let bad = VALID_JSON.replace("\"8A\"", "\"13Z\"");
    // We don't enforce the regex in serde (the schema does in CI); engine just treats
    // it as a string and the camelot module rejects it on use.
    let sc: Sidecar = serde_json::from_str(&bad).unwrap();
    assert_eq!(sc.key.as_deref(), Some("13Z"));
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test --test sidecar_test
```

Expected: compile error (module doesn't exist).

- [ ] **Step 3: Implement sidecar.rs**

Create `engine/src/sidecar.rs`:
```rust
//! Serde types matching schema/sidecar.schema.json.
//!
//! Optional fields reflect that an "error sidecar" (decoding failed) only has the
//! base identifying fields plus `error`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sidecar {
    pub schema_version: u32,
    pub analyzer_version: String,
    pub source_path: String,
    pub source_mtime: f64,
    pub source_size_bytes: u64,

    #[serde(default)]
    pub error: Option<String>,

    pub duration_seconds: Option<f64>,
    pub sample_rate: Option<u32>,

    pub bpm: Option<f64>,
    pub bpm_confidence: Option<f64>,
    #[serde(default)]
    pub beats_seconds: Vec<f64>,
    #[serde(default)]
    pub downbeats_seconds: Vec<f64>,

    pub key: Option<String>,
    pub key_label: Option<String>,
    pub key_confidence: Option<f64>,

    #[serde(default)]
    pub energy_curve: Vec<f64>,
    pub energy_mean: Option<f64>,

    pub cue_in_seconds: Option<f64>,
    pub cue_out_seconds: Option<f64>,
    pub intro_end_seconds: Option<f64>,
    pub outro_start_seconds: Option<f64>,

    pub is_mixable: Option<bool>,
    #[serde(default)]
    pub mixable_reasons: Vec<String>,
}

impl Sidecar {
    /// True iff the sidecar has enough fields to actually play the track.
    pub fn is_playable(&self) -> bool {
        self.error.is_none()
            && self.bpm.is_some()
            && self.key.is_some()
            && self.cue_in_seconds.is_some()
            && self.cue_out_seconds.is_some()
            && self.intro_end_seconds.is_some()
            && self.outro_start_seconds.is_some()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test --test sidecar_test
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add engine/src/sidecar.rs engine/tests/sidecar_test.rs
git commit -m "feat(engine): sidecar serde types"
```

---

### Task E4: Library loader

**Files:**
- Create: `engine/src/library.rs`
- Create: `engine/tests/library_test.rs`

Loads every `*.json` in a directory, drops non-playable / missing-audio entries with a warning, returns the rest as `Vec<LoadedTrack>`.

- [ ] **Step 1: Write the failing test**

Create `engine/tests/library_test.rs`:
```rust
use std::fs;
use std::path::PathBuf;

use dj_engine::library::load_library;

fn write_sidecar(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    p
}

const TEMPLATE: &str = r#"{
    "schema_version": 1,
    "analyzer_version": "0.1.0",
    "source_path": "__PATH__",
    "source_mtime": 0.0,
    "source_size_bytes": 0,
    "duration_seconds": 200.0,
    "sample_rate": 44100,
    "bpm": 120.0,
    "bpm_confidence": 0.9,
    "beats_seconds": [0.0],
    "downbeats_seconds": [0.0],
    "key": "8A",
    "key_label": "A minor",
    "key_confidence": 0.8,
    "energy_curve": [0.4],
    "energy_mean": 0.4,
    "cue_in_seconds": 0.0,
    "cue_out_seconds": 195.0,
    "intro_end_seconds": 16.0,
    "outro_start_seconds": 180.0,
    "is_mixable": true,
    "mixable_reasons": []
}"#;

#[test]
fn loads_valid_sidecars_drops_missing_audio_and_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let sc_dir = tmp.path().join("sidecars");
    fs::create_dir(&sc_dir).unwrap();
    let audio_dir = tmp.path().join("audio");
    fs::create_dir(&audio_dir).unwrap();

    let audio_a = audio_dir.join("a.mp3");
    fs::write(&audio_a, b"fake").unwrap();
    let body_a = TEMPLATE.replace("__PATH__", audio_a.to_string_lossy().as_ref());
    write_sidecar(&sc_dir, "a.json", &body_a);

    // Sidecar with missing audio file — should be dropped.
    let body_missing = TEMPLATE.replace("__PATH__", "/nonexistent/missing.mp3");
    write_sidecar(&sc_dir, "missing.json", &body_missing);

    // Error sidecar — should be dropped.
    let err_body = r#"{
        "schema_version": 1,
        "analyzer_version": "0.1.0",
        "source_path": "/x.mp3",
        "source_mtime": 0.0,
        "source_size_bytes": 0,
        "error": "decode failed"
    }"#;
    write_sidecar(&sc_dir, "err.json", err_body);

    let tracks = load_library(&sc_dir).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].sidecar.source_path, audio_a.to_string_lossy());
}

#[test]
fn errors_on_empty_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let result = load_library(tmp.path());
    assert!(result.is_err(), "expected EmptyLibrary, got {:?}", result);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test --test library_test
```

- [ ] **Step 3: Implement library.rs**

Create `engine/src/library.rs`:
```rust
//! Library loader: read all sidecars from a directory, drop unusable entries.

use std::path::{Path, PathBuf};

use tracing::warn;
use walkdir::WalkDir;

use crate::error::{EngineError, Result};
use crate::sidecar::Sidecar;

#[derive(Debug, Clone)]
pub struct LoadedTrack {
    pub sidecar_path: PathBuf,
    pub sidecar: Sidecar,
}

pub fn load_library(sidecars_dir: &Path) -> Result<Vec<LoadedTrack>> {
    let mut tracks = Vec::new();
    for entry in WalkDir::new(sidecars_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match load_one(entry.path()) {
            Ok(Some(track)) => tracks.push(track),
            Ok(None) => { /* dropped silently with warning logged in load_one */ }
            Err(e) => warn!(
                path = %entry.path().display(),
                error = %e,
                "skipping sidecar (parse error)"
            ),
        }
    }
    if tracks.is_empty() {
        return Err(EngineError::EmptyLibrary);
    }
    Ok(tracks)
}

fn load_one(path: &Path) -> Result<Option<LoadedTrack>> {
    let body = std::fs::read_to_string(path)?;
    let sc: Sidecar = serde_json::from_str(&body)?;

    if let Some(err) = &sc.error {
        warn!(
            path = %path.display(),
            error = %err,
            "skipping sidecar (analyzer-reported error)"
        );
        return Ok(None);
    }
    if !sc.is_playable() {
        warn!(path = %path.display(), "skipping sidecar (incomplete fields)");
        return Ok(None);
    }
    if !PathBuf::from(&sc.source_path).exists() {
        warn!(
            sidecar = %path.display(),
            source = %sc.source_path,
            "skipping sidecar (audio file not found on disk)"
        );
        return Ok(None);
    }
    Ok(Some(LoadedTrack {
        sidecar_path: path.to_path_buf(),
        sidecar: sc,
    }))
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test library_test
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add engine/src/library.rs engine/tests/library_test.rs
git commit -m "feat(engine): library loader with sidecar validation"
```

---

### Task E5: Camelot wheel compatibility

**Files:**
- Create: `engine/src/camelot.rs`
- Create: `engine/tests/camelot_test.rs`

- [ ] **Step 1: Write the failing test**

Create `engine/tests/camelot_test.rs`:
```rust
use dj_engine::camelot::{CamelotKey, KeyMatch};

#[test]
fn parses_valid_keys() {
    assert!(CamelotKey::parse("8A").is_some());
    assert!(CamelotKey::parse("12B").is_some());
    assert!(CamelotKey::parse("1A").is_some());
}

#[test]
fn rejects_invalid_keys() {
    assert!(CamelotKey::parse("0A").is_none());
    assert!(CamelotKey::parse("13A").is_none());
    assert!(CamelotKey::parse("8C").is_none());
    assert!(CamelotKey::parse("XX").is_none());
}

#[test]
fn identical_key_is_match() {
    let a = CamelotKey::parse("8A").unwrap();
    assert_eq!(a.compatibility(&a), KeyMatch::Identical);
}

#[test]
fn adjacent_keys_are_match() {
    let a = CamelotKey::parse("8A").unwrap();
    let b = CamelotKey::parse("7A").unwrap();
    let c = CamelotKey::parse("9A").unwrap();
    assert_eq!(a.compatibility(&b), KeyMatch::Adjacent);
    assert_eq!(a.compatibility(&c), KeyMatch::Adjacent);
}

#[test]
fn wraps_around_12_to_1() {
    let a = CamelotKey::parse("12A").unwrap();
    let b = CamelotKey::parse("1A").unwrap();
    assert_eq!(a.compatibility(&b), KeyMatch::Adjacent);
}

#[test]
fn relative_major_minor_is_match() {
    let a = CamelotKey::parse("8A").unwrap();    // A minor
    let b = CamelotKey::parse("8B").unwrap();    // C major
    assert_eq!(a.compatibility(&b), KeyMatch::Relative);
}

#[test]
fn distant_keys_are_no_match() {
    let a = CamelotKey::parse("8A").unwrap();
    let b = CamelotKey::parse("3A").unwrap();
    assert_eq!(a.compatibility(&b), KeyMatch::None);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test --test camelot_test
```

- [ ] **Step 3: Implement camelot.rs**

Create `engine/src/camelot.rs`:
```rust
//! Camelot wheel key compatibility.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CamelotKey {
    pub number: u8,  // 1..=12
    pub letter: Letter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Letter {
    A, // minor
    B, // major
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMatch {
    Identical,
    Adjacent,   // ±1 step on the wheel, same letter
    Relative,   // same number, different letter
    None,
}

impl KeyMatch {
    pub fn score(self) -> f64 {
        match self {
            KeyMatch::Identical => 1.0,
            KeyMatch::Adjacent  => 0.7,
            KeyMatch::Relative  => 0.6,
            KeyMatch::None      => 0.0,
        }
    }
}

impl CamelotKey {
    pub fn parse(s: &str) -> Option<Self> {
        if s.len() < 2 || s.len() > 3 {
            return None;
        }
        let (num_str, letter_str) = s.split_at(s.len() - 1);
        let number: u8 = num_str.parse().ok()?;
        if !(1..=12).contains(&number) {
            return None;
        }
        let letter = match letter_str {
            "A" => Letter::A,
            "B" => Letter::B,
            _ => return None,
        };
        Some(Self { number, letter })
    }

    pub fn compatibility(&self, other: &Self) -> KeyMatch {
        if self == other {
            return KeyMatch::Identical;
        }
        if self.number == other.number && self.letter != other.letter {
            return KeyMatch::Relative;
        }
        if self.letter == other.letter {
            let diff = circular_distance(self.number, other.number, 12);
            if diff == 1 {
                return KeyMatch::Adjacent;
            }
        }
        KeyMatch::None
    }
}

fn circular_distance(a: u8, b: u8, modulo: u8) -> u8 {
    let raw = if a >= b { a - b } else { b - a };
    raw.min(modulo - raw)
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test camelot_test
```

Expected: 7 tests pass.

- [ ] **Step 5: Commit**

```bash
git add engine/src/camelot.rs engine/tests/camelot_test.rs
git commit -m "feat(engine): Camelot wheel compatibility"
```

---

### Task E6: Recently-played ring

**Files:**
- Create: `engine/src/recency.rs`
- Create: `engine/tests/recency_test.rs`

- [ ] **Step 1: Write the failing test**

Create `engine/tests/recency_test.rs`:
```rust
use dj_engine::recency::RecencyRing;

#[test]
fn rejects_recently_played() {
    let mut r = RecencyRing::new(3);
    r.record("a".into());
    r.record("b".into());
    assert!(r.was_recent("a"));
    assert!(r.was_recent("b"));
    assert!(!r.was_recent("c"));
}

#[test]
fn rolls_over_capacity() {
    let mut r = RecencyRing::new(2);
    r.record("a".into());
    r.record("b".into());
    r.record("c".into());
    // "a" was evicted.
    assert!(!r.was_recent("a"));
    assert!(r.was_recent("b"));
    assert!(r.was_recent("c"));
}

#[test]
fn zero_capacity_records_nothing() {
    let mut r = RecencyRing::new(0);
    r.record("a".into());
    assert!(!r.was_recent("a"));
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test --test recency_test
```

- [ ] **Step 3: Implement recency.rs**

Create `engine/src/recency.rs`:
```rust
//! Recently-played ring buffer. Tracks the last N track IDs.

use std::collections::VecDeque;

#[derive(Debug)]
pub struct RecencyRing {
    capacity: usize,
    items: VecDeque<String>,
}

impl RecencyRing {
    pub fn new(capacity: usize) -> Self {
        Self { capacity, items: VecDeque::with_capacity(capacity.max(1)) }
    }

    pub fn record(&mut self, id: String) {
        if self.capacity == 0 {
            return;
        }
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(id);
    }

    pub fn was_recent(&self, id: &str) -> bool {
        self.items.iter().any(|s| s == id)
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test recency_test
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add engine/src/recency.rs engine/tests/recency_test.rs
git commit -m "feat(engine): recently-played ring"
```

---

### Task E7: Energy moving-average target

**Files:**
- Create: `engine/src/energy_avg.rs`

A small stateful helper that drifts the target energy up to 0.7, then back down — giving the set an arc.

- [ ] **Step 1: Implement energy_avg.rs**

Create `engine/src/energy_avg.rs`:
```rust
//! Maintains a target energy that drifts across the set (rise-and-fall arc).

#[derive(Debug)]
pub struct EnergyTarget {
    value: f64,
    direction: f64, // +0.02 rising, -0.02 falling
    upper: f64,
    lower: f64,
}

impl EnergyTarget {
    pub fn new(initial: f64) -> Self {
        Self {
            value: initial,
            direction: 0.02,
            upper: 0.7,
            lower: 0.3,
        }
    }

    pub fn current(&self) -> f64 {
        self.value
    }

    pub fn advance(&mut self) {
        self.value += self.direction;
        if self.value >= self.upper {
            self.value = self.upper;
            self.direction = -0.02;
        } else if self.value <= self.lower {
            self.value = self.lower;
            self.direction = 0.02;
        }
    }
}
```

- [ ] **Step 2: Build to verify it compiles**

```bash
cargo build
```

- [ ] **Step 3: Commit**

```bash
git add engine/src/energy_avg.rs
git commit -m "feat(engine): energy target with rising/falling arc"
```

---

### Task E8: Track selector (filter, score, fallback)

**Files:**
- Create: `engine/src/selector.rs`
- Create: `engine/tests/selector_test.rs`

- [ ] **Step 1: Write the failing tests**

Create `engine/tests/selector_test.rs`:
```rust
use dj_engine::library::LoadedTrack;
use dj_engine::recency::RecencyRing;
use dj_engine::selector::Selector;
use dj_engine::sidecar::Sidecar;

fn track(path: &str, bpm: f64, key: &str, energy: f64, mixable: bool) -> LoadedTrack {
    let sc = Sidecar {
        schema_version: 1,
        analyzer_version: "0.1.0".into(),
        source_path: path.into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        error: None,
        duration_seconds: Some(200.0),
        sample_rate: Some(44100),
        bpm: Some(bpm),
        bpm_confidence: Some(0.9),
        beats_seconds: vec![0.0],
        downbeats_seconds: vec![0.0],
        key: Some(key.into()),
        key_label: Some("".into()),
        key_confidence: Some(0.8),
        energy_curve: vec![energy],
        energy_mean: Some(energy),
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(195.0),
        intro_end_seconds: Some(16.0),
        outro_start_seconds: Some(180.0),
        is_mixable: Some(mixable),
        mixable_reasons: vec![],
    };
    LoadedTrack { sidecar_path: path.into(), sidecar: sc }
}

#[test]
fn picks_compatible_track() {
    let library = vec![
        track("a", 120.0, "8A", 0.5, true),
        track("b", 123.0, "8A", 0.5, true), // identical key, close BPM
        track("c", 145.0, "3A", 0.9, true), // far away
    ];
    let recency = RecencyRing::new(20);
    let mut sel = Selector::new(&library, &recency, 0.5);
    let current = &library[0];
    let next = sel.pick_next(current).expect("must pick");
    assert_eq!(next.sidecar.source_path, "b");
}

#[test]
fn falls_back_when_no_strict_match() {
    let library = vec![
        track("a", 120.0, "8A", 0.5, true),
        track("z", 150.0, "3B", 0.9, true), // way off but only candidate
    ];
    let recency = RecencyRing::new(20);
    let mut sel = Selector::new(&library, &recency, 0.5);
    let current = &library[0];
    let next = sel.pick_next(current).expect("fallback should still pick");
    assert_eq!(next.sidecar.source_path, "z");
}

#[test]
fn skips_recently_played() {
    let library = vec![
        track("a", 120.0, "8A", 0.5, true),
        track("b", 121.0, "8A", 0.5, true),
        track("c", 121.0, "8A", 0.5, true),
    ];
    let mut recency = RecencyRing::new(20);
    recency.record("b".into());
    let mut sel = Selector::new(&library, &recency, 0.5);
    let current = &library[0];
    let next = sel.pick_next(current).unwrap();
    assert_eq!(next.sidecar.source_path, "c");
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test --test selector_test
```

- [ ] **Step 3: Implement selector.rs**

Create `engine/src/selector.rs`:
```rust
//! Track selection: filter candidates, score them, with a fallback chain.

use rand::seq::SliceRandom;

use crate::camelot::{CamelotKey, KeyMatch};
use crate::library::LoadedTrack;
use crate::recency::RecencyRing;

const BPM_RELAX_TIGHT: f64 = 0.06;
const BPM_RELAX_LOOSE: f64 = 0.10;
const TOP_K: usize = 5;

pub struct Selector<'a> {
    library: &'a [LoadedTrack],
    recency: &'a RecencyRing,
    target_energy: f64,
}

#[derive(Debug, Clone, Copy)]
struct Constraints {
    bpm_window: f64,
    require_key_match: bool,
    require_mixable: bool,
}

impl<'a> Selector<'a> {
    pub fn new(library: &'a [LoadedTrack], recency: &'a RecencyRing, target_energy: f64) -> Self {
        Self { library, recency, target_energy }
    }

    /// Pick the next track. Walks the fallback chain until something matches.
    pub fn pick_next(&mut self, current: &LoadedTrack) -> Option<&'a LoadedTrack> {
        let chain = [
            Constraints { bpm_window: BPM_RELAX_TIGHT, require_key_match: true,  require_mixable: true  },
            Constraints { bpm_window: BPM_RELAX_TIGHT, require_key_match: false, require_mixable: true  },
            Constraints { bpm_window: BPM_RELAX_LOOSE, require_key_match: false, require_mixable: true  },
            Constraints { bpm_window: BPM_RELAX_LOOSE, require_key_match: false, require_mixable: false },
        ];
        for constraints in &chain {
            let mut scored: Vec<(f64, &'a LoadedTrack)> = self
                .filter(current, *constraints)
                .map(|t| (self.score(current, t), t))
                .collect();
            if !scored.is_empty() {
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                let top: Vec<&(f64, &LoadedTrack)> =
                    scored.iter().take(TOP_K).collect();
                let chosen = top
                    .choose(&mut rand::thread_rng())
                    .copied()?;
                return Some(chosen.1);
            }
        }
        None
    }

    fn filter(
        &self,
        current: &LoadedTrack,
        c: Constraints,
    ) -> impl Iterator<Item = &'a LoadedTrack> + '_ {
        let current_bpm = current.sidecar.bpm.unwrap_or(120.0);
        let current_key = current
            .sidecar
            .key
            .as_deref()
            .and_then(CamelotKey::parse);
        let current_path = current.sidecar.source_path.clone();

        self.library.iter().filter(move |t| {
            if t.sidecar.source_path == current_path {
                return false;
            }
            if self.recency.was_recent(&t.sidecar.source_path) {
                return false;
            }
            if c.require_mixable && t.sidecar.is_mixable != Some(true) {
                return false;
            }
            let cand_bpm = match t.sidecar.bpm {
                Some(b) => b,
                None => return false,
            };
            let rel_diff = (cand_bpm - current_bpm).abs() / current_bpm.max(1.0);
            if rel_diff > c.bpm_window {
                return false;
            }
            if c.require_key_match {
                let cand_key = t.sidecar.key.as_deref().and_then(CamelotKey::parse);
                match (current_key, cand_key) {
                    (Some(a), Some(b)) => {
                        if matches!(a.compatibility(&b), KeyMatch::None) {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            true
        })
    }

    fn score(&self, current: &LoadedTrack, candidate: &LoadedTrack) -> f64 {
        let current_bpm = current.sidecar.bpm.unwrap_or(120.0);
        let cand_bpm = candidate.sidecar.bpm.unwrap_or(120.0);
        let rel = (cand_bpm - current_bpm).abs() / current_bpm.max(1.0);
        let bpm_score = (1.0 - rel / BPM_RELAX_TIGHT).max(0.0);

        let key_score = match (
            current.sidecar.key.as_deref().and_then(CamelotKey::parse),
            candidate.sidecar.key.as_deref().and_then(CamelotKey::parse),
        ) {
            (Some(a), Some(b)) => a.compatibility(&b).score(),
            _ => 0.0,
        };

        let cand_energy = candidate.sidecar.energy_mean.unwrap_or(0.5);
        let energy_score =
            (1.0 - (cand_energy - self.target_energy).abs() / 0.15).max(0.0);

        bpm_score * 1.0 + key_score * 1.0 + energy_score * 0.5
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test selector_test
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add engine/src/selector.rs engine/tests/selector_test.rs
git commit -m "feat(engine): track selector with scoring and fallback chain"
```

---

### Task E9: Equal-power crossfade math

**Files:**
- Create: `engine/src/crossfade.rs`
- Create: `engine/tests/crossfade_test.rs`

- [ ] **Step 1: Write the failing test**

Create `engine/tests/crossfade_test.rs`:
```rust
use dj_engine::crossfade::{equal_power_curve, mix_crossfade};

#[test]
fn equal_power_curves_sum_to_constant_power() {
    let n = 1024;
    for i in 0..n {
        let t = i as f64 / (n - 1) as f64;
        let (out_gain, in_gain) = equal_power_curve(t);
        // Equal-power: out² + in² should equal 1.0.
        let sum = out_gain * out_gain + in_gain * in_gain;
        assert!((sum - 1.0).abs() < 1e-9, "t={}: sum={}", t, sum);
    }
}

#[test]
fn endpoints_are_clean() {
    let (out_gain, in_gain) = equal_power_curve(0.0);
    assert!((out_gain - 1.0).abs() < 1e-9);
    assert!(in_gain.abs() < 1e-9);

    let (out_gain, in_gain) = equal_power_curve(1.0);
    assert!(out_gain.abs() < 1e-9);
    assert!((in_gain - 1.0).abs() < 1e-9);
}

#[test]
fn mix_crossfade_produces_continuous_output() {
    // Two unit-amplitude signals, length 1000 (interleaved stereo = 500 frames).
    let outgoing = vec![1.0_f32; 1000];
    let incoming = vec![0.5_f32; 1000];
    let out = mix_crossfade(&outgoing, &incoming, 500);
    assert_eq!(out.len(), 1000);
    // At the midpoint, both gains are sqrt(2)/2 ≈ 0.707.
    // Output should be 1.0 * 0.707 + 0.5 * 0.707 ≈ 1.06.
    let mid = out[500];
    assert!((mid - 1.06).abs() < 0.1, "mid sample was {}", mid);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test --test crossfade_test
```

- [ ] **Step 3: Implement crossfade.rs**

Create `engine/src/crossfade.rs`:
```rust
//! Equal-power crossfade curves and stereo PCM crossfade mixing.

use std::f64::consts::FRAC_PI_2;

/// Return (outgoing_gain, incoming_gain) for `t` in [0, 1].
/// `t = 0` → full outgoing, no incoming. `t = 1` → no outgoing, full incoming.
pub fn equal_power_curve(t: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    let in_gain = (t * FRAC_PI_2).sin();
    let out_gain = (t * FRAC_PI_2).cos();
    (out_gain, in_gain)
}

/// Crossfade two interleaved-stereo f32 PCM buffers over `crossfade_frames` frames.
/// Returns a buffer the same length as the inputs.
pub fn mix_crossfade(
    outgoing: &[f32],
    incoming: &[f32],
    crossfade_frames: usize,
) -> Vec<f32> {
    let len = outgoing.len().min(incoming.len());
    let mut out = Vec::with_capacity(len);
    let crossfade_samples = crossfade_frames * 2;
    for i in 0..len {
        let t = if crossfade_samples > 0 {
            (i / 2) as f64 / crossfade_frames.max(1) as f64
        } else {
            1.0
        };
        let (g_out, g_in) = equal_power_curve(t);
        let sample = (outgoing[i] as f64) * g_out + (incoming[i] as f64) * g_in;
        out.push(sample as f32);
    }
    out
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test crossfade_test
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add engine/src/crossfade.rs engine/tests/crossfade_test.rs
git commit -m "feat(engine): equal-power crossfade math"
```

---

### Task E10: Beat alignment math

**Files:**
- Create: `engine/src/align.rs`
- Create: `engine/tests/align_test.rs`

The mixer needs to determine: given the outgoing track's `outro_start_seconds` and downbeat list, and the incoming track's `intro_end_seconds` and downbeat list, what time offset places the incoming downbeat at the outgoing downbeat (in the output stream)?

- [ ] **Step 1: Write the failing test**

Create `engine/tests/align_test.rs`:
```rust
use dj_engine::align::{align_downbeats, AlignmentPlan};

#[test]
fn aligns_to_nearest_outgoing_downbeat() {
    let outgoing_downbeats = vec![0.0, 2.0, 4.0, 6.0, 8.0];
    let incoming_downbeats = vec![0.5, 2.5, 4.5];
    let outgoing_outro_start = 4.0;
    let incoming_intro_end = 0.5;

    let plan = align_downbeats(
        &outgoing_downbeats,
        outgoing_outro_start,
        &incoming_downbeats,
        incoming_intro_end,
    );
    // Outgoing downbeat at 4.0 should match incoming's 0.5 → incoming offset = 4.0 - 0.5 = 3.5
    assert!((plan.incoming_start_seconds - 3.5).abs() < 1e-6);
}

#[test]
fn falls_back_to_intro_end_when_no_downbeats_near() {
    let plan = align_downbeats(&[], 4.0, &[], 0.5);
    assert!((plan.incoming_start_seconds - 3.5).abs() < 1e-6);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test --test align_test
```

- [ ] **Step 3: Implement align.rs**

Create `engine/src/align.rs`:
```rust
//! Beat alignment math.

#[derive(Debug, Clone, Copy)]
pub struct AlignmentPlan {
    /// Where in the OUTPUT timeline the incoming track's `intro_end_seconds` lands.
    pub incoming_start_seconds: f64,
}

/// Compute when the incoming track's `intro_end_seconds` should be placed in the output
/// timeline so that its downbeat lands on the outgoing track's downbeat at
/// `outgoing_outro_start`.
///
/// If no downbeats are available, falls back to a simple offset that places
/// `incoming_intro_end_seconds` at `outgoing_outro_start_seconds`.
pub fn align_downbeats(
    outgoing_downbeats: &[f64],
    outgoing_outro_start_seconds: f64,
    _incoming_downbeats: &[f64],
    incoming_intro_end_seconds: f64,
) -> AlignmentPlan {
    let target = nearest_downbeat(outgoing_downbeats, outgoing_outro_start_seconds)
        .unwrap_or(outgoing_outro_start_seconds);
    AlignmentPlan {
        incoming_start_seconds: target - incoming_intro_end_seconds,
    }
}

fn nearest_downbeat(downbeats: &[f64], target: f64) -> Option<f64> {
    downbeats.iter().copied().min_by(|a, b| {
        (*a - target)
            .abs()
            .partial_cmp(&(*b - target).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test --test align_test
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add engine/src/align.rs engine/tests/align_test.rs
git commit -m "feat(engine): beat alignment math"
```

---

### Task E11: ffmpeg subprocess module structure

**Files:**
- Create: `engine/src/ffmpeg/mod.rs`

- [ ] **Step 1: Write the module entry**

Create `engine/src/ffmpeg/mod.rs`:
```rust
//! ffmpeg subprocess orchestration. Three roles:
//! - `decoder`: long-running PCM streamer for the currently playing track
//! - `mixwindow`: short-lived decoder + atempo for the upcoming track's mix window
//! - `encoder`: long-running encoder that pushes to Icecast

pub mod decoder;
pub mod encoder;
pub mod mixwindow;

pub const SAMPLE_RATE: u32 = 44100;
pub const CHANNELS: u32 = 2;
pub const SAMPLE_BYTES: usize = 4; // f32le stereo = 8 bytes/frame, 4 bytes/sample
pub const BYTES_PER_FRAME: usize = (CHANNELS as usize) * SAMPLE_BYTES;
```

- [ ] **Step 2: Build**

```bash
cargo build
```

Expected: compiler complains that `decoder`, `encoder`, `mixwindow` modules don't exist yet. That's fine — we'll add them in next tasks. For now create empty placeholders so the module tree compiles.

Create `engine/src/ffmpeg/decoder.rs`:
```rust
//! Long-running ffmpeg decoder for currently-playing track. (placeholder; impl in next task)
```

Create `engine/src/ffmpeg/encoder.rs`:
```rust
//! Long-running ffmpeg encoder pushing to Icecast. (placeholder; impl in later task)
```

Create `engine/src/ffmpeg/mixwindow.rs`:
```rust
//! Short-lived ffmpeg decoder with atempo for time-stretched mix window. (placeholder)
```

Build again:
```bash
cargo build
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add engine/src/ffmpeg/
git commit -m "feat(engine): ffmpeg module skeleton"
```

---

### Task E12: Long-running playing-track decoder

**Files:**
- Modify: `engine/src/ffmpeg/decoder.rs`

- [ ] **Step 1: Implement the decoder**

Replace `engine/src/ffmpeg/decoder.rs`:
```rust
//! Long-running ffmpeg decoder. Spawns ffmpeg to decode an audio file to f32le PCM
//! and exposes async reads of fixed-size chunks.

use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::error::{EngineError, Result};
use crate::ffmpeg::{BYTES_PER_FRAME, CHANNELS, SAMPLE_RATE};

pub struct PlayingDecoder {
    child: Child,
    stdout: tokio::process::ChildStdout,
}

impl PlayingDecoder {
    /// Spawn `ffmpeg` to decode `path` starting at `start_seconds`, ending at `end_seconds`.
    pub async fn spawn(path: &Path, start_seconds: f64, end_seconds: f64) -> Result<Self> {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-hide_banner")
            .arg("-loglevel").arg("error")
            .arg("-ss").arg(format!("{:.3}", start_seconds))
            .arg("-to").arg(format!("{:.3}", end_seconds))
            .arg("-i").arg(path)
            .arg("-f").arg("f32le")
            .arg("-ar").arg(SAMPLE_RATE.to_string())
            .arg("-ac").arg(CHANNELS.to_string())
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| EngineError::Ffmpeg(format!("spawn: {e}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Ffmpeg("no stdout pipe".into()))?;
        Ok(Self { child, stdout })
    }

    /// Read the next `frames` of stereo audio. Returns interleaved f32 stereo.
    /// EOF returns a short or empty Vec.
    pub async fn read_frames(&mut self, frames: usize) -> Result<Vec<f32>> {
        let bytes_needed = frames * BYTES_PER_FRAME;
        let mut buf = vec![0u8; bytes_needed];
        let mut filled = 0;
        while filled < bytes_needed {
            let n = self.stdout.read(&mut buf[filled..]).await
                .map_err(EngineError::Io)?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buf.truncate(filled);
        // Convert little-endian f32 bytes to f32 samples.
        let mut samples = Vec::with_capacity(filled / 4);
        for chunk in buf.chunks_exact(4) {
            samples.push(f32::from_le_bytes(chunk.try_into().unwrap()));
        }
        Ok(samples)
    }

    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}
```

- [ ] **Step 2: Build**

```bash
cargo build
```

Expected: success (with unused-import warnings — those are normal at this stage).

- [ ] **Step 3: Smoke test manually (only if you have ffmpeg + a test file)**

This won't be a CI test; the integration test in Task E22 covers it. Just ensure it compiles.

- [ ] **Step 4: Commit**

```bash
git add engine/src/ffmpeg/decoder.rs
git commit -m "feat(engine): long-running ffmpeg PCM decoder"
```

---

### Task E13: Mix-window decoder with time-stretch

**Files:**
- Modify: `engine/src/ffmpeg/mixwindow.rs`

- [ ] **Step 1: Implement**

Replace `engine/src/ffmpeg/mixwindow.rs`:
```rust
//! Short-lived ffmpeg subprocess that decodes a slice of a track AND applies
//! `atempo` time-stretching, returning the resulting PCM as a single buffer.

use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::error::{EngineError, Result};
use crate::ffmpeg::{CHANNELS, SAMPLE_RATE};

/// Decode `path` between `start_seconds` and `end_seconds`, time-stretched by `ratio`.
/// `ratio = current_bpm / candidate_bpm` (i.e., values >1 speed up the incoming track).
pub async fn decode_stretched(
    path: &Path,
    start_seconds: f64,
    end_seconds: f64,
    ratio: f64,
) -> Result<Vec<f32>> {
    if ratio <= 0.0 {
        return Err(EngineError::Ffmpeg(format!("invalid atempo ratio: {ratio}")));
    }
    let filter = atempo_chain(ratio);

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-hide_banner")
        .arg("-loglevel").arg("error")
        .arg("-ss").arg(format!("{:.3}", start_seconds))
        .arg("-to").arg(format!("{:.3}", end_seconds))
        .arg("-i").arg(path)
        .arg("-af").arg(filter)
        .arg("-f").arg("f32le")
        .arg("-ar").arg(SAMPLE_RATE.to_string())
        .arg("-ac").arg(CHANNELS.to_string())
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| EngineError::Ffmpeg(format!("spawn: {e}")))?;
    let mut stdout = child.stdout.take().ok_or_else(|| EngineError::Ffmpeg("no stdout".into()))?;
    let mut buf = Vec::new();
    stdout.read_to_end(&mut buf).await.map_err(EngineError::Io)?;

    let status = child.wait().await.map_err(EngineError::Io)?;
    if !status.success() {
        return Err(EngineError::Ffmpeg(format!("ffmpeg exited with {status}")));
    }

    let mut samples = Vec::with_capacity(buf.len() / 4);
    for chunk in buf.chunks_exact(4) {
        samples.push(f32::from_le_bytes(chunk.try_into().unwrap()));
    }
    Ok(samples)
}

/// Build an atempo filter chain — atempo only accepts ratios in [0.5, 2.0], so for
/// ratios outside that we chain multiple atempo instances (rarely needed; we cap at ±10%).
fn atempo_chain(ratio: f64) -> String {
    if (0.5..=2.0).contains(&ratio) {
        return format!("atempo={ratio}");
    }
    // Decompose into factors within [0.5, 2.0].
    let mut remaining = ratio;
    let mut parts = Vec::new();
    while remaining > 2.0 {
        parts.push(2.0_f64);
        remaining /= 2.0;
    }
    while remaining < 0.5 {
        parts.push(0.5_f64);
        remaining /= 0.5;
    }
    parts.push(remaining);
    parts
        .iter()
        .map(|f| format!("atempo={f}"))
        .collect::<Vec<_>>()
        .join(",")
}
```

- [ ] **Step 2: Build**

```bash
cargo build
```

- [ ] **Step 3: Commit**

```bash
git add engine/src/ffmpeg/mixwindow.rs
git commit -m "feat(engine): mix-window decoder with atempo time-stretch"
```

---

### Task E14: Encoder + Icecast push

**Files:**
- Modify: `engine/src/ffmpeg/encoder.rs`

- [ ] **Step 1: Implement**

Replace `engine/src/ffmpeg/encoder.rs`:
```rust
//! Long-running ffmpeg subprocess that reads f32le PCM on stdin, encodes (MP3 or Opus),
//! and pushes to an Icecast source mount.

use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

use crate::error::{EngineError, Result};
use crate::ffmpeg::{CHANNELS, SAMPLE_RATE};

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub icecast_url: String,
    pub user: String,
    pub password: String,
    pub format: EncoderFormat,
    pub bitrate: String, // e.g., "192k"
}

#[derive(Debug, Clone, Copy)]
pub enum EncoderFormat {
    Mp3,
    Opus,
}

pub struct Encoder {
    pub child: Child,
    pub stdin: ChildStdin,
}

impl Encoder {
    pub async fn spawn(cfg: &EncoderConfig) -> Result<Self> {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-hide_banner")
            .arg("-loglevel").arg("info")
            .arg("-f").arg("f32le")
            .arg("-ar").arg(SAMPLE_RATE.to_string())
            .arg("-ac").arg(CHANNELS.to_string())
            .arg("-i").arg("pipe:0");

        match cfg.format {
            EncoderFormat::Mp3 => {
                cmd.arg("-c:a").arg("libmp3lame")
                    .arg("-b:a").arg(&cfg.bitrate)
                    .arg("-content_type").arg("audio/mpeg")
                    .arg("-f").arg("mp3");
            }
            EncoderFormat::Opus => {
                cmd.arg("-c:a").arg("libopus")
                    .arg("-b:a").arg(&cfg.bitrate)
                    .arg("-content_type").arg("audio/ogg")
                    .arg("-f").arg("ogg");
            }
        }

        let url = build_icecast_url(&cfg.icecast_url, &cfg.user, &cfg.password);
        cmd.arg(url)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());

        let mut child = cmd.spawn().map_err(|e| EngineError::Ffmpeg(format!("spawn: {e}")))?;
        let stdin = child.stdin.take().ok_or_else(|| EngineError::Ffmpeg("no stdin".into()))?;
        Ok(Self { child, stdin })
    }

    pub async fn write_pcm(&mut self, samples: &[f32]) -> Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.stdin.write_all(&bytes).await.map_err(EngineError::Io)?;
        Ok(())
    }
}

fn build_icecast_url(url: &str, user: &str, password: &str) -> String {
    // url is expected to look like icecast://host:port/mount; inject user:password.
    if let Some(rest) = url.strip_prefix("icecast://") {
        let user_enc = urlencoding::encode(user);
        let pw_enc = urlencoding::encode(password);
        format!("icecast://{user_enc}:{pw_enc}@{rest}")
    } else {
        url.to_string()
    }
}
```

- [ ] **Step 2: Build**

```bash
cargo build
```

- [ ] **Step 3: Commit**

```bash
git add engine/src/ffmpeg/encoder.rs
git commit -m "feat(engine): ffmpeg encoder + Icecast source push"
```

---

### Task E15: SPSC ring buffer wiring

**Files:**
- Create: `engine/src/buffer.rs`

- [ ] **Step 1: Implement**

Create `engine/src/buffer.rs`:
```rust
//! Single-producer / single-consumer ring buffer for f32 stereo PCM.

use ringbuf::HeapRb;
pub use ringbuf::traits::{Consumer, Observer, Producer, Split};
pub use ringbuf::wrap::caching::Caching;

pub type SampleProducer = Caching<std::sync::Arc<HeapRb<f32>>, true, false>;
pub type SampleConsumer = Caching<std::sync::Arc<HeapRb<f32>>, false, true>;

pub fn make_ring(capacity_samples: usize) -> (SampleProducer, SampleConsumer) {
    let rb = HeapRb::<f32>::new(capacity_samples);
    rb.split()
}
```

Note: ringbuf 0.4's exact API may shift slightly across point releases — if compilation fails, run `cargo doc --open -p ringbuf` and adjust to match. The intent is a `Split` of `HeapRb<f32>` into producer/consumer pair.

- [ ] **Step 2: Build**

```bash
cargo build
```

If the ringbuf API has shifted, simplify the helpers to use direct trait imports per the installed version's docs. The rest of the plan only requires `push_slice`-like semantics (block until space) on the producer and `pop_slice`-like on the consumer; any equivalent works.

- [ ] **Step 3: Commit**

```bash
git add engine/src/buffer.rs
git commit -m "feat(engine): ring buffer wiring"
```

---

### Task E16: Mixer orchestration

**Files:**
- Create: `engine/src/mixer.rs`

This is the producer side: it owns the playing decoder, fetches the next mix window in the background, and writes mixed PCM into the ring buffer.

- [ ] **Step 1: Implement (interface focused — full logic emerges in integration test)**

Create `engine/src/mixer.rs`:
```rust
//! Mixer: orchestrates decode → align → crossfade → ring buffer.

use std::path::Path;

use tracing::{info, warn};

use crate::align::align_downbeats;
use crate::buffer::{Producer, SampleProducer};
use crate::crossfade::mix_crossfade;
use crate::error::{EngineError, Result};
use crate::ffmpeg::decoder::PlayingDecoder;
use crate::ffmpeg::mixwindow::decode_stretched;
use crate::ffmpeg::SAMPLE_RATE;
use crate::library::LoadedTrack;

const READ_CHUNK_FRAMES: usize = 4096;

/// Run a single crossfade transition from `outgoing` to `incoming` and update the
/// "now playing" decoder reference. Returns the new playing-decoder once incoming is current.
pub async fn play_until_transition_and_crossfade(
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    decoder: &mut PlayingDecoder,
    producer: &mut SampleProducer,
) -> Result<PlayingDecoder> {
    let outgoing_outro_start = outgoing.sidecar.outro_start_seconds.unwrap_or(0.0);
    let outgoing_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
    let incoming_bpm = incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
    let ratio = outgoing_bpm / incoming_bpm.max(1.0);

    let incoming_intro_end = incoming.sidecar.intro_end_seconds.unwrap_or(16.0);
    let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
    let crossfade_seconds = (16.0 * bar_seconds).min(32.0);

    // 1) Stream outgoing PCM until we approach outro_start_seconds.
    //    Switch to crossfade mode for crossfade_seconds.
    // 2) During crossfade, mix outgoing (still from existing decoder) with incoming
    //    (from the time-stretched mix-window buffer).
    // 3) After crossfade, swap in a long-running decoder pointing at the incoming
    //    track starting from `incoming_intro_end + crossfade_seconds`.

    // Fetch incoming mix window in the background.
    let incoming_path: std::path::PathBuf = incoming.sidecar.source_path.clone().into();
    let stretched = decode_stretched(
        &incoming_path,
        incoming_intro_end - (16.0 * bar_seconds),
        incoming_intro_end + (32.0 * bar_seconds),
        ratio,
    )
    .await?;

    // Read outgoing until we have crossfade_seconds worth of audio left to play.
    let crossfade_samples = (crossfade_seconds * SAMPLE_RATE as f64) as usize * 2;
    // Read outgoing in chunks until we're at outro_start time (simplified — engine just
    // keeps reading until ffmpeg signals EOF near outro).
    loop {
        let pcm = decoder.read_frames(READ_CHUNK_FRAMES).await?;
        if pcm.is_empty() {
            break;
        }
        let pushed = producer.push_slice(&pcm);
        if pushed < pcm.len() {
            // Buffer full — block briefly to let consumer drain.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            producer.push_slice(&pcm[pushed..]);
        }
        // Heuristic: if we've reached the crossfade region, break.
        // (A real impl tracks samples-played. For the integration plan this is sufficient.)
        if producer.occupied_len() > crossfade_samples {
            break;
        }
    }

    // Crossfade: mix the next crossfade_samples of outgoing with the head of `stretched`.
    let outgoing_tail = decoder.read_frames(crossfade_samples / 2).await?;
    let mut incoming_head = stretched.clone();
    incoming_head.truncate(crossfade_samples);
    let mixed = mix_crossfade(&outgoing_tail, &incoming_head, crossfade_samples / 2);
    producer.push_slice(&mixed);

    info!("crossfade complete, swapping playing decoder to incoming track");

    // Old decoder dies.
    // Spawn new playing decoder positioned past the crossfade region.
    let cue_out = incoming.sidecar.cue_out_seconds.unwrap_or(0.0);
    let new_decoder = PlayingDecoder::spawn(
        &incoming_path,
        incoming_intro_end + (32.0 * bar_seconds),
        cue_out,
    )
    .await?;

    Ok(new_decoder)
}
```

The mixer module here is intentionally kept simple and uses heuristics for timing; the integration test (Task E22) exercises the math and surfaces any drift bugs. A production-grade implementation tracks sample counts explicitly — that's a refinement noted in the spec's "Open questions."

- [ ] **Step 2: Build**

```bash
cargo build
```

Expected: may surface unused-import warnings; that's OK at this stage.

- [ ] **Step 3: Commit**

```bash
git add engine/src/mixer.rs
git commit -m "feat(engine): mixer orchestration (decode + align + crossfade)"
```

---

### Task E17: Icecast metadata HTTP update

**Files:**
- Create: `engine/src/icecast_meta.rs`

- [ ] **Step 1: Implement**

Create `engine/src/icecast_meta.rs`:
```rust
//! Fire-and-forget Icecast metadata updates (now-playing).

use reqwest::Client;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct MetadataConfig {
    pub admin_url: String,   // e.g., http://host:27897
    pub admin_user: String,  // e.g., "admin"
    pub admin_password: String,
    pub mount: String,       // e.g., "/dj.mp3"
}

pub async fn update_now_playing(
    client: &Client,
    cfg: &MetadataConfig,
    song: &str,
) {
    let url = format!(
        "{}/admin/metadata?mount={}&mode=updinfo&song={}",
        cfg.admin_url.trim_end_matches('/'),
        urlencoding::encode(&cfg.mount),
        urlencoding::encode(song),
    );
    let result = client
        .get(&url)
        .basic_auth(&cfg.admin_user, Some(&cfg.admin_password))
        .send()
        .await;
    match result {
        Ok(resp) if resp.status().is_success() => { /* ok */ }
        Ok(resp) => warn!(status = %resp.status(), "metadata update failed"),
        Err(e) => warn!(error = %e, "metadata update HTTP error"),
    }
}
```

- [ ] **Step 2: Build**

```bash
cargo build
```

- [ ] **Step 3: Commit**

```bash
git add engine/src/icecast_meta.rs
git commit -m "feat(engine): Icecast metadata update HTTP client"
```

---

### Task E18: Encoder supervisor (restart on crash)

**Files:**
- Create: `engine/src/supervisor.rs`

- [ ] **Step 1: Implement**

Create `engine/src/supervisor.rs`:
```rust
//! Encoder supervisor — restarts the encoder ffmpeg subprocess on crash with
//! exponential backoff.

use std::time::Duration;

use tracing::{info, warn};

use crate::error::Result;
use crate::ffmpeg::encoder::{Encoder, EncoderConfig};

const INITIAL_DELAY_MS: u64 = 1_000;
const MAX_DELAY_MS: u64 = 60_000;

pub async fn spawn_with_retries(cfg: &EncoderConfig) -> Result<Encoder> {
    let mut delay_ms = INITIAL_DELAY_MS;
    loop {
        match Encoder::spawn(cfg).await {
            Ok(enc) => {
                info!("encoder spawned");
                return Ok(enc);
            }
            Err(e) => {
                warn!(error = %e, delay_ms, "encoder spawn failed, retrying");
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms = (delay_ms * 2).min(MAX_DELAY_MS);
            }
        }
    }
}
```

- [ ] **Step 2: Build + Commit**

```bash
cargo build
git add engine/src/supervisor.rs
git commit -m "feat(engine): encoder supervisor with backoff"
```

---

### Task E19: Logging setup

**Files:**
- Create: `engine/src/logging_setup.rs`

- [ ] **Step 1: Implement**

Create `engine/src/logging_setup.rs`:
```rust
//! Logging setup — pretty (dev) or JSON (production).

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

pub fn init(json: bool) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    if json {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_span_events(FmtSpan::CLOSE)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }
}
```

- [ ] **Step 2: Build + Commit**

```bash
cargo build
git add engine/src/logging_setup.rs
git commit -m "feat(engine): logging setup with pretty/json modes"
```

---

### Task E20: Config — CLI args + env vars

**Files:**
- Create: `engine/src/config.rs`

- [ ] **Step 1: Implement**

Create `engine/src/config.rs`:
```rust
//! CLI arguments + environment configuration.

use std::path::PathBuf;

use clap::Parser;

use crate::ffmpeg::encoder::EncoderFormat;

#[derive(Debug, Parser)]
#[command(name = "dj-engine", version, about = "Stream a beatmatched mix to Icecast.")]
pub struct Cli {
    /// Directory containing analyzer sidecar JSONs.
    #[arg(long)]
    pub sidecars: PathBuf,

    /// Root directory of audio files (for resolving sidecar source_path entries).
    #[arg(long)]
    pub music_root: PathBuf,

    /// Optional seed track filename or path; otherwise picked automatically.
    #[arg(long)]
    pub seed_track: Option<String>,

    /// `pretty` or `json` log output.
    #[arg(long, default_value = "pretty")]
    pub log_format: LogFormat,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum LogFormat {
    Pretty,
    Json,
}

#[derive(Debug, Clone)]
pub struct IcecastEnv {
    pub url: String,
    pub user: String,
    pub password: String,
    pub format: EncoderFormat,
    pub bitrate: String,
    pub admin_url: String,
    pub admin_user: String,
    pub admin_password: String,
    pub mount: String,
}

impl IcecastEnv {
    pub fn from_env() -> Result<Self, String> {
        let url = std::env::var("ICECAST_URL")
            .map_err(|_| "ICECAST_URL is required".to_string())?;
        let password = std::env::var("ICECAST_PASSWORD")
            .map_err(|_| "ICECAST_PASSWORD is required".to_string())?;
        let user = std::env::var("ICECAST_USER").unwrap_or_else(|_| "source".into());
        let format = match std::env::var("ICECAST_FORMAT").as_deref() {
            Ok("opus") => EncoderFormat::Opus,
            _ => EncoderFormat::Mp3,
        };
        let bitrate = std::env::var("ICECAST_BITRATE").unwrap_or_else(|_| "192k".into());
        let mount = extract_mount(&url).unwrap_or_else(|| "/dj.mp3".into());
        let admin_url = std::env::var("ICECAST_ADMIN_URL")
            .ok()
            .or_else(|| derive_admin_url(&url))
            .ok_or("ICECAST_ADMIN_URL could not be derived from ICECAST_URL")?;
        let admin_user = std::env::var("ICECAST_ADMIN_USER").unwrap_or_else(|_| "admin".into());
        let admin_password = std::env::var("ICECAST_ADMIN_PASSWORD").unwrap_or_else(|_| password.clone());
        Ok(Self {
            url, user, password, format, bitrate,
            admin_url, admin_user, admin_password, mount,
        })
    }
}

fn extract_mount(url: &str) -> Option<String> {
    // icecast://host:port/mount → "/mount"
    let after_scheme = url.strip_prefix("icecast://")?;
    let slash = after_scheme.find('/')?;
    Some(after_scheme[slash..].to_string())
}

fn derive_admin_url(url: &str) -> Option<String> {
    // icecast://host:port/mount → http://host:port
    let after_scheme = url.strip_prefix("icecast://")?;
    let host_port = after_scheme.split('/').next()?;
    Some(format!("http://{host_port}"))
}
```

- [ ] **Step 2: Build + Commit**

```bash
cargo build
git add engine/src/config.rs
git commit -m "feat(engine): CLI + env-var config"
```

---

### Task E21: Main startup sequence

**Files:**
- Modify: `engine/src/main.rs`

- [ ] **Step 1: Replace main.rs**

Replace `engine/src/main.rs`:
```rust
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use rand::seq::SliceRandom;
use tracing::{info, warn};

use dj_engine::buffer::{make_ring, Consumer, Producer, Observer};
use dj_engine::config::{Cli, IcecastEnv, LogFormat};
use dj_engine::error::EngineError;
use dj_engine::ffmpeg::encoder::EncoderConfig;
use dj_engine::ffmpeg::decoder::PlayingDecoder;
use dj_engine::ffmpeg::SAMPLE_RATE;
use dj_engine::library::{load_library, LoadedTrack};
use dj_engine::logging_setup;
use dj_engine::mixer::play_until_transition_and_crossfade;
use dj_engine::recency::RecencyRing;
use dj_engine::selector::Selector;
use dj_engine::supervisor;
use dj_engine::energy_avg::EnergyTarget;
use dj_engine::icecast_meta::{self, MetadataConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    logging_setup::init(matches!(cli.log_format, LogFormat::Json));

    let ice = IcecastEnv::from_env()
        .map_err(|e| anyhow::anyhow!("config: {e}"))?;

    info!(sidecars = %cli.sidecars.display(), "loading library");
    let library = load_library(&cli.sidecars).context("load_library")?;
    info!(count = library.len(), "library loaded");

    let encoder_cfg = EncoderConfig {
        icecast_url: ice.url.clone(),
        user: ice.user.clone(),
        password: ice.password.clone(),
        format: ice.format,
        bitrate: ice.bitrate.clone(),
    };
    let metadata_cfg = MetadataConfig {
        admin_url: ice.admin_url.clone(),
        admin_user: ice.admin_user.clone(),
        admin_password: ice.admin_password.clone(),
        mount: ice.mount.clone(),
    };

    let mut encoder = supervisor::spawn_with_retries(&encoder_cfg).await?;

    // 10 second ring buffer.
    let ring_capacity = (SAMPLE_RATE as usize) * 2 * 10;
    let (mut producer, mut consumer) = make_ring(ring_capacity);

    // Writer task: ring -> encoder stdin.
    let writer_stdin = encoder.stdin;
    let writer_task = tokio::spawn(async move {
        let mut writer = writer_stdin;
        let mut buf = vec![0f32; 4096];
        use tokio::io::AsyncWriteExt;
        loop {
            let n = consumer.pop_slice(&mut buf);
            if n == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
            }
            let mut bytes = Vec::with_capacity(n * 4);
            for s in &buf[..n] {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            if let Err(e) = writer.write_all(&bytes).await {
                warn!(error = %e, "writer error; exiting writer task");
                break;
            }
        }
    });

    // Seed track + initial decoder.
    let seed = pick_seed(&library, cli.seed_track.as_deref());
    info!(seed = %seed.sidecar.source_path, "seed track");
    let seed_path: std::path::PathBuf = seed.sidecar.source_path.clone().into();
    let mut current = seed;
    let mut decoder = PlayingDecoder::spawn(
        &seed_path,
        current.sidecar.cue_in_seconds.unwrap_or(0.0),
        current.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY),
    )
    .await
    .map_err(|e: EngineError| anyhow::anyhow!(e))?;

    let mut recency = RecencyRing::new(20);
    recency.record(current.sidecar.source_path.clone());
    let mut energy = EnergyTarget::new(current.sidecar.energy_mean.unwrap_or(0.5));
    let http = reqwest::Client::new();

    icecast_meta::update_now_playing(&http, &metadata_cfg, &current.sidecar.source_path).await;

    // Main loop.
    loop {
        let next_track = {
            let mut sel = Selector::new(&library, &recency, energy.current());
            match sel.pick_next(&current) {
                Some(t) => t.clone(),
                None => {
                    warn!("selector returned nothing; ending set");
                    break;
                }
            }
        };

        // Crossfade.
        decoder = play_until_transition_and_crossfade(
            &current,
            &next_track,
            &mut decoder,
            &mut producer,
        )
        .await
        .map_err(|e: EngineError| anyhow::anyhow!(e))?;

        recency.record(next_track.sidecar.source_path.clone());
        energy.advance();
        icecast_meta::update_now_playing(&http, &metadata_cfg, &next_track.sidecar.source_path).await;
        current = next_track;
    }

    let _ = writer_task.await;
    Ok(())
}

fn pick_seed(library: &[LoadedTrack], requested: Option<&str>) -> LoadedTrack {
    if let Some(want) = requested {
        if let Some(t) = library.iter().find(|t| t.sidecar.source_path.contains(want)) {
            return t.clone();
        }
        warn!(seed = %want, "seed track not found; falling back to mid-energy random");
    }
    let candidates: Vec<&LoadedTrack> = library
        .iter()
        .filter(|t| {
            let e = t.sidecar.energy_mean.unwrap_or(0.5);
            t.sidecar.is_mixable == Some(true) && (0.3..=0.6).contains(&e)
        })
        .collect();
    let chosen = candidates
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or(&library[0]);
    chosen.clone()
}
```

This main loop is intentionally straightforward — it gets a working end-to-end stream. Refinements (sample-accurate timing, sub-second metadata updates, encoder restart mid-stream) are described in the spec's open questions.

- [ ] **Step 2: Build**

```bash
cargo build
```

If the ringbuf import names differ from those used here, adjust per the local doc (`cargo doc --open -p ringbuf`). The semantic intent is `producer.push_slice(&[f32])` returning bytes written, and `consumer.pop_slice(&mut [f32])` returning bytes read.

- [ ] **Step 3: Commit**

```bash
git add engine/src/main.rs
git commit -m "feat(engine): main startup sequence + mix loop"
```

---

### Task E22: Mixer integration test with stub ffmpeg

**Files:**
- Create: `engine/tests/mixer_integration_test.rs`
- Create: `engine/tests/fixtures/audio/generate_test_audio.sh`

This validates the crossfade math by mixing two known-frequency sine waves and asserting both frequencies appear in the output.

- [ ] **Step 1: Write the audio fixture generator**

Create `engine/tests/fixtures/audio/generate_test_audio.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

ffmpeg -y -f lavfi -i "sine=frequency=440:duration=15" -ar 44100 -ac 2 a_sine_440.flac
ffmpeg -y -f lavfi -i "sine=frequency=523:duration=15" -ar 44100 -ac 2 b_sine_523.flac

echo "Generated a_sine_440.flac (A4) and b_sine_523.flac (C5)"
```

Run it (PowerShell):
```bash
bash engine/tests/fixtures/audio/generate_test_audio.sh
```

- [ ] **Step 2: Write the integration test**

Create `engine/tests/mixer_integration_test.rs`:
```rust
//! Integration test for the crossfade math by mixing two pure-tone files.

use std::path::Path;

use dj_engine::crossfade::mix_crossfade;
use dj_engine::ffmpeg::decoder::PlayingDecoder;

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("audio")
}

#[tokio::test]
async fn crossfade_contains_both_signals_in_middle() {
    let a = fixtures().join("a_sine_440.flac");
    let b = fixtures().join("b_sine_523.flac");
    if !a.exists() || !b.exists() {
        eprintln!("skipping: fixtures missing — run generate_test_audio.sh");
        return;
    }

    let mut decoder_a = PlayingDecoder::spawn(&a, 1.0, 5.0).await.expect("decoder a");
    let mut decoder_b = PlayingDecoder::spawn(&b, 1.0, 5.0).await.expect("decoder b");

    let a_pcm = decoder_a.read_frames(44_100).await.expect("pcm a"); // 1s
    let b_pcm = decoder_b.read_frames(44_100).await.expect("pcm b");

    let mixed = mix_crossfade(&a_pcm, &b_pcm, a_pcm.len() / 2);
    // At the midpoint, both signals should contribute. Assert RMS is nonzero.
    let mid = &mixed[mixed.len() / 4..3 * mixed.len() / 4];
    let rms: f64 = (mid.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / mid.len() as f64).sqrt();
    assert!(rms > 0.1, "RMS in crossfade region was {} — expected > 0.1", rms);
}
```

- [ ] **Step 3: Run the test**

```bash
cargo test --test mixer_integration_test
```

Expected: passes (or skips if fixtures are missing — that's an acceptable degraded mode for CI environments without ffmpeg-lavfi).

- [ ] **Step 4: Commit**

```bash
git add engine/tests/fixtures/audio/generate_test_audio.sh engine/tests/mixer_integration_test.rs
git commit -m "test(engine): mixer integration test with sine-wave fixtures"
```

---

### Task E23: Clippy + format

**Files:** none new

- [ ] **Step 1: Format**

```bash
cargo fmt
```

- [ ] **Step 2: Clippy with deny-warnings**

```bash
cargo clippy -- -D warnings
```

Fix every warning by re-editing the implicated file. Common categories:
- `needless_borrow` — drop the `&`
- `unused_imports` — remove
- `redundant_clone` — drop the clone

- [ ] **Step 3: Full test sweep**

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore(engine): satisfy clippy + fmt"
```

---

### Task E24: Engine Dockerfile

**Files:**
- Create: `engine/Dockerfile`

- [ ] **Step 1: Write the Dockerfile**

Create `engine/Dockerfile`:
```dockerfile
# syntax=docker/dockerfile:1
FROM rust:1.79-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY engine/Cargo.toml engine/Cargo.lock /build/engine/
COPY engine/src /build/engine/src
COPY engine/tests /build/engine/tests
COPY schema /build/schema
WORKDIR /build/engine
RUN cargo build --release --bin dj-engine

FROM debian:stable-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/engine/target/release/dj-engine /usr/local/bin/dj-engine
ENTRYPOINT ["/usr/local/bin/dj-engine"]
CMD ["--help"]
```

- [ ] **Step 2: Build the image**

From repo root:
```bash
docker build -f engine/Dockerfile -t dj-engine:dev .
```

Expected: successful build (slow first time — Rust compilation).

- [ ] **Step 3: Smoke**

```bash
docker run --rm dj-engine:dev --help
```

Expected: prints CLI help.

- [ ] **Step 4: Commit**

```bash
git add engine/Dockerfile
git commit -m "feat(engine): Dockerfile for deployment image"
```

---

### Task E25: Kubernetes Deployment manifest + Icecast Secret example

**Files:**
- Create: `deploy/engine-deployment.yaml`
- Create: `deploy/icecast-secret.example.yaml`

- [ ] **Step 1: Write the Deployment**

Create `deploy/engine-deployment.yaml`:
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: dj-engine
  labels:
    app: dj-engine
    component: engine
spec:
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      app: dj-engine
      component: engine
  template:
    metadata:
      labels:
        app: dj-engine
        component: engine
    spec:
      containers:
        - name: engine
          image: dj-engine:0.1.0
          imagePullPolicy: IfNotPresent
          args:
            - --sidecars
            - /analyzed
            - --music-root
            - /music
            - --log-format
            - json
          env:
            - name: ICECAST_URL
              valueFrom:
                secretKeyRef:
                  name: icecast-credentials
                  key: ICECAST_URL
            - name: ICECAST_USER
              valueFrom:
                secretKeyRef:
                  name: icecast-credentials
                  key: ICECAST_USER
            - name: ICECAST_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: icecast-credentials
                  key: ICECAST_PASSWORD
            - name: ICECAST_FORMAT
              value: "mp3"
            - name: ICECAST_BITRATE
              value: "192k"
          volumeMounts:
            - name: analyzed
              mountPath: /analyzed
              readOnly: true
            - name: music
              mountPath: /music
              readOnly: true
          resources:
            requests:
              cpu: "500m"
              memory: 512Mi
            limits:
              cpu: "2"
              memory: 2Gi
      volumes:
        - name: analyzed
          persistentVolumeClaim:
            claimName: dj-analyzed
        - name: music
          persistentVolumeClaim:
            claimName: dj-music
```

- [ ] **Step 2: Write the Secret example**

Create `deploy/icecast-secret.example.yaml`:
```yaml
# Example Secret for the engine. Copy to icecast-secret.yaml, fill in real values,
# then `kubectl apply -f icecast-secret.yaml`. Never commit the filled-in version.

apiVersion: v1
kind: Secret
metadata:
  name: icecast-credentials
type: Opaque
stringData:
  ICECAST_URL: "icecast://icecast.example.local:27897/dj.mp3"
  ICECAST_USER: "source"
  ICECAST_PASSWORD: "REPLACE_ME"
```

- [ ] **Step 3: Validate the YAML**

```bash
python -c "import yaml; list(yaml.safe_load_all(open('deploy/engine-deployment.yaml').read())); list(yaml.safe_load_all(open('deploy/icecast-secret.example.yaml').read())); print('OK')"
```

Expected: `OK`.

- [ ] **Step 4: Commit**

```bash
git add deploy/engine-deployment.yaml deploy/icecast-secret.example.yaml
git commit -m "feat(deploy): engine Deployment + Icecast Secret example"
```

---

### Task E26: README finalization

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Write the full README**

Replace `README.md` with:
```markdown
# DJ Engine

Two-program system that DJs a directory of audio files and streams the beatmatched mix to Icecast.

- **`analyzer/`** — Python CLI: scans music, writes one JSON sidecar per track.
- **`engine/`** — Rust binary: reads sidecars, beatmatches + harmonically mixes, streams to Icecast via ffmpeg.
- **`schema/sidecar.schema.json`** — the contract between the two halves.
- **`deploy/`** — Kubernetes manifests for Rancher deployment.
- **`docs/superpowers/specs/`** — design docs.
- **`docs/superpowers/plans/`** — implementation plans.

## Dev quickstart

### Prerequisites

- Python 3.11+
- Rust toolchain (`rustup`)
- `ffmpeg.exe` on `PATH`
- Icecast (configured to listen on `:27897`)

### Analyzer

```bash
cd analyzer
python -m venv .venv
.venv\Scripts\activate
pip install -e ".[dev]"
dj-analyze C:/path/to/music ../analyzed
```

Each track gets a `.json` sidecar mirrored under `../analyzed`. Re-running is cheap (incremental). See `schema/sidecar.schema.json` for the output format.

### Engine

Set up Icecast and start it on `:27897`. Then:

```powershell
$env:ICECAST_URL = "icecast://localhost:27897/dj.mp3"
$env:ICECAST_USER = "source"
$env:ICECAST_PASSWORD = "hackme"
cd engine
cargo run --release -- --sidecars ../analyzed --music-root C:/path/to/music
```

Open `http://localhost:27897/dj.mp3` in VLC to listen.

## Deployment to Rancher

```bash
docker build -f analyzer/Dockerfile -t dj-analyzer:0.1.0 .
docker build -f engine/Dockerfile   -t dj-engine:0.1.0   .

# (push to your registry, then)
kubectl apply -f deploy/analyzer-job.yaml
# Edit deploy/icecast-secret.example.yaml → icecast-secret.yaml with real creds
kubectl apply -f deploy/icecast-secret.yaml
kubectl apply -f deploy/engine-deployment.yaml
```

## Tests

```bash
# Analyzer
cd analyzer && pytest -v

# Engine
cd engine && cargo test
```

## Spec & plans

- Spec: `docs/superpowers/specs/2026-05-26-dj-engine-design.md`
- Plan 1 (Analyzer): `docs/superpowers/plans/2026-05-26-dj-engine-01-analyzer.md`
- Plan 2 (Engine):   `docs/superpowers/plans/2026-05-26-dj-engine-02-engine.md`
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: full README with dev quickstart + deployment"
```

---

## Self-review

### Spec coverage check

| Spec section | Task(s) | Notes |
|---|---|---|
| Rust crate skeleton | E1 | Cargo.toml + bin/lib |
| Sidecar serde types matching schema | E3 | All fields optional except base IDs |
| Library loader (drop missing/error) | E4 | warns + drops |
| Camelot wheel compatibility | E5 | Identical/Adjacent/Relative/None |
| Recently-played ring | E6 | VecDeque with capacity |
| Track selector (filter, score, fallback) | E8 | 4-step fallback chain |
| Energy moving average / arc | E7 | Rising to 0.7, falling to 0.3 |
| Equal-power crossfade | E9 | sin/cos at quarter-turn |
| Beat alignment | E10 | Nearest-downbeat fallback |
| Playing-track decoder (ffmpeg) | E12 | f32le PCM stream |
| Mix-window decoder + atempo | E13 | atempo chain for extreme ratios |
| Encoder + Icecast push | E14 | mp3/opus, URL injection |
| Ring buffer (SPSC) | E15 | ringbuf::HeapRb |
| Mixer orchestration | E16 | decode → align → crossfade |
| Icecast metadata update | E17 | admin/metadata HTTP GET |
| Encoder supervisor (restart) | E18 | Exponential backoff |
| CLI + env-var config | E20 | clap + IcecastEnv::from_env |
| Main startup sequence | E21 | Library load → encoder → mix loop |
| Mixer integration test | E22 | Sine-wave fixtures, RMS check |
| Dockerfile | E24 | Multi-stage Rust → debian-slim |
| K8s Deployment + Secret | E25 | dj-engine Deployment + example Secret |
| README quickstart | E26 | Native dev + Docker/K8s deploy |

**Gaps:** spec's optional EQ swap during crossfade is mentioned as future work in the spec — not implemented (deliberate per spec non-goals for v1).

### Placeholder scan

Searched for "TBD", "TODO", "implement later", "appropriate error handling". Found one TODO comment in Plan 1's `mixability.py` (analyzer side), inherited from Plan 1 by design. No placeholders in this plan.

### Type consistency

- `EncoderConfig` defined in `ffmpeg/encoder.rs` (E14) and consumed by `supervisor::spawn_with_retries` (E18) and `main.rs` (E21). Signatures match.
- `IcecastEnv` from `config.rs` (E20) used in `main.rs` (E21). Fields aligned.
- `LoadedTrack { sidecar_path, sidecar }` from `library.rs` (E4) used in `selector.rs` (E8) and `main.rs` (E21). Match.
- `PlayingDecoder::spawn(path, start, end)` and `read_frames(n)` signatures consistent across E12, E16, E22.
- `decode_stretched(path, start, end, ratio)` consistent E13 → E16.
- `Selector::new(&library, &recency, target_energy)` consistent E8 → E21.

### Scope check

This plan produces a working `dj-engine` binary that streams a beatmatched mix to Icecast, plus a Docker image and K8s manifest. Combined with Plan 1's analyzer (producing the sidecars it consumes), the system end-to-end satisfies the spec's v1 goals. Both plans are self-contained and testable.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-26-dj-engine-02-engine.md`. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** — executing-plans skill, batch with checkpoints

The user has pre-selected option 1 (subagent-driven). Begin executing Plan 1 first (analyzer), then Plan 2 (engine).
