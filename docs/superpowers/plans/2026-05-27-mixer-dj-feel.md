# Mixer DJ-Feel Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the engine's mixer from a volume-only crossfade into a DJ-feel mixer with EQ swap, 16-bar phrase alignment, and adaptive crossfade length (32-bar long blend / 16-bar standard / 8-bar quick cut / hard cut).

**Architecture:** Three new pure modules (`eq.rs`, `phrase.rs`, `transition.rs`) plus a new `mix_crossfade_eq()` in `crossfade.rs`. `mixer.rs` becomes a thin executor of a `TransitionPlan` produced by `transition::plan_transition()`. No analyzer changes — phrase boundaries are derived from existing sidecar fields (`downbeats_seconds`, `intro_end_seconds`, `outro_start_seconds`).

**Tech Stack:** Rust (existing engine crate), tokio (existing), no new external dependencies. Biquad filters implemented from scratch using the Robert Bristow-Johnson cookbook formulas.

**Spec:** `docs/superpowers/specs/2026-05-27-mixer-dj-feel-design.md`

**Prerequisite:** Engine builds and tests pass on `main` (24 tests).

---

## File structure (this plan creates / modifies these files)

```
DJ-Engine/
  engine/
    src/
      eq.rs                    # NEW — biquad filters + swept filter helper
      phrase.rs                # NEW — derive phrase boundaries from a sidecar
      transition.rs            # NEW — TransitionPlan + plan_transition()
      crossfade.rs             # MODIFY — add mix_crossfade_eq()
      mixer.rs                 # MODIFY — consume TransitionPlan, support all variants
      lib.rs                   # MODIFY — declare new modules
    tests/
      eq_test.rs                       # NEW
      phrase_test.rs                   # NEW
      transition_test.rs               # NEW
      crossfade_eq_test.rs             # NEW
      mixer_integration_test.rs        # MODIFY — add hard-cut + phrase-snap cases
```

Each new module has one clear responsibility:
- `eq.rs` — pure DSP, no knowledge of tracks or transitions
- `phrase.rs` — pure data, derives timestamps from sidecar metadata
- `transition.rs` — policy: pick length/variant/snapped times for a pair of tracks
- `crossfade.rs` — pure mixing math; existing `mix_crossfade` + new `mix_crossfade_eq`
- `mixer.rs` — orchestration only; consumes a plan, drives ffmpeg + ring buffer

---

### Task 1: Scaffold — declare new modules

**Files:**
- Modify: `engine/src/lib.rs`
- Create: `engine/src/eq.rs`
- Create: `engine/src/phrase.rs`
- Create: `engine/src/transition.rs`

- [ ] **Step 1: Add module declarations to `engine/src/lib.rs`**

Open `engine/src/lib.rs`. After the existing `pub mod crossfade;` line add three new lines so the full module list becomes:

```rust
//! DJ Engine — Rust orchestration around ffmpeg subprocesses.

pub mod align;
pub mod buffer;
pub mod camelot;
pub mod config;
pub mod crossfade;
pub mod energy_avg;
pub mod eq;
pub mod error;
pub mod ffmpeg;
pub mod icecast_meta;
pub mod library;
pub mod logging_setup;
pub mod mixer;
pub mod phrase;
pub mod recency;
pub mod selector;
pub mod sidecar;
pub mod supervisor;
pub mod transition;
```

(Keep them alphabetically sorted to match the existing style.)

- [ ] **Step 2: Create empty placeholder files**

Create `engine/src/eq.rs`:
```rust
//! Biquad filters + swept filter helper. Pure DSP, no knowledge of tracks.
```

Create `engine/src/phrase.rs`:
```rust
//! Derive 16-bar phrase boundaries from a sidecar. Pure data.
```

Create `engine/src/transition.rs`:
```rust
//! Pick crossfade length, variant, and snapped start times for a pair of tracks.
```

- [ ] **Step 3: Build to verify the crate still compiles**

Run:
```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" build --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

Expected: `Finished ...` (with three unused-module warnings — fine, they'll be filled in next tasks).

- [ ] **Step 4: Commit**

```
git add engine/src/lib.rs engine/src/eq.rs engine/src/phrase.rs engine/src/transition.rs
git commit -m "feat(engine): scaffold eq + phrase + transition modules"
```

---

### Task 2: `eq.rs` — Biquad lowpass

**Files:**
- Modify: `engine/src/eq.rs`
- Create: `engine/tests/eq_test.rs`

- [ ] **Step 1: Write the failing tests**

Create `engine/tests/eq_test.rs`:
```rust
use dj_engine::eq::Biquad;

fn rms(samples: &[f32]) -> f32 {
    let sum: f64 = samples.iter().map(|s| (*s as f64).powi(2)).sum();
    ((sum / samples.len() as f64).sqrt()) as f32
}

fn sine(freq_hz: f64, sample_rate: f64, duration_s: f64) -> Vec<f32> {
    let n = (duration_s * sample_rate) as usize;
    (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * freq_hz * (i as f64) / sample_rate).sin() as f32)
        .collect()
}

#[test]
fn lowpass_passes_low_frequencies() {
    let sr = 44100.0;
    let signal = sine(100.0, sr, 1.0);
    let mut filter = Biquad::lowpass(sr, 500.0);
    let filtered: Vec<f32> = signal.iter().map(|s| filter.process(*s)).collect();

    // Allow a brief warm-up where the filter state is converging. Skip the first 200 samples.
    let in_rms = rms(&signal[200..]);
    let out_rms = rms(&filtered[200..]);
    let ratio_db = 20.0 * (out_rms / in_rms).log10();
    assert!(
        ratio_db.abs() < 1.0,
        "100 Hz should pass through a 500 Hz lowpass with <1 dB loss; got {} dB",
        ratio_db
    );
}

