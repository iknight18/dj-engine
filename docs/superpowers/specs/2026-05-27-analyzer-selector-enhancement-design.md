# Analyzer + Selector Enhancement — Smarter Transitions

**Date:** 2026-05-27
**Status:** Approved for planning
**Builds on:** `2026-05-26-dj-engine-design.md`, `2026-05-27-mixer-dj-feel-design.md`

## Summary

The mixer DJ-feel enhancement landed but hard-cuts fire too often. Root cause investigation on the 24-track real library showed 100% of tracks flagged `is_mixable=false`, predominantly because `madmom` is unavailable on Python 3.13/Windows so the downbeat detector always uses the "every 4th beat" fallback, which the classifier treats as a disqualifier. The transition planner's first rule converts non-mixable incoming to hard-cut, so every transition was sharp.

This spec makes the analyzer's classification more nuanced, replaces the crude downbeat fallback with a librosa-based phrase detector, adds per-band (bass/mid/high) energy to the sidecar, teaches the selector to prefer pairs that will transition smoothly, and relaxes the engine's hard-cut threshold so it fires only on truly extreme energy jumps. Hard cuts become rare; the listener hears blends as the default.

## Goals

- Hard cuts fire only when musically appropriate (extreme energy jumps) — not because of metadata weakness.
- Most transitions in the user's library produce 16-bar standard blends; some produce 32-bar long blends; some produce 8-bar quick cuts. Variety is audible across a 15-minute listen.
- Analyzer classification reflects real track properties, not toolchain limitations (madmom absence).
- New analyzer outputs (phrase boundaries, per-band energy) are *consumed* by the engine — no dead fields.
- Backwards compatible: schema v1 sidecars still play correctly; v2 sidecars get the smarter behavior.

## Non-Goals (this iteration)

- Full structural segmentation (intro / verse / chorus / drop labels). Considered as Option 3 in brainstorming; deferred. If we want it later, it's a separate spec.
- Replacing librosa with a deep-learning model for beat / downbeat detection. We're staying within the existing dependency set.
- Vocal detection. Useful for avoiding back-to-back vocal-heavy tracks; deferred until per-band energy proves insufficient.
- Listening-history persistence across sessions. Selector still resets recency on each engine start.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ Analyzer (Python)                                           │
│  - mixability.py: split is_mixable into                     │
│      is_beatmatchable + is_phrase_aligned                   │
│  - detect_downbeats.py: librosa onset-based phrase detector │
│    (replaces "every 4th beat" fallback)                     │
│  - detect_bands.py [NEW]: per-band RMS curves               │
│  - analyze.py: orchestrator emits new fields                │
└─────────────────────────────────────────────────────────────┘
                            │ writes
                            ▼
┌─────────────────────────────────────────────────────────────┐
│ schema/sidecar.schema.json — version 2                      │
│  +phrase_boundaries_seconds, +phrase_confidence             │
│  +band_energy_curves { bass, mid, high }                    │
│  +band_energy_means  { bass, mid, high }                    │
│  +is_beatmatchable, +is_phrase_aligned                      │
│  Old "is_mixable" remains as derived alias                  │
│  Backwards compatible: v1 sidecars still validate           │
└─────────────────────────────────────────────────────────────┘
                            │ consumed by
                            ▼