#[test]
fn lowpass_kills_high_frequencies() {
    let sr = 44100.0;
    let signal = sine(10_000.0, sr, 1.0);
    let mut filter = Biquad::lowpass(sr, 200.0);
    let filtered: Vec<f32> = signal.iter().map(|s| filter.process(*s)).collect();

    let in_rms = rms(&signal[200..]);
    let out_rms = rms(&filtered[200..]);
    assert!(
        out_rms < 0.05 * in_rms,
        "10 kHz signal should be <5% of input after 200 Hz lowpass; got out/in = {}",
        out_rms / in_rms
    );
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test eq_test
```

Expected: compile error — `Biquad` not found.

- [ ] **Step 3: Implement `Biquad::lowpass` and `Biquad::process`**

Replace `engine/src/eq.rs` with:
```rust
//! Biquad filters + swept filter helper. Pure DSP, no knowledge of tracks.

/// Direct Form I biquad. Coefficients use the Robert Bristow-Johnson cookbook.
/// Q is fixed at 1/sqrt(2) (Butterworth — flat passband, no resonance).
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    // Normalized coefficients (a0 already divided out).
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    // State.
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

const Q_BUTTERWORTH: f64 = 0.707_106_781_186_547_5; // 1 / sqrt(2)

impl Biquad {
    pub fn lowpass(sample_rate: f64, cutoff_hz: f64) -> Self {
        let (b0, b1, b2, a1, a2) = lowpass_coeffs(sample_rate, cutoff_hz);
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Apply the filter to one sample. Stateful — call in order.
    pub fn process(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y as f32
    }
}

fn lowpass_coeffs(sample_rate: f64, cutoff_hz: f64) -> (f64, f64, f64, f64, f64) {
    let cutoff = cutoff_hz.clamp(1.0, sample_rate * 0.49);
    let w0 = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * Q_BUTTERWORTH);
    let a0 = 1.0 + alpha;

    let b0 = ((1.0 - cos_w0) / 2.0) / a0;
    let b1 = (1.0 - cos_w0) / a0;
    let b2 = ((1.0 - cos_w0) / 2.0) / a0;
    let a1 = (-2.0 * cos_w0) / a0;
    let a2 = (1.0 - alpha) / a0;
    (b0, b1, b2, a1, a2)
}
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test eq_test
```

Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/eq.rs engine/tests/eq_test.rs
git commit -m "feat(engine): Biquad lowpass filter"
```

---

### Task 3: `eq.rs` — Biquad highpass

**Files:**
- Modify: `engine/src/eq.rs`
- Modify: `engine/tests/eq_test.rs`

- [ ] **Step 1: Add failing tests**

Append to `engine/tests/eq_test.rs`:
```rust
#[test]
fn highpass_passes_high_frequencies() {
    let sr = 44100.0;
    let signal = sine(10_000.0, sr, 1.0);
    let mut filter = Biquad::highpass(sr, 1000.0);
    let filtered: Vec<f32> = signal.iter().map(|s| filter.process(*s)).collect();
    let in_rms = rms(&signal[200..]);
    let out_rms = rms(&filtered[200..]);
    let ratio_db = 20.0 * (out_rms / in_rms).log10();
    assert!(
        ratio_db.abs() < 1.0,
        "10 kHz should pass through a 1 kHz highpass; got {} dB",
        ratio_db
    );
}

#[test]
fn highpass_kills_low_frequencies() {
    let sr = 44100.0;
    let signal = sine(50.0, sr, 1.0);
    let mut filter = Biquad::highpass(sr, 1000.0);
    let filtered: Vec<f32> = signal.iter().map(|s| filter.process(*s)).collect();
    let in_rms = rms(&signal[200..]);
    let out_rms = rms(&filtered[200..]);
    assert!(
        out_rms < 0.05 * in_rms,
        "50 Hz signal should be <5% after 1 kHz highpass; got out/in = {}",
        out_rms / in_rms
    );
}
```

- [ ] **Step 2: Run tests, confirm failure**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test eq_test
```

Expected: 2 new test failures — `Biquad::highpass` not found.

- [ ] **Step 3: Implement `Biquad::highpass`**

Append to `engine/src/eq.rs` (inside `impl Biquad`):
```rust
impl Biquad {
    pub fn highpass(sample_rate: f64, cutoff_hz: f64) -> Self {
        let (b0, b1, b2, a1, a2) = highpass_coeffs(sample_rate, cutoff_hz);
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}
```

(Add this as a SECOND `impl Biquad { ... }` block so we don't have to surgically edit the inside of the first. Rust allows multiple impl blocks.)

Then add the highpass coefficients function at the bottom of the file:
```rust
fn highpass_coeffs(sample_rate: f64, cutoff_hz: f64) -> (f64, f64, f64, f64, f64) {
    let cutoff = cutoff_hz.clamp(1.0, sample_rate * 0.49);
    let w0 = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * Q_BUTTERWORTH);
    let a0 = 1.0 + alpha;

    let b0 = ((1.0 + cos_w0) / 2.0) / a0;
    let b1 = (-(1.0 + cos_w0)) / a0;
    let b2 = ((1.0 + cos_w0) / 2.0) / a0;
    let a1 = (-2.0 * cos_w0) / a0;
    let a2 = (1.0 - alpha) / a0;
    (b0, b1, b2, a1, a2)
}
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test eq_test
```

Expected: 4 tests pass total.

- [ ] **Step 5: Commit**

```
git add engine/src/eq.rs engine/tests/eq_test.rs
git commit -m "feat(engine): Biquad highpass filter"
```

---

### Task 4: `eq.rs` — `apply_swept_filter` with cutoff sweep + stereo + no-NaN tests

**Files:**
- Modify: `engine/src/eq.rs`
- Modify: `engine/tests/eq_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `engine/tests/eq_test.rs`:
```rust
use dj_engine::eq::{apply_swept_filter, FilterKind};

#[test]
fn apply_swept_filter_kills_bass_at_end_of_sweep() {
    let sr = 44100.0;
    // Half a second of 80 Hz sine in stereo (interleaved).
    let mono = sine(80.0, sr, 0.5);
    let mut stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();

    // Sweep a lowpass from 20 kHz down to 100 Hz across the buffer.
    apply_swept_filter(&mut stereo, FilterKind::LowPass, sr, |t| {
        // Linear sweep in log-Hz for smoother audible behaviour.
        let log_start = 20_000.0_f64.log10();
        let log_end = 100.0_f64.log10();
        10.0_f64.powf(log_start + (log_end - log_start) * t)
    });

    // Take the LAST 1000 samples (where cutoff is near 100 Hz, so 80 Hz signal should be attenuated).
    let tail: Vec<f32> = stereo[stereo.len() - 1000..].to_vec();
    let head: Vec<f32> = stereo[200..1200].to_vec();
    let tail_rms = rms(&tail);
    let head_rms = rms(&head);
    assert!(
        tail_rms < 0.5 * head_rms,
        "Tail RMS ({}) should be <50% of head RMS ({}) after the sweep narrows the lowpass",
        tail_rms,
        head_rms
    );
}

#[test]
fn apply_swept_filter_stereo_channels_independent() {
    let sr = 44100.0;
    // L = 100 Hz sine, R = 10 kHz sine, interleaved.
    let n = (sr * 0.5) as usize;
    let mut stereo = Vec::with_capacity(n * 2);
    for i in 0..n {
        let l = (2.0 * std::f64::consts::PI * 100.0 * (i as f64) / sr).sin() as f32;
        let r = (2.0 * std::f64::consts::PI * 10_000.0 * (i as f64) / sr).sin() as f32;
        stereo.push(l);
        stereo.push(r);
    }

    // Static 500 Hz lowpass.
    apply_swept_filter(&mut stereo, FilterKind::LowPass, sr, |_| 500.0);

    // L (100 Hz) should pass; R (10 kHz) should be killed.
    let l_channel: Vec<f32> = stereo.iter().step_by(2).copied().collect();
    let r_channel: Vec<f32> = stereo.iter().skip(1).step_by(2).copied().collect();
    let l_rms = rms(&l_channel[200..]);
    let r_rms = rms(&r_channel[200..]);
    assert!(l_rms > 0.4, "L (100 Hz) should still be loud; got {}", l_rms);
    assert!(r_rms < 0.1, "R (10 kHz) should be killed; got {}", r_rms);
}

#[test]
fn apply_swept_filter_no_nan_no_clip() {
    let sr = 44100.0;
    let n = sr as usize;
    // Unit-amplitude white-ish noise (cheap deterministic version).
    let mut buf: Vec<f32> = (0..n * 2)
        .map(|i| (((i as u64).wrapping_mul(1103515245).wrapping_add(12345)) as f32 / u32::MAX as f32) * 2.0 - 1.0)
        .collect();

    apply_swept_filter(&mut buf, FilterKind::LowPass, sr, |t| {
        // Sweep from 50 Hz to 20 kHz in log space.
        let log_start = 50.0_f64.log10();
        let log_end = 20_000.0_f64.log10();
        10.0_f64.powf(log_start + (log_end - log_start) * t)
    });

    let max_abs = buf.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    assert!(buf.iter().all(|s| s.is_finite()), "filter produced NaN/Inf");
    assert!(max_abs < 1.5, "output max abs ({}) is unreasonably high", max_abs);
}
```

- [ ] **Step 2: Run tests, confirm failure**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test eq_test
```

Expected: 3 new failures — `apply_swept_filter` / `FilterKind` not found.

- [ ] **Step 3: Implement `apply_swept_filter` + `FilterKind`**

Append to `engine/src/eq.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    LowPass,
    HighPass,
}