┌─────────────────────────────────────────────────────────────┐
│ Engine (Rust)                                               │
│  - sidecar.rs: add new fields as Option<...>                │
│    is_beatmatchable.unwrap_or(is_mixable.unwrap_or(false))  │
│  - phrase.rs: prefer sidecar.phrase_boundaries when set     │
│  - selector.rs: transition-aware scoring                    │
│  - transition.rs: relax hard-cut trigger                    │
│  - crossfade.rs: band-aware EQ swap window                  │
└─────────────────────────────────────────────────────────────┘
```

### Boundary discipline

- **Schema is versioned, additive.** v2 = v1 + new optional fields. Engine handles both.
- **`phrase.rs` is "prefer-sidecar-if-set."** Same return type, same callers, no orchestration change.
- **Selector adds one term to its score function.** Doesn't reshape the selector.
- **Engine never decides what "good downbeats" mean** — it consumes the analyzer's classification verdict.

## Component 1 — Analyzer changes

### 1.1 — Relax mixability classification

Today: any `fallback_4_4` downbeat method → `no_clear_downbeat` reason → `is_mixable=false`. That fires on every track since madmom is unavailable.

New: split into two flags, both written to the sidecar:

```python
is_beatmatchable: bool   # safe to time-stretch + EQ-swap crossfade
is_phrase_aligned: bool  # downbeats accurate enough to snap to phrase boundaries
```

**`is_beatmatchable` is `false` only if:**
- decode error (no audio data)
- duration < 90 s (too short for a meaningful transition)
- `bpm_confidence < 0.4` (BPM detection wildly unreliable)

Note the relaxation: BPM confidence threshold drops from 0.6 → 0.4, and `fallback_4_4` downbeats no longer disqualify.

**`is_phrase_aligned` is `true` only if:**
- `madmom` downbeat method was used and succeeded, OR
- new librosa phrase detector (1.2) returned `phrase_confidence ≥ 0.5`

`is_mixable` stays in the schema as a derived alias: `is_mixable = is_beatmatchable`. Old engine code and v1 schema validation still work.

Updated `mixable_reasons` enum gains two diagnostic entries (kept optional in schema):
- `phrase_low_confidence` — added when `is_phrase_aligned=false`
- `bass_only_track` — reserved for future use; not emitted in this iteration

### 1.2 — Librosa-based phrase detection

Replace the "every 4th beat = downbeat" fallback in `detect_downbeats.py`. New function `detect_phrases_librosa()` runs whenever madmom isn't available.

Algorithm:
1. Compute beat-synchronous onset strength: `librosa.onset.onset_strength(y, sr)` → strength envelope; resample to beats using the beat list from `detect_beats`.
2. Build a self-similarity matrix on beat-synchronous chroma + onset features (single matrix, dimensions `n_beats × n_beats`).
3. For each candidate period `P ∈ {4, 8, 16}`, compute the mean diagonal score at offset `P`. The strongest period is the dominant phrase length in beats.
4. Anchor on the first beat where similarity to `beat + P` is above the matrix median (the first "phrase repeat").
5. Emit `phrase_boundaries_seconds = beats[anchor::P]` filtered to `< outro_start_seconds`.
6. `phrase_confidence = (strongest_period_score - second_strongest) / strongest_period_score`, clipped to `[0, 1]`.

If `phrase_confidence < 0.5`, emit no boundaries and set `is_phrase_aligned=false`. The track still plays — the engine will fall back to bar-snapping (Section 3.2).

Existing `madmom` path stays as the preferred branch when madmom is importable. The "every 4th beat" fallback is removed.

Per-track cost: ~2 seconds of librosa work added. Re-analyzing the 24-track library: 3-4 minutes total.

### 1.3 — Per-band energy (new module)

New `detect_bands.py` exposes:

```python
def detect_band_energies(audio: np.ndarray, sr: int) -> dict[str, Any]:
    """
    Returns:
        {
          "curves": {"bass": [...], "mid": [...], "high": [...]},  # per-second, normalized
          "means":  {"bass": float, "mid": float, "high": float}
        }
    """