/// Process a stereo interleaved buffer IN PLACE with a time-varying cutoff.
/// `cutoff_curve(t)` returns the cutoff in Hz for normalized time `t in [0, 1]`.
/// Filters are evaluated independently per channel; state persists across the buffer.
pub fn apply_swept_filter(
    samples: &mut [f32],
    kind: FilterKind,
    sample_rate: f64,
    cutoff_curve: impl Fn(f64) -> f64,
) {
    if samples.len() < 2 {
        return;
    }
    let frames = samples.len() / 2;
    let initial = cutoff_curve(0.0);
    let mut filt_l = match kind {
        FilterKind::LowPass => Biquad::lowpass(sample_rate, initial),
        FilterKind::HighPass => Biquad::highpass(sample_rate, initial),
    };
    let mut filt_r = match kind {
        FilterKind::LowPass => Biquad::lowpass(sample_rate, initial),
        FilterKind::HighPass => Biquad::highpass(sample_rate, initial),
    };

    // Update cutoff every 256 frames — fine enough for a smooth audible sweep,
    // coarse enough to avoid hammering the coefficient recompute.
    const RECOMPUTE_EVERY: usize = 256;
    for frame in 0..frames {
        if frame > 0 && frame % RECOMPUTE_EVERY == 0 {
            let t = frame as f64 / frames.max(1) as f64;
            let cutoff = cutoff_curve(t);
            filt_l.set_cutoff(sample_rate, kind, cutoff);
            filt_r.set_cutoff(sample_rate, kind, cutoff);
        }
        let l = samples[frame * 2];
        let r = samples[frame * 2 + 1];
        samples[frame * 2] = filt_l.process(l);
        samples[frame * 2 + 1] = filt_r.process(r);
    }
}
```

Add `set_cutoff` to Biquad (as a new method in either of the existing `impl Biquad` blocks):
```rust
    /// Update coefficients without resetting state. Used for sweeps.
    pub fn set_cutoff(&mut self, sample_rate: f64, kind: FilterKind, cutoff_hz: f64) {
        let (b0, b1, b2, a1, a2) = match kind {
            FilterKind::LowPass => lowpass_coeffs(sample_rate, cutoff_hz),
            FilterKind::HighPass => highpass_coeffs(sample_rate, cutoff_hz),
        };
        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test eq_test
```

Expected: 7 tests pass total.

- [ ] **Step 5: Commit**

```
git add engine/src/eq.rs engine/tests/eq_test.rs
git commit -m "feat(engine): swept-cutoff filter helper (stereo, no clip)"
```

---

### Task 5: `phrase.rs` — phrase boundaries

**Files:**
- Modify: `engine/src/phrase.rs`
- Create: `engine/tests/phrase_test.rs`

- [ ] **Step 1: Write the failing tests**

Create `engine/tests/phrase_test.rs`:
```rust
use dj_engine::phrase::phrase_boundaries;
use dj_engine::sidecar::Sidecar;

fn make_sc(downbeats: Vec<f64>, intro_end: f64, outro_start: f64) -> Sidecar {
    Sidecar {
        schema_version: 1,
        analyzer_version: "0.1.0".into(),
        source_path: "x".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        error: None,
        duration_seconds: Some(200.0),
        sample_rate: Some(44100),
        bpm: Some(120.0),
        bpm_confidence: Some(0.9),
        beats_seconds: vec![],
        downbeats_seconds: downbeats,
        key: Some("8A".into()),
        key_label: Some("A minor".into()),
        key_confidence: Some(0.8),
        energy_curve: vec![],
        energy_mean: Some(0.4),
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(200.0),
        intro_end_seconds: Some(intro_end),
        outro_start_seconds: Some(outro_start),
        is_mixable: Some(true),
        mixable_reasons: vec![],
    }
}

#[test]
fn anchors_on_first_downbeat_at_or_after_intro_end() {
    // Downbeats every 2s from 0 to 60 (i.e., 31 downbeats).
    let downbeats: Vec<f64> = (0..31).map(|i| i as f64 * 2.0).collect();
    let sc = make_sc(downbeats, 9.0, 60.0);
    let bounds = phrase_boundaries(&sc, 16);
    // First downbeat at or after intro_end=9.0 is at 10.0 (index 5).
    // Phrase boundaries: index 5, 21 (15 + 5+16). Index 37 is past outro_start.
    // So we expect [10.0, 42.0] — but 42 is also before 60, and 42 + 16*2=74 is past, so just [10.0, 42.0].
    assert_eq!(bounds, vec![10.0, 42.0]);
}

#[test]
fn spacing_is_phrase_bars_downbeats_apart() {
    let downbeats: Vec<f64> = (0..50).map(|i| i as f64).collect(); // 1s apart
    let sc = make_sc(downbeats, 0.0, 50.0);
    let bounds = phrase_boundaries(&sc, 8);
    // First boundary at downbeat[0] = 0.0. Then every 8th downbeat: 0, 8, 16, 24, 32, 40, 48.
    assert_eq!(bounds, vec![0.0, 8.0, 16.0, 24.0, 32.0, 40.0, 48.0]);
}

#[test]
fn stops_before_outro_start() {
    let downbeats: Vec<f64> = (0..100).map(|i| i as f64).collect();
    let sc = make_sc(downbeats, 0.0, 20.0);
    let bounds = phrase_boundaries(&sc, 16);
    // All boundaries must be < 20.
    assert!(bounds.iter().all(|b| *b < 20.0), "got {:?}", bounds);
}

#[test]
fn short_body_returns_empty_or_one() {
    let downbeats = vec![0.0, 1.0, 2.0, 3.0]; // only 4 downbeats
    let sc = make_sc(downbeats, 0.0, 4.0);
    let bounds = phrase_boundaries(&sc, 16);
    // Asking for 16-bar phrases but only 4 downbeats → just the first if present, else empty.
    // First downbeat at intro_end=0 IS index 0 = 0.0; next would be at index 16 (doesn't exist).
    // So just [0.0].
    assert_eq!(bounds, vec![0.0]);
}

#[test]
fn empty_downbeats_returns_empty() {
    let sc = make_sc(vec![], 0.0, 60.0);
    let bounds = phrase_boundaries(&sc, 16);
    assert!(bounds.is_empty());
}

#[test]
fn missing_intro_end_falls_back_to_zero() {
    let mut sc = make_sc(vec![0.0, 1.0, 2.0, 3.0], 0.0, 60.0);
    sc.intro_end_seconds = None;
    let bounds = phrase_boundaries(&sc, 4);
    // Anchor at 0, every 4th downbeat: 0.0 (and that's all within the 4 downbeats).
    assert_eq!(bounds, vec![0.0]);
}
```

- [ ] **Step 2: Run tests, confirm failure**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test phrase_test
```

Expected: compile error — `phrase_boundaries` not found.

- [ ] **Step 3: Implement `phrase_boundaries`**

Replace `engine/src/phrase.rs`:
```rust
//! Derive 16-bar phrase boundaries from a sidecar. Pure data.

use crate::sidecar::Sidecar;

/// Return ascending timestamps of phrase boundaries inside `sc`'s body.
///
/// Algorithm:
/// 1. Anchor on the first downbeat at or after `sc.intro_end_seconds` (or 0 if None).
/// 2. From that anchor index, every `phrase_bars`-th downbeat is a phrase boundary.
/// 3. Skip any boundaries at or after `sc.outro_start_seconds` (if set).
///
/// Returns an empty Vec if there are no downbeats or the body is too short.
pub fn phrase_boundaries(sc: &Sidecar, phrase_bars: usize) -> Vec<f64> {
    if phrase_bars == 0 || sc.downbeats_seconds.is_empty() {
        return Vec::new();
    }
    let intro_end = sc.intro_end_seconds.unwrap_or(0.0);
    let outro_start = sc.outro_start_seconds.unwrap_or(f64::INFINITY);

    let anchor_idx = sc
        .downbeats_seconds
        .iter()
        .position(|d| *d >= intro_end)
        .unwrap_or(sc.downbeats_seconds.len());

    let mut out = Vec::new();
    let mut idx = anchor_idx;
    while idx < sc.downbeats_seconds.len() {
        let t = sc.downbeats_seconds[idx];
        if t >= outro_start {
            break;
        }
        out.push(t);
        idx += phrase_bars;
    }
    out
}
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test phrase_test
```

Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/phrase.rs engine/tests/phrase_test.rs
git commit -m "feat(engine): derive phrase boundaries from sidecar"
```

---

### Task 6: `transition.rs` — length + variant decision logic

**Files:**
- Modify: `engine/src/transition.rs`
- Create: `engine/tests/transition_test.rs`

- [ ] **Step 1: Write the failing tests**

Create `engine/tests/transition_test.rs`:
```rust
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::{plan_transition, TransitionPlan};

fn sc(bpm: f64, key: &str, energy: f64, mixable: bool, downbeats: Vec<f64>, intro_end: f64, outro_start: f64) -> Sidecar {
    Sidecar {
        schema_version: 1,
        analyzer_version: "0.1.0".into(),
        source_path: "x".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        error: None,
        duration_seconds: Some(200.0),
        sample_rate: Some(44100),
        bpm: Some(bpm),
        bpm_confidence: Some(0.9),
        beats_seconds: vec![],
        downbeats_seconds: downbeats,
        key: Some(key.into()),
        key_label: Some("".into()),
        key_confidence: Some(0.8),
        energy_curve: vec![],
        energy_mean: Some(energy),
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(200.0),
        intro_end_seconds: Some(intro_end),
        outro_start_seconds: Some(outro_start),
        is_mixable: Some(mixable),
        mixable_reasons: vec![],
    }
}

fn dense_downbeats() -> Vec<f64> {
    (0..100).map(|i| i as f64).collect()
}

#[test]
fn identical_tracks_get_long_blend() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 32);
    assert!(plan.use_eq_swap);
}

#[test]
fn big_energy_jump_triggers_hard_cut() {
    let out = sc(120.0, "8A", 0.1, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.8, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 0);
    assert!(!plan.use_eq_swap);
}

#[test]
fn non_mixable_incoming_triggers_hard_cut() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.4, false, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 0);
    assert!(!plan.use_eq_swap);
}

#[test]
fn moderate_energy_jump_triggers_quick_cut() {
    let out = sc(120.0, "8A", 0.3, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.55, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 8);
    assert!(plan.use_eq_swap);
}

#[test]
fn default_pair_gets_standard_blend() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    // Adjacent key, slightly different BPM, slight energy delta — none extreme.
    let inc = sc(123.0, "7A", 0.45, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 16);
    assert!(plan.use_eq_swap);
}

#[test]
fn relative_key_triggers_quick_cut() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    // 8A minor vs 8B major are relative keys.
    let inc = sc(120.0, "8B", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 8);
    assert!(plan.use_eq_swap);
}

#[test]
fn plan_returns_TransitionPlan_struct() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let _plan: TransitionPlan = plan_transition(&out, &inc);
}
```

- [ ] **Step 2: Run tests, confirm failure**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test transition_test
```

Expected: compile error.

- [ ] **Step 3: Implement `transition.rs` (length + variant; phrase snapping comes next task)**

Replace `engine/src/transition.rs`:
```rust
//! Pick crossfade length, variant, and snapped start times for a pair of tracks.

use crate::camelot::{CamelotKey, KeyMatch};
use crate::phrase::phrase_boundaries;
use crate::sidecar::Sidecar;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionPlan {
    pub outgoing_crossfade_start_seconds: f64,
    pub incoming_crossfade_start_seconds: f64,
    /// 0 means hard cut (no crossfade). Other values: 8, 16, 32 bars.
    pub length_bars: usize,
    pub use_eq_swap: bool,
}

// Tunable thresholds. Adjust by ear after listening to several hours of output.
const HARD_CUT_ENERGY_DELTA: f64 = 0.35;
const QUICK_CUT_ENERGY_DELTA: f64 = 0.20;
const LONG_BLEND_BPM_DELTA: f64 = 0.02;
const LONG_BLEND_ENERGY_DELTA: f64 = 0.10;
const PHRASE_BARS: usize = 16;

pub fn plan_transition(outgoing: &Sidecar, incoming: &Sidecar) -> TransitionPlan {
    let (length_bars, use_eq_swap) = pick_length_and_variant(outgoing, incoming);

    // Snapping (filled in by Task 7). For now use raw values.
    let outgoing_crossfade_start_seconds =
        outgoing.outro_start_seconds.unwrap_or(0.0);
    let incoming_crossfade_start_seconds =
        incoming.intro_end_seconds.unwrap_or(0.0);

    // Silence unused-import warning until Task 7 wires it in.
    let _ = phrase_boundaries(incoming, PHRASE_BARS);

    TransitionPlan {
        outgoing_crossfade_start_seconds,
        incoming_crossfade_start_seconds,
        length_bars,
        use_eq_swap,
    }
}

fn pick_length_and_variant(out: &Sidecar, inc: &Sidecar) -> (usize, bool) {
    let out_bpm = out.bpm.unwrap_or(120.0);
    let inc_bpm = inc.bpm.unwrap_or(out_bpm);
    let bpm_delta = ((inc_bpm - out_bpm).abs() / out_bpm.max(1.0)).abs();

    let out_energy = out.energy_mean.unwrap_or(0.5);
    let inc_energy = inc.energy_mean.unwrap_or(out_energy);
    let energy_delta = (inc_energy - out_energy).abs();

    let key_match = match (
        out.key.as_deref().and_then(CamelotKey::parse),
        inc.key.as_deref().and_then(CamelotKey::parse),
    ) {
        (Some(a), Some(b)) => a.compatibility(&b),
        _ => KeyMatch::None,
    };

    let is_non_mixable = inc.is_mixable != Some(true);

    let bars = if is_non_mixable || energy_delta > HARD_CUT_ENERGY_DELTA {
        0
    } else if energy_delta > QUICK_CUT_ENERGY_DELTA
        || matches!(key_match, KeyMatch::Relative | KeyMatch::None)
    {
        8
    } else if bpm_delta <= LONG_BLEND_BPM_DELTA
        && energy_delta <= LONG_BLEND_ENERGY_DELTA
        && matches!(key_match, KeyMatch::Identical)
    {
        32
    } else {
        16
    };

    let use_eq_swap = bars >= 8;
    (bars, use_eq_swap)
}
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test transition_test
```