```

Implementation:
1. Pre-filter the audio three times using `scipy.signal.butter` Butterworth biquads, order 4:
   - Bass: low-pass at 250 Hz
   - Mid: band-pass 250 Hz – 2000 Hz
   - High: high-pass at 2000 Hz
2. For each band, compute per-second RMS with `librosa.feature.rms(y=band, frame_length=sr, hop_length=sr)`.
3. Normalize per-band by dividing by the 95th percentile of that band's RMS, clipped to `[0, 1]` (same approach as the existing `detect_energy.py`).
4. Mean = mean of the normalized curve.

The orchestrator (`analyze.py`) calls `detect_band_energies` once per track and copies the result into the sidecar dict.

### 1.4 — Sidecar additions

Output for v2 sidecar gains these top-level fields:

```json
{
  "schema_version": 2,

  "phrase_boundaries_seconds": [12.0, 27.4, 42.8, 58.2, ...],
  "phrase_confidence": 0.72,

  "band_energy_curves": {
    "bass": [0.42, 0.51, ...],
    "mid":  [0.31, 0.34, ...],
    "high": [0.18, 0.22, ...]
  },
  "band_energy_means": {
    "bass": 0.55,
    "mid":  0.36,
    "high": 0.21
  },

  "is_beatmatchable": true,
  "is_phrase_aligned": false,

  "is_mixable": true
}
```

Notes:
- All v1 fields remain. `is_mixable` continues to exist (alias of `is_beatmatchable`) for backwards compat with v1 readers.
- `phrase_boundaries_seconds` may be absent or empty if phrase detection didn't pass confidence.
- `band_energy_curves` is always present in v2.

### 1.5 — Analyzer version bump

`__version__` in `dj_analyze/__init__.py`: `0.1.0` → `0.2.0`.

This bump invalidates all existing sidecars (skip-if-fresh check compares `analyzer_version`). One full re-analysis required on first run after upgrade.

## Component 2 — Schema migration (v1 → v2)

`schema/sidecar.schema.json` updates:

```json
{
  "properties": {
    "schema_version": { "const": 2 },

    "phrase_boundaries_seconds": {
      "type": "array",
      "items": { "type": "number", "minimum": 0 }
    },
    "phrase_confidence": { "type": "number", "minimum": 0, "maximum": 1 },

    "band_energy_curves": {
      "type": "object",
      "required": ["bass", "mid", "high"],
      "properties": {
        "bass": { "type": "array", "items": { "type": "number", "minimum": 0, "maximum": 1 } },
        "mid":  { "type": "array", "items": { "type": "number", "minimum": 0, "maximum": 1 } },
        "high": { "type": "array", "items": { "type": "number", "minimum": 0, "maximum": 1 } }
      },
      "additionalProperties": false
    },
    "band_energy_means": {
      "type": "object",
      "required": ["bass", "mid", "high"],
      "properties": {
        "bass": { "type": "number", "minimum": 0, "maximum": 1 },
        "mid":  { "type": "number", "minimum": 0, "maximum": 1 },
        "high": { "type": "number", "minimum": 0, "maximum": 1 }
      },
      "additionalProperties": false
    },

    "is_beatmatchable": { "type": "boolean" },
    "is_phrase_aligned": { "type": "boolean" },

    "mixable_reasons": {
      "type": "array",
      "items": {
        "type": "string",
        "enum": [
          "bpm_low_confidence",
          "bpm_variable",
          "no_clear_downbeat",
          "too_short",
          "key_low_confidence",
          "phrase_low_confidence",
          "bass_only_track"
        ]
      }
    }
  }
}
```

The `required` block under the no-error branch stays the same — new fields are optional so v1 sidecars (without them) still validate. v2 emitters always include `is_beatmatchable`, `is_phrase_aligned`, `band_energy_curves`, `band_energy_means`.

## Component 3 — Engine changes

### 3.1 — `sidecar.rs`: new optional fields + flag fallback

Add to the `Sidecar` struct:

```rust
pub phrase_boundaries_seconds: Option<Vec<f64>>,
pub phrase_confidence: Option<f64>,
pub band_energy_curves: Option<BandCurves>,
pub band_energy_means: Option<BandMeans>,
pub is_beatmatchable: Option<bool>,
pub is_phrase_aligned: Option<bool>,
```

Where `BandCurves` and `BandMeans` are small typed sub-structs with `bass`, `mid`, `high` fields.

Add a method:

```rust
impl Sidecar {
    /// True if this track is safe to time-stretch + EQ-swap crossfade.
    /// v2 sidecars use is_beatmatchable; v1 sidecars fall back to is_mixable.
    pub fn beatmatchable(&self) -> bool {
        self.is_beatmatchable
            .or(self.is_mixable)
            .unwrap_or(false)
    }

    /// True if downbeats are accurate enough to snap to phrase boundaries.
    /// v2 sidecars use is_phrase_aligned; v1 sidecars conservatively return false.
    pub fn phrase_aligned(&self) -> bool {
        self.is_phrase_aligned.unwrap_or(false)
    }
}
```

### 3.2 — `phrase.rs`: prefer-sidecar-if-set

```rust
pub fn phrase_boundaries(sc: &Sidecar, phrase_bars: usize) -> Vec<f64> {
    if let Some(explicit) = &sc.phrase_boundaries_seconds {
        if !explicit.is_empty() {
            return explicit.clone();
        }
    }
    // existing heuristic from earlier work: anchor on intro_end, step downbeats by phrase_bars
    ...
}
```

The `phrase_bars` argument is honored by the heuristic path only; explicit sidecar boundaries are pre-computed at one phrase length (16 bars, set by the analyzer).

### 3.3 — `selector.rs`: transition-aware scoring

The current `score` function in `Selector` returns a weighted sum of three terms. Add a fourth:

```rust
fn score(&self, current: &LoadedTrack, candidate: &LoadedTrack) -> f64 {
    let bpm_score = ...;
    let key_score = ...;
    let energy_score = ...;

    // NEW — look ahead at what the transition would be.
    let transition_score = match plan_transition(&current.sidecar, &candidate.sidecar).length_bars {
        32 => 1.0,
        16 => 0.85,
        8 => 0.6,
        0 => 0.2,
        _ => 0.5,
    };

    bpm_score * 1.0 + key_score * 1.0 + energy_score * 0.5 + transition_score * 1.0
}
```

The new term has weight `1.0` (same as BPM and key). All other selector behavior unchanged: filter chain, top-K weighted-random, recency ring.

Effect: when two candidates score equally on BPM/key/energy, the one that would produce a smoother transition wins. The selector actively avoids hard-cut-prone pairs whenever a better option exists.

### 3.4 — `transition.rs`: relaxed hard-cut trigger

Current rule (top of `pick_length_and_variant`):

```rust
let bars = if is_non_mixable || energy_delta > HARD_CUT_ENERGY_DELTA {  // 0.35
    0
} ...
```

New rule:

```rust
let is_beatmatchable = inc.beatmatchable();
let is_phrase_aligned = inc.phrase_aligned();