Expected: 7 tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/transition.rs engine/tests/transition_test.rs
git commit -m "feat(engine): transition length + variant decision logic"
```

---

### Task 7: `transition.rs` — phrase snapping

**Files:**
- Modify: `engine/src/transition.rs`
- Modify: `engine/tests/transition_test.rs`

- [ ] **Step 1: Add failing tests for snapping**

Append to `engine/tests/transition_test.rs`:
```rust
#[test]
fn outgoing_start_snaps_to_nearest_phrase_boundary() {
    // Downbeats every 1s from 0 to 60. phrase_bars=16 → boundaries at indices 0, 16, 32, 48.
    // intro_end=0 → anchor at downbeat[0]. outro_start=42 → phrase boundaries [0, 16, 32].
    // Nearest to outro_start=42 is 32.
    let downbeats: Vec<f64> = (0..60).map(|i| i as f64).collect();
    let out = sc(120.0, "8A", 0.4, true, downbeats.clone(), 0.0, 42.0);
    let inc = sc(120.0, "8A", 0.4, true, downbeats, 0.0, 60.0);
    let plan = plan_transition(&out, &inc);
    assert!(
        (plan.outgoing_crossfade_start_seconds - 32.0).abs() < 1e-6,
        "expected 32.0 (nearest phrase boundary), got {}",
        plan.outgoing_crossfade_start_seconds
    );
}

#[test]
fn incoming_start_is_first_phrase_boundary() {
    let downbeats: Vec<f64> = (0..60).map(|i| i as f64).collect();
    let out = sc(120.0, "8A", 0.4, true, downbeats.clone(), 0.0, 40.0);
    // intro_end=5, phrase boundaries anchored at downbeat[5]=5 → first boundary is 5.0.
    let inc = sc(120.0, "8A", 0.4, true, downbeats, 5.0, 60.0);
    let plan = plan_transition(&out, &inc);
    assert!(
        (plan.incoming_crossfade_start_seconds - 5.0).abs() < 1e-6,
        "expected 5.0 (first phrase boundary), got {}",
        plan.incoming_crossfade_start_seconds
    );
}

#[test]
fn falls_back_to_raw_values_when_no_phrase_boundaries() {
    // Empty downbeats → phrase_boundaries returns empty → fall back to raw outro_start / intro_end.
    let out = sc(120.0, "8A", 0.4, true, vec![], 0.0, 42.0);
    let inc = sc(120.0, "8A", 0.4, true, vec![], 5.0, 60.0);
    let plan = plan_transition(&out, &inc);
    assert!((plan.outgoing_crossfade_start_seconds - 42.0).abs() < 1e-6);
    assert!((plan.incoming_crossfade_start_seconds - 5.0).abs() < 1e-6);
}
```

- [ ] **Step 2: Run tests to confirm new ones fail**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test transition_test
```

Expected: 3 failures (the new snap tests). Earlier 7 still pass.

- [ ] **Step 3: Wire snapping into `plan_transition`**

Replace the body of `plan_transition` in `engine/src/transition.rs` (everything from `pub fn plan_transition...` down to the closing brace of that function) with:
```rust
pub fn plan_transition(outgoing: &Sidecar, incoming: &Sidecar) -> TransitionPlan {
    let (length_bars, use_eq_swap) = pick_length_and_variant(outgoing, incoming);

    let raw_outgoing_start = outgoing.outro_start_seconds.unwrap_or(0.0);
    let raw_incoming_start = incoming.intro_end_seconds.unwrap_or(0.0);

    let outgoing_boundaries = phrase_boundaries(outgoing, PHRASE_BARS);
    let incoming_boundaries = phrase_boundaries(incoming, PHRASE_BARS);

    let outgoing_crossfade_start_seconds = nearest(&outgoing_boundaries, raw_outgoing_start)
        .unwrap_or(raw_outgoing_start);

    let incoming_crossfade_start_seconds = incoming_boundaries
        .first()
        .copied()
        .unwrap_or(raw_incoming_start);

    TransitionPlan {
        outgoing_crossfade_start_seconds,
        incoming_crossfade_start_seconds,
        length_bars,
        use_eq_swap,
    }
}

fn nearest(boundaries: &[f64], target: f64) -> Option<f64> {
    boundaries
        .iter()
        .copied()
        .min_by(|a, b| {
            (*a - target)
                .abs()
                .partial_cmp(&(*b - target).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}
```

- [ ] **Step 4: Run all transition tests**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test transition_test
```

Expected: 10 tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/transition.rs engine/tests/transition_test.rs
git commit -m "feat(engine): snap transition start times to phrase boundaries"
```

---

### Task 8: `crossfade.rs` — `mix_crossfade_eq`

**Files:**
- Modify: `engine/src/crossfade.rs`
- Create: `engine/tests/crossfade_eq_test.rs`

- [ ] **Step 1: Write the failing tests**