let bars = if !is_beatmatchable {
    16  // simple equal-power crossfade, no time-stretch, no EQ swap — never hard cut
} else if energy_delta > HARD_CUT_ENERGY_DELTA {
    0  // hard cut for truly extreme jumps
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

let use_eq_swap = bars >= 8 && is_beatmatchable && is_phrase_aligned;
```

Threshold constants change:
- `HARD_CUT_ENERGY_DELTA`: `0.35` → `0.50`
- `QUICK_CUT_ENERGY_DELTA`: `0.20` (unchanged)
- `LONG_BLEND_BPM_DELTA`: `0.02` (unchanged)
- `LONG_BLEND_ENERGY_DELTA`: `0.10` (unchanged)

`use_eq_swap` gating ensures EQ swap only runs when the analyzer has confidence in phrase alignment. Non-aligned tracks get a clean volume crossfade.

The transition planner also returns the bass-mix hint (Section 3.5).

### 3.5 — `crossfade.rs`: band-aware EQ swap

`mix_crossfade_eq` gains an optional bass-mix hint:

```rust
pub struct BassMixHint {
    pub outgoing_bass_mean: f64,
    pub incoming_bass_mean: f64,
}

pub fn mix_crossfade_eq(
    outgoing: &[f32],
    incoming: &[f32],
    crossfade_frames: usize,
    sample_rate: f64,
    bass_hint: Option<BassMixHint>,
) -> Vec<f32>;
```

Behavior:
- `bass_hint = None` (v1 sidecars or feature opt-out) → current behavior: lowpass sweep across `t ∈ [0.375, 0.5]`.
- `bass_hint = Some(h)` where both means > 0.5 → sweep window shifts earlier: `t ∈ [0.25, 0.4]`.
- `bass_hint = Some(h)` where both means < 0.3 → skip EQ swap entirely; return result of pure `mix_crossfade(...)`.
- Otherwise → current behavior.

The `mixer.rs` call site reads `outgoing.sidecar.band_energy_means.bass` and `incoming.sidecar.band_energy_means.bass` (or `None` if absent) and passes the hint through.

## Component 4 — Testing strategy

### 4.1 — Analyzer

| Module | Tests added |
|---|---|
| `mixability.py` | Track with fallback downbeats + good BPM → `is_beatmatchable=true`, `is_phrase_aligned=false`. Short track (<90s) → both false. Decode error → both false. `is_mixable` alias matches `is_beatmatchable` for both true and false cases. |
| `detect_downbeats.py` | Click-track fixture → librosa phrase detector returns `phrase_confidence > 0.7` + boundaries spaced 16 beats apart. Noise-only fixture → `phrase_confidence < 0.3`, empty boundaries. madmom path (when installed) returns its result preferentially. |
| `detect_bands.py` | Bass-only sine (80 Hz) → `bass_mean > 0.5`, `mid_mean < 0.1`, `high_mean < 0.1`. High-only sine (8 kHz) → mirror image. Combined chord → all three bands present. Output curves sum to roughly the original `detect_energy` total (sanity check). |
| `analyze.py` | Output dict includes all new top-level fields. Output validates against v2 schema. |
| Schema validation | Both v1 and v2 fixture sidecars validate without errors. Removed fields would fail; new fields when absent still validate. |

### 4.2 — Engine

| Module | Tests added |
|---|---|
| `sidecar.rs` | Parses both v1 and v2 fixture JSONs. `beatmatchable()` returns `is_mixable` for v1, `is_beatmatchable` for v2. `phrase_aligned()` returns false for v1, the v2 value when present. |
| `phrase.rs` | Sidecar with `phrase_boundaries_seconds` set → returns those verbatim. Sidecar without → returns the existing heuristic. Empty `phrase_boundaries_seconds` array → falls back to heuristic. |
| `selector.rs` | Two candidates tied on BPM/key/energy where one would produce hard-cut, the other 32-bar blend → the blend wins. Existing top-K weighted-random behavior preserved (top-K still has 5 entries; tiebreak still random). |
| `transition.rs` | Non-beatmatchable incoming → `length_bars=16`, `use_eq_swap=false` (was: `length_bars=0`). Energy delta 0.40 → quick cut (8) not hard cut. Energy delta 0.55 → hard cut. Beatmatchable but not phrase-aligned → `use_eq_swap=false` even for blends. |
| `crossfade.rs::mix_crossfade_eq` | `bass_hint=None` → identical to pre-change behavior. Both > 0.5 → swap window centered at frame index ≈ 25-40% (earlier than the 37.5-50% default). Both < 0.3 → output matches `mix_crossfade(...)` exactly. |

### 4.3 — End-to-end smoke (manual)

1. Stop the running engine.
2. Re-analyze the `data/` library: `dj-analyze data/ analyzed-real/`. Expected: all 24 tracks reprocessed (3-4 min on 2 workers). 0 errors.
3. Inspect 3 sidecars: confirm `schema_version: 2`, `is_beatmatchable: true`, `band_energy_means.bass` present, `phrase_boundaries_seconds` populated for at least some tracks.
4. Rebuild + restart engine with new sidecars.
5. Listen via VLC for 15 minutes. Tail the engine log.

Listening / log checklist:
- `length_bars` values vary across transitions (mix of 16, some 32, some 8).
- `hard cut complete` rare (or absent).
- `use_eq_swap=true` for phrase-aligned transitions; `use_eq_swap=false` for non-aligned (still smooth, just without filter sweep).
- No two basslines fighting during overlaps.

### 4.4 — Not tested

- Subjective DJ-feel quality. Smoke by ear is the gate.
- Threshold values themselves (e.g., the new 0.50 for HARD_CUT_ENERGY_DELTA, the 1.0 weight on transition_score). These are named constants in their modules — easy one-line tuning post-listening.
- Per-band detector accuracy at edge cases (e.g., overlapping bands, transients): the sine-fixture tests cover the main cases.

## Project layout — changes

```
DJ-Engine/
  analyzer/
    src/dj_analyze/
      __init__.py                # version 0.1.0 → 0.2.0
      detect_bands.py            # NEW
      detect_downbeats.py        # MODIFIED — librosa phrase detector
      mixability.py              # MODIFIED — split flags, relaxed thresholds
      analyze.py                 # MODIFIED — orchestrator emits new fields
    tests/
      test_detect_bands.py       # NEW
      test_detect_downbeats.py   # MODIFIED
      test_mixability.py         # MODIFIED
      test_analyze.py            # MODIFIED
      test_schema_validation.py  # MODIFIED — v1 and v2 fixtures
      fixtures/                  # ADDS bass-only and high-only sine fixtures
  schema/
    sidecar.schema.json          # MODIFIED — v2 additions, additionalProperties stays false
  engine/
    src/
      sidecar.rs                 # MODIFIED — new optional fields, beatmatchable() / phrase_aligned() methods
      phrase.rs                  # MODIFIED — prefer sidecar-set boundaries
      selector.rs                # MODIFIED — transition-aware score term
      transition.rs              # MODIFIED — relaxed hard-cut, use_eq_swap gating, returns BassMixHint
      crossfade.rs               # MODIFIED — mix_crossfade_eq signature gains bass_hint
      mixer.rs                   # MODIFIED — wires bass_hint through from sidecars
    tests/
      sidecar_test.rs            # MODIFIED — v1 / v2 cases
      phrase_test.rs             # MODIFIED — sidecar-set boundaries case
      selector_test.rs           # MODIFIED — transition-aware test
      transition_test.rs         # MODIFIED — new threshold cases
      crossfade_eq_test.rs       # MODIFIED — bass_hint cases
```

## Default port

Unchanged. The engine continues to connect to whatever Icecast `ICECAST_URL` points at (currently `localhost:8000` for the user's local setup; the design's reference port stays `27897` for spec consistency).

## Open questions / future work

- **Structural segmentation** (intro / verse / chorus / drop): deferred. If we ever need it, would add `sections: [{ label, start_seconds, end_seconds }]` to the sidecar and a section-aware transition planner.
- **Vocal detection**: deferred until we see two-vocal-on-vocal transitions in practice.
- **Threshold tuning**: the constants in `transition.rs` and the new selector weight (`1.0` on transition_score) are starting values. Likely tune by ear after listening to 30-60 minutes of output.
- **Bands-only mode**: could add a global config to disable EQ swap entirely (just equal-power) for testing. Not in v1.
- **Per-bar phrase confidence**: emit per-boundary confidences instead of a single track-level number, so the engine could prefer specific boundaries during snapping. Adds complexity for unclear gain.