Create `engine/tests/crossfade_eq_test.rs`:
```rust
use dj_engine::crossfade::mix_crossfade_eq;

fn sine_stereo(freq_hz: f64, frames: usize, sample_rate: f64) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let s = (2.0 * std::f64::consts::PI * freq_hz * (i as f64) / sample_rate).sin() as f32;
        out.push(s);
        out.push(s);
    }
    out
}

fn rms_window(samples: &[f32], start: usize, end: usize) -> f32 {
    let slice = &samples[start..end];
    let sum: f64 = slice.iter().map(|s| (*s as f64).powi(2)).sum();
    ((sum / slice.len() as f64).sqrt()) as f32
}

#[test]
fn mid_region_contains_energy() {
    let sr = 44100.0;
    let frames = 4 * sr as usize; // 4 seconds
    let outgoing = sine_stereo(440.0, frames, sr);   // A4
    let incoming = sine_stereo(523.0, frames, sr);   // C5
    let mixed = mix_crossfade_eq(&outgoing, &incoming, frames, sr);
    assert_eq!(mixed.len(), outgoing.len());

    // Middle 25% — both should have some signal energy here.
    let n = mixed.len();
    let mid_rms = rms_window(&mixed, 3 * n / 8, 5 * n / 8);
    assert!(mid_rms > 0.05, "midpoint RMS too low ({}), expected nontrivial signal", mid_rms);
}

#[test]
fn output_length_matches_input() {
    let sr = 44100.0;
    let frames = sr as usize; // 1 second
    let outgoing = sine_stereo(440.0, frames, sr);
    let incoming = sine_stereo(523.0, frames, sr);
    let mixed = mix_crossfade_eq(&outgoing, &incoming, frames, sr);
    assert_eq!(mixed.len(), frames * 2);
}

#[test]
fn no_nan_or_clip() {
    let sr = 44100.0;
    let frames = 2 * sr as usize;
    let outgoing = sine_stereo(80.0, frames, sr);   // bass sine
    let incoming = sine_stereo(80.0, frames, sr);
    let mixed = mix_crossfade_eq(&outgoing, &incoming, frames, sr);
    let max_abs = mixed.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    assert!(mixed.iter().all(|s| s.is_finite()), "NaN or Inf in output");
    assert!(max_abs < 1.5, "max abs sample {} unreasonably high", max_abs);
}
```

- [ ] **Step 2: Run tests, confirm failure**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test crossfade_eq_test
```

Expected: compile error.

- [ ] **Step 3: Implement `mix_crossfade_eq`**

Open `engine/src/crossfade.rs`. At the top of the file, after the existing `use` lines, add:
```rust
use crate::eq::{apply_swept_filter, FilterKind};
```

At the bottom of the file (after the existing `mix_crossfade` function), add:
```rust
/// Equal-power crossfade with a 2-band EQ swap centered at the midpoint.
///
/// `crossfade_frames` is the number of stereo frames in the transition (length in
/// samples divided by 2). The buffers MUST have `crossfade_frames * 2` interleaved
/// samples each (or the function takes the shorter of the two).
pub fn mix_crossfade_eq(
    outgoing: &[f32],
    incoming: &[f32],
    crossfade_frames: usize,
    sample_rate: f64,
) -> Vec<f32> {
    let mut outgoing_filtered = outgoing.to_vec();
    let mut incoming_filtered = incoming.to_vec();

    // Outgoing: lowpass that sweeps 22 kHz → 200 Hz across t in [0.375, 0.5].
    // The cutoff plateau on either side ensures the highs pass cleanly outside the swap window.
    apply_swept_filter(
        &mut outgoing_filtered,
        FilterKind::LowPass,
        sample_rate,
        |t| {
            let lo = 200.0_f64;
            let hi = 22_000.0_f64;
            if t <= 0.375 {
                hi
            } else if t >= 0.5 {
                lo
            } else {
                // Log-space sweep from hi to lo across [0.375, 0.5].
                let local = (t - 0.375) / (0.5 - 0.375);
                let log_hi = hi.log10();
                let log_lo = lo.log10();
                10.0_f64.powf(log_hi + (log_lo - log_hi) * local)
            }
        },
    );

    // Incoming: highpass that sweeps 200 Hz → 22 kHz across t in [0.5, 0.625].
    apply_swept_filter(
        &mut incoming_filtered,
        FilterKind::HighPass,
        sample_rate,
        |t| {
            let lo = 200.0_f64;
            let hi = 22_000.0_f64;
            if t <= 0.5 {
                lo
            } else if t >= 0.625 {
                hi
            } else {
                let local = (t - 0.5) / (0.625 - 0.5);
                let log_lo = lo.log10();
                let log_hi = hi.log10();
                10.0_f64.powf(log_lo + (log_hi - log_lo) * local)
            }
        },
    );

    // Apply equal-power volume curves on top of the filtered signals.
    mix_crossfade(&outgoing_filtered, &incoming_filtered, crossfade_frames)
}
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test crossfade_eq_test
```

Expected: 3 tests pass. Also confirm the existing 3 crossfade tests still pass:

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test crossfade_test
```

Expected: 3 tests still pass.

- [ ] **Step 5: Commit**

```
git add engine/src/crossfade.rs engine/tests/crossfade_eq_test.rs
git commit -m "feat(engine): mix_crossfade_eq — equal-power + 2-band EQ swap"
```

---

### Task 9: `mixer.rs` — consume `TransitionPlan` (non-hard-cut path)

**Files:**
- Modify: `engine/src/mixer.rs`

This task wires the new modules into the mixer for the **standard crossfade path** (length_bars > 0). Hard cut is added in Task 10.

- [ ] **Step 1: Read the current `mixer.rs` to understand the structure**

Read `engine/src/mixer.rs`. It currently:
1. Computes `outgoing_outro_start`, `bar_seconds`, `crossfade_seconds`.
2. Pre-fetches `stretched` via `decode_stretched`.
3. Streams outgoing until `samples_pushed >= target_samples`.
4. Reads crossfade tail from outgoing, mixes with incoming head via `mix_crossfade`, pushes.
5. Spawns new playing decoder.

- [ ] **Step 2: Replace the mixer body to use `TransitionPlan`**

Replace the entire contents of `engine/src/mixer.rs` with:

```rust
//! Mixer: orchestrates decode → align → crossfade → ring buffer.

use tracing::info;

use crate::buffer::{Producer, SampleProducer};
use crate::crossfade::{mix_crossfade, mix_crossfade_eq};
use crate::error::Result;
use crate::ffmpeg::decoder::PlayingDecoder;
use crate::ffmpeg::mixwindow::decode_stretched;
use crate::ffmpeg::SAMPLE_RATE;
use crate::library::LoadedTrack;
use crate::transition::{plan_transition, TransitionPlan};

const READ_CHUNK_FRAMES: usize = 4096;

async fn push_all(producer: &mut SampleProducer, samples: &[f32]) {
    let mut offset = 0;
    while offset < samples.len() {
        let n = producer.push_slice(&samples[offset..]);
        if n == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        } else {
            offset += n;
        }
    }
}

/// Run a single transition from `outgoing` to `incoming`. Returns the new playing-decoder.
pub async fn play_until_transition_and_crossfade(
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    decoder: &mut PlayingDecoder,
    producer: &mut SampleProducer,
) -> Result<PlayingDecoder> {
    let plan = plan_transition(&outgoing.sidecar, &incoming.sidecar);

    if plan.length_bars == 0 {
        // Hard cut (Task 10).
        return execute_hard_cut(outgoing, incoming, decoder, producer, &plan).await;
    }

    execute_crossfade(outgoing, incoming, decoder, producer, &plan).await
}

async fn execute_crossfade(
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    decoder: &mut PlayingDecoder,
    producer: &mut SampleProducer,
    plan: &TransitionPlan,
) -> Result<PlayingDecoder> {
    let outgoing_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
    let incoming_bpm = incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
    let ratio = outgoing_bpm / incoming_bpm.max(1.0);

    let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
    let crossfade_seconds = plan.length_bars as f64 * bar_seconds;

    let incoming_path: std::path::PathBuf = incoming.sidecar.source_path.clone().into();

    // Pre-fetch a stretched window around the incoming crossfade-start point.
    // We grab from start - 1 bar (small head room) to start + length_bars (the crossfade region).
    let stretched_start = (plan.incoming_crossfade_start_seconds - bar_seconds).max(0.0);
    let stretched_end =
        plan.incoming_crossfade_start_seconds + crossfade_seconds + bar_seconds;
    let stretched =
        decode_stretched(&incoming_path, stretched_start, stretched_end, ratio).await?;

    // The crossfade audio inside `stretched` starts at offset (incoming_crossfade_start - stretched_start)
    // in track-time seconds; the stretched buffer has 1x sample rate after atempo, so this offset
    // in samples is just (offset_seconds * SR * 2) — no rate correction needed because atempo preserves SR.
    let offset_seconds = plan.incoming_crossfade_start_seconds - stretched_start;
    let offset_samples = (offset_seconds * SAMPLE_RATE as f64) as usize * 2;
    let crossfade_frames = (crossfade_seconds * SAMPLE_RATE as f64) as usize;
    let crossfade_samples = crossfade_frames * 2;

    let incoming_head_end = (offset_samples + crossfade_samples).min(stretched.len());
    let mut incoming_head = stretched[offset_samples.min(stretched.len())..incoming_head_end].to_vec();
    if incoming_head.len() < crossfade_samples {
        incoming_head.resize(crossfade_samples, 0.0);
    }

    // 1) Stream outgoing PCM until the decoder's output cursor reaches outgoing_crossfade_start.
    let target_seconds =
        (plan.outgoing_crossfade_start_seconds - decoder.start_seconds).max(0.0);
    let target_samples = (target_seconds * SAMPLE_RATE as f64) as usize * 2;
    let mut pushed: usize = 0;

    while pushed < target_samples {
        let pcm = decoder.read_frames(READ_CHUNK_FRAMES).await?;
        if pcm.is_empty() {
            break;
        }
        let want = target_samples - pushed;
        let to_push = pcm.len().min(want);
        push_all(producer, &pcm[..to_push]).await;
        pushed += to_push;
    }

    // 2) Read the crossfade tail of outgoing.
    let mut outgoing_tail = decoder.read_frames(crossfade_frames).await?;
    if outgoing_tail.len() < crossfade_samples {
        outgoing_tail.resize(crossfade_samples, 0.0);
    }

    // 3) Mix.
    let mixed = if plan.use_eq_swap {
        mix_crossfade_eq(&outgoing_tail, &incoming_head, crossfade_frames, SAMPLE_RATE as f64)
    } else {
        mix_crossfade(&outgoing_tail, &incoming_head, crossfade_frames)
    };
    push_all(producer, &mixed).await;

    info!(
        outgoing = %outgoing.sidecar.source_path,
        incoming = %incoming.sidecar.source_path,
        length_bars = plan.length_bars,
        eq = plan.use_eq_swap,
        "crossfade complete"
    );

    // 4) Spawn the new playing decoder past the crossfade region on the incoming side.
    let new_start = plan.incoming_crossfade_start_seconds + crossfade_seconds;
    let cue_out = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
    let new_decoder = PlayingDecoder::spawn(&incoming_path, new_start, cue_out).await?;
    Ok(new_decoder)
}

/// Placeholder — implemented in Task 10.
async fn execute_hard_cut(
    _outgoing: &LoadedTrack,
    _incoming: &LoadedTrack,
    _decoder: &mut PlayingDecoder,
    _producer: &mut SampleProducer,
    _plan: &TransitionPlan,
) -> Result<PlayingDecoder> {
    unimplemented!("hard cut path — Task 10")
}
```

- [ ] **Step 3: Build to verify it compiles**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" build --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

Expected: success.

- [ ] **Step 4: Run existing tests to make sure regressions haven't crept in**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

Expected: all current tests pass (eq + phrase + transition + crossfade + crossfade_eq + everything from before).

- [ ] **Step 5: Commit**

```
git add engine/src/mixer.rs
git commit -m "feat(engine): mixer uses TransitionPlan for non-hard-cut path"
```

---

### Task 10: `mixer.rs` — hard cut path

**Files:**
- Modify: `engine/src/mixer.rs`

- [ ] **Step 1: Replace the `execute_hard_cut` stub with the real implementation**

In `engine/src/mixer.rs`, replace the `async fn execute_hard_cut(...)` placeholder with:

```rust
/// Hard cut: play outgoing up to the snapped boundary, then splice incoming's first frame
/// directly with no overlap and no volume blending.
async fn execute_hard_cut(
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    decoder: &mut PlayingDecoder,
    producer: &mut SampleProducer,
    plan: &TransitionPlan,
) -> Result<PlayingDecoder> {
    let outgoing_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
    let incoming_bpm = incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
    let ratio = outgoing_bpm / incoming_bpm.max(1.0);
    let bar_seconds = (60.0 / outgoing_bpm) * 4.0;

    let incoming_path: std::path::PathBuf = incoming.sidecar.source_path.clone().into();

    // 1) Stream outgoing until we reach the snapped boundary, exactly the same as the
    //    crossfade case — but no crossfade region after.
    let target_seconds =
        (plan.outgoing_crossfade_start_seconds - decoder.start_seconds).max(0.0);
    let target_samples = (target_seconds * SAMPLE_RATE as f64) as usize * 2;
    let mut pushed: usize = 0;

    while pushed < target_samples {
        let pcm = decoder.read_frames(READ_CHUNK_FRAMES).await?;
        if pcm.is_empty() {
            break;
        }
        let want = target_samples - pushed;
        let to_push = pcm.len().min(want);
        push_all(producer, &pcm[..to_push]).await;
        pushed += to_push;
    }

    // 2) Pre-fetch a tiny bit of the incoming track at the snapped start, time-stretched.
    //    We grab 1 bar so the splice has audio ready, then spawn the long-running decoder
    //    to take over for the rest.
    let stretched_start = plan.incoming_crossfade_start_seconds;
    let stretched_end = plan.incoming_crossfade_start_seconds + bar_seconds;
    let stretched =
        decode_stretched(&incoming_path, stretched_start, stretched_end, ratio).await?;
    push_all(producer, &stretched).await;

    info!(
        outgoing = %outgoing.sidecar.source_path,
        incoming = %incoming.sidecar.source_path,
        "hard cut complete"
    );

    // 3) Spawn the new playing decoder past the bar we just spliced.
    let new_start = stretched_end;
    let cue_out = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
    let new_decoder = PlayingDecoder::spawn(&incoming_path, new_start, cue_out).await?;
    Ok(new_decoder)
}
```

- [ ] **Step 2: Build to verify**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" build --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

Expected: success.

- [ ] **Step 3: Run all tests**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

Expected: all green.

- [ ] **Step 4: Commit**

```
git add engine/src/mixer.rs
git commit -m "feat(engine): hard cut transition variant"
```

---

### Task 11: cargo fmt + clippy

**Files:** none (tooling)

- [ ] **Step 1: Format**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" fmt --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

- [ ] **Step 2: Clippy**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" clippy --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --all-targets -- -D warnings
```

Fix every warning by editing the implicated file. Likely categories:
- `needless_borrow`, `redundant_clone`, `useless_conversion` — usually one-line fixes
- Unused imports in the modified mixer — remove them

If a clippy lint conflicts with code clarity (e.g., it wants you to inline a clearly-named binding), allow it with `#[allow(clippy::<lint>)]` at the smallest scope and add a one-line comment explaining why.

- [ ] **Step 3: Full test sweep**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

Expected: all tests pass (24 existing + 7 eq + 6 phrase + 10 transition + 3 crossfade_eq = ~50 total).

- [ ] **Step 4: Commit (if anything changed)**

```
git add -A
git commit -m "chore(engine): satisfy clippy + fmt for mixer enhancements"
```

---

### Task 12: End-to-end smoke test

**Files:** none (manual procedure)

- [ ] **Step 1: Stop any running engine**

```powershell
Get-Process dj-engine -ErrorAction SilentlyContinue | Stop-Process -Force
```

- [ ] **Step 2: Rebuild release binary**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" build --release --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

- [ ] **Step 3: Start Icecast if not already running**

```powershell
if (-not (Get-Process icecast -ErrorAction SilentlyContinue)) {
    $cfg = 'C:\Users\f.nefzi\AppData\Local\Icecast-run\icecast.xml'
    Start-Process -FilePath 'C:\Program Files (x86)\Icecast\bin\icecast.exe' `
        -ArgumentList '-c', $cfg -WindowStyle Hidden `
        -RedirectStandardOutput 'C:\Users\f.nefzi\AppData\Local\Icecast-run\stdout.log' `
        -RedirectStandardError 'C:\Users\f.nefzi\AppData\Local\Icecast-run\stderr.log'
    Start-Sleep -Seconds 2
}
```

- [ ] **Step 4: Launch the engine**

```powershell
$env:ICECAST_URL = 'icecast://localhost:8000/dj.mp3'
$env:ICECAST_USER = 'source'
$env:ICECAST_PASSWORD = 'hackme'
$env:ICECAST_ADMIN_USER = 'admin'
$env:ICECAST_ADMIN_PASSWORD = 'hackme'
$env:ICECAST_FORMAT = 'mp3'
$env:ICECAST_BITRATE = '192k'
$env:RUST_LOG = 'info'

$logBase = 'C:\Users\f.nefzi\AppData\Local\Icecast-run'
Clear-Content "$logBase\engine-stdout.log","$logBase\engine-stderr.log" -ErrorAction SilentlyContinue

Start-Process `
    -FilePath 'C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\target\release\dj-engine.exe' `
    -ArgumentList '--sidecars','C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzed-real',
                  '--music-root','C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\data' `
    -RedirectStandardOutput "$logBase\engine-stdout.log" `
    -RedirectStandardError "$logBase\engine-stderr.log" `
    -WindowStyle Hidden -PassThru
```

- [ ] **Step 5: Listen via VLC**

Open `http://localhost:8000/dj.mp3` in VLC or any browser-based MP3 player. Listen for at least 10 minutes (ideally through 3-4 transitions).

Listening checklist:
- Transitions don't have two basslines fighting (EQ swap working)
- Transitions feel like they land on a "natural" musical moment (phrase alignment working)
- At least one transition feels longer than 16 bars, OR at least one feels shorter / harder (adaptive length working). With 24 tracks in the library, you may need to listen ~15-20 minutes to hear all three variants in a row.

- [ ] **Step 6: Tail the engine log for crossfade lines**

```
Get-Content C:\Users\f.nefzi\AppData\Local\Icecast-run\engine-stdout.log -Tail 30 -Wait
```

Look for `crossfade complete` or `hard cut complete` log lines. Confirm `length_bars` and `eq=true|false` vary across transitions.

- [ ] **Step 7: If any issue is heard, document it**

If anything sounds off (e.g., a particular pair causes an audible glitch at the swap point), note:
- The two tracks involved (from the log)
- What it sounded like
- The plan's `length_bars` / `use_eq_swap` for that pair

Don't fix it now — open it as a follow-up. The thresholds in `transition.rs` are deliberate tuning knobs and the spec calls them out as "tune after listening."

---

## Self-review

### Spec coverage

| Spec section | Task(s) | Notes |
|---|---|---|
| `eq.rs` — Biquad lowpass/highpass | 2, 3 | Cookbook coeffs, Q = 1/√2 |
| `eq.rs` — `apply_swept_filter` | 4 | Stereo state, log-space sweep |
| `phrase.rs` — `phrase_boundaries` | 5 | Anchored on intro_end, stops at outro_start |
| `transition.rs` — length/variant | 6 | All four variants, named thresholds |
| `transition.rs` — phrase snapping | 7 | Outgoing snaps to nearest; incoming uses first |
| `crossfade.rs::mix_crossfade_eq` | 8 | 2-band swap centered at midpoint |
| `mixer.rs` — TransitionPlan path | 9 | Standard crossfade |
| `mixer.rs` — hard cut path | 10 | Splice without overlap |
| Clippy + fmt | 11 | Standard cleanup |
| End-to-end smoke | 12 | Manual listening checklist |

**Gaps:** none. All spec sections map to tasks.

### Placeholder scan

Searched for "TBD", "TODO", "implement later", "appropriate error handling", "similar to task N", "etc." — none in the plan. Task 9 uses a `unimplemented!()` stub for `execute_hard_cut` that's deliberately replaced in Task 10; this is acceptable because Task 10 is the immediately following task.

### Type consistency

- `Biquad` has `lowpass`, `highpass`, `set_cutoff`, `process` methods — used consistently in Tasks 2, 3, 4 and in `crossfade.rs` via `apply_swept_filter`.
- `FilterKind` enum: `LowPass`, `HighPass` — consistent everywhere.
- `phrase_boundaries(sc: &Sidecar, phrase_bars: usize) -> Vec<f64>` — consistent in `phrase.rs` test, `transition.rs` usage.
- `TransitionPlan { outgoing_crossfade_start_seconds, incoming_crossfade_start_seconds, length_bars, use_eq_swap }` — consistent across Tasks 6, 7, 9, 10.
- `plan_transition(&outgoing.sidecar, &incoming.sidecar)` — same signature in mixer (Task 9).
- `mix_crossfade_eq(outgoing: &[f32], incoming: &[f32], crossfade_frames: usize, sample_rate: f64)` — consistent in Tasks 8 and 9.

### Scope

One focused subsystem (mixer enhancements), engine-side only, no external deps. Twelve tasks each ~2-15 minutes of focused work; entire plan executable in a single session.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-27-mixer-dj-feel.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
