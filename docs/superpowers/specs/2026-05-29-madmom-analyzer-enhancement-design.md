# Madmom Analyzer Enhancement — Design

**Date:** 2026-05-29
**Status:** Draft, pending user review

## Goal

Replace librosa's fixed-tempo beat tracker with madmom's variable-tempo RNN+DBN tracker as the analyzer's primary beat/downbeat detector, and expose its derived signals (per-bar local BPM, per-beat confidence, per-downbeat confidence) in the sidecar so the engine can compute time-stretch ratios from the actual transition window's local BPM instead of a song-wide average.

## Motivation

Audible symptom: during transitions, beats feel misaligned despite the engine matching global BPMs of the two tracks.

Empirical check on the existing 18-track library showed `local_bpm[intro] == local_bpm[outro] == global_bpm` to three decimal places for 17 of 18 tracks. This is because librosa's `beat_track` enforces a fixed-tempo grid: beats are placed at uniform `60/bpm` intervals by construction, so per-bar BPM derived from them is identically global. Per-bar BPM cannot give a different value than global BPM under the current beat tracker.

Madmom's RNN+DBN beat tracker snaps beats to actual audio onsets, giving genuinely non-uniform beat times, and provides RNN-derived confidence per beat and per downbeat. This unlocks:

- Genuine per-bar BPM (where any internal tempo drift exists in the track).
- Confidence values to gate or down-weight transitions on low-confidence sections.
- Better downbeat detection (RNN trained on musical content vs. librosa's self-similarity-based phrase detector currently used as the fallback path).

The empirical risk is that on a steady-tempo dance/pop library, the per-bar BPM may STILL be uniform — just at slightly different values than librosa produces. In that case the ratio refactor is a no-op, the new-ratio observability logging will show it, and we'll know to look at downbeat detection or non-tempo causes for the audible misalignment.

## Scope

### Delivers

- Madmom becomes a hard-required dependency of the analyzer (no fallback to librosa for beats or downbeats).
- Three new optional sidecar fields: `local_bpm`, `beat_confidence`, `downbeat_confidence`.
- Engine computes the time-stretch ratio from the local BPM of the actual transition window (outgoing's last `length_bars` bars + incoming's first `length_bars` bars), falling back to global BPM if local data is missing.
- Engine logs `ratio_global` and `ratio_local` at every transition arming so the impact is observable in logs even when audio playout delay obscures real-time ear-judgment.
- All 18 tracks re-analyzed; analyzer version bumps 0.4.0 → 0.5.0.

### Out of scope

- Time-varying ratio across the transition window (Q3 option C). Stays as a single ratio per transition.
- Promoting `is_phrase_aligned` from bool to confidence-derived continuous value. `downbeat_confidence` is collected and logged but not yet consumed by scoring.
- Per-bar loudness / LUFS / spectrogram features. Possible Phase F.
- Removing librosa from the dependency tree. Librosa still serves key detection, energy curves, band detection, novelty, phrase SSM, chroma. Only its beat-tracking role is displaced.

## Architecture

### Analyzer pipeline change

Current: `detect_bpm` → `detect_beats(audio, sr, bpm)` → `detect_downbeats(audio, sr, beats)` (three separate audio passes).

New: `detect_beat_grid(audio, sr) -> BeatGrid` produces beats, downbeats, beat confidences, downbeat confidences in a single madmom RNN+DBN pass. The existing `detect_beats` and `detect_downbeats` become thin wrappers that select fields out of the shared result.

`detect_bpm` switches to deriving global BPM from madmom beats (median IBI across the full song) and continues to route the result through the existing `resolve_octave` guard against octave errors.

### Engine ratio computation

A single helper `Sidecar::local_bpm_in_window(start_seconds, end_seconds) -> Option<f64>` averages `local_bpm` entries whose bar overlaps the window, skipping `None`s. Returns `None` when no usable bars are in range, signaling the caller to fall back to `self.bpm`.

The scheduler computes both `ratio_global` (the old formula) and `ratio_local` (the new formula) when arming a transition, stashes the active one onto `TransitionContext::ratio` for recipes to consume, and logs both at the existing `transition arming` info span.

Recipes are refactored to read `ctx.ratio` instead of recomputing it from `outgoing.sidecar.bpm / incoming.sidecar.bpm`. This is a mechanical edit across ~14 recipes.

## Sidecar Schema Additions

All new fields are optional. Older sidecars without them continue to deserialize cleanly. The engine treats their absence as "fall back to global BPM."

### `local_bpm: Vec<Option<f64>>`

- One entry per fully-defined bar (interval between consecutive downbeats).
- Length: `len(downbeats_seconds) - 1` when ≥2 downbeats exist; `0` otherwise.
- Each entry: `Some(bpm)` when the bar has ≥2 beats (median IBI is defined); `None` for underdetermined bars.

### `beat_confidence: Vec<f64>`

- Parallel to `beats_seconds`. Same length.
- Values in `[0, 1]`. The RNN beat activation sampled at each detected beat time.

### `downbeat_confidence: Vec<f64>`

- Parallel to `downbeats_seconds`. Same length.
- Values in `[0, 1]`. The RNN downbeat activation sampled at each detected downbeat time.

### Schema versioning

- `schema_version` stays at 2 (fields are additive and optional).
- `analyzer_version` bumps 0.4.0 → 0.5.0 to mark re-analysis-needed.
- No engine-side schema migration; the engine just sees `Vec` (empty if old) thanks to `#[serde(default)]`.

## Components Touched

### Analyzer

- `analyzer/pyproject.toml` — re-enable `madmom>=0.16.1`, re-pin `numpy>=1.26,<2.0`, add `cython==0.29.36` to build-system requires.
- `analyzer/src/dj_analyze/__init__.py` — `__version__` bump to `0.5.0`.
- `analyzer/src/dj_analyze/detect_beat_grid.py` — NEW. `BeatGrid` dataclass + `detect_beat_grid(audio, sr)` running the madmom RNN+DBN pass once.
- `analyzer/src/dj_analyze/detect_beats.py` — gutted to a thin wrapper returning `grid.beats`; `bpm` parameter dropped (madmom doesn't need it).
- `analyzer/src/dj_analyze/detect_downbeats.py` — `_madmom_downbeats`, librosa-phrase fallback, and `fallback_4_4` paths deleted. `detect_downbeats` becomes a thin wrapper over `detect_beat_grid`; `method` stays `"madmom"` always.
- `analyzer/src/dj_analyze/detect_bpm.py` — derives global BPM from madmom beats (median IBI); keeps the `resolve_octave` guard.
- `analyzer/src/dj_analyze/analyze.py` — single `grid = detect_beat_grid(audio, sr)` call; new `compute_local_bpm` step; populate the three new sidecar fields.
- `analyzer/src/dj_analyze/mixability.py` — audit for madmom-dependent logic that may simplify after hard-required madmom.

### Engine

- `engine/src/sidecar.rs` — three new fields on `Sidecar` with `#[serde(default)]`; new `local_bpm_in_window` helper method.
- `engine/src/recipe.rs` — `TransitionContext` gains `ratio: f64`, `ratio_global: f64`, `outgoing_section_bpm: f64`, `incoming_section_bpm: f64`. Each of the ~14 recipes' `build_script` reads `ctx.ratio` rather than recomputing from globals.
- `engine/src/scheduler.rs` — compute both ratios at arming, set the new context fields, extend the `transition arming` info span with `ratio_global`, `ratio_local`, `bpm_delta_pct`, `outgoing_db_conf_mean`, `incoming_db_conf_mean`.

### Schema docs

- `schema/sidecar.schema.json` — add the three new fields as optional with appropriate type / range constraints.

## Error Handling

### Madmom failure modes

- **Madmom not installed:** analyzer import fails at module load with a clear error. No silent fallback. Re-running pip install with the documented pin combo is the resolution.
- **Madmom fails on a specific audio file:** the exception propagates and the analyzer skips that file (writing a sidecar with `error` set), matching the existing decode-failure behavior. This is the "Q9 option A" choice — loud failure, simpler code.

### Missing fields in engine

- `local_bpm` absent (older sidecar) → `local_bpm_in_window` returns `None` → caller falls back to `self.bpm` → `ratio_local == ratio_global`. No regression from current behavior.
- `beat_confidence` / `downbeat_confidence` absent → consumers (currently just logging) handle empty `Vec` gracefully.

### Length-invariant violations

- Engine logs a warning if `beat_confidence.len() != beats_seconds.len()` or `downbeat_confidence.len() != downbeats_seconds.len()` and treats the confidence array as absent. Defensive; should never fire on analyzer-emitted sidecars.

## Testing

### Analyzer unit tests

- `tests/test_detect_beat_grid.py` — fixture audio in, verify all four arrays populated and length-consistent.
- Confidence values in `[0, 1]`.
- `test_compute_local_bpm` — handcraft beats + downbeats, verify medians.
- `test_compute_local_bpm_underdetermined_bar` — bar with fewer than 2 beats yields `None`.
- `test_analyze_track_emits_new_fields` — end-to-end smoke test; sidecar dict has the three new keys with correct shapes.

### Analyzer tests to update or delete

- `test_detect_downbeats.py` — fallback-path tests become dead. Either delete or repurpose to verify error propagation when madmom fails.
- `test_detect_beats.py` — rewrite against madmom outputs.
- `test_mixability.py` — verify after `mixability.py` audit.

### Engine unit tests

- `test_local_bpm_in_window` — uniform values, partial `None` entries skipped, window outside all bars → `None`, empty array → `None`.
- `test_transition_context_ratio_uses_local` — valid `local_bpm` arrays produce ratio reflecting local BPMs.
- `test_transition_context_ratio_falls_back_to_global` — empty `local_bpm` → context.ratio = global ratio.
- `test_scheduler_logs_both_ratios` — verify `transition arming` info span carries `ratio_global` and `ratio_local`.

## Empirical Verification Post-Implementation

1. **Sidecar diff** — for each of the 18 tracks, compare old (v0.4.0) vs new (v0.5.0) `downbeats_seconds`. Per-track tabulate: max disagreement, count of downbeats shifted >100ms. Isolates whether madmom moves downbeats at all on this library.
2. **Library local_bpm scan** — for each track, compute the local BPM of the first 16 bars vs the last 16 bars vs the global BPM. Report max delta per track. If max delta is still 0.000 across the library, the field is dead weight on this library — but the cost was zero and it carries value once non-steady-tempo tracks arrive. (Script: ~30 lines of Python reading the new sidecar JSONs; left to the implementer to write or run ad-hoc.)
3. **Live-listen check** — restart the engine. Each transition logs `ratio_global=X ratio_local=Y bpm_delta_pct=Z`. If consistently equal, the ratio refactor isn't moving the needle for this library, and we look elsewhere for "beats not aligned" cause.
4. **Downbeat-confidence audit** — log distribution per transition. Tracks with low confidence are the candidates for the audible misalignment complaint. Possible follow-up: gate phrase_aligned on a threshold.

## Migration

1. Install madmom in `analyzer/.venv` (Python 3.10):
   - `pip install "numpy>=1.26,<2.0" "cython==0.29.36"`
   - `pip install "madmom>=0.16.1" --no-build-isolation`
   - On Windows, MSVC Build Tools must be present (already present on this machine via the torch/demucs install path).
2. Snapshot existing sidecars: `cp -r analyzed-real analyzed-real.v0.4.0` (cheap rollback).
3. Re-run analysis on all 18 tracks. Confirm during implementation whether `dj-analyze` auto-re-runs on `analyzer_version` mismatch or needs `--force`; document the actual command used.
4. Rebuild engine. Stop the running `dj-engine.exe` first (binary lock per existing memory).
5. Restart engine; verify the new log fields appear at the first transition arming.

## Rollback

- Restore `analyzed-real/` from the v0.4.0 snapshot.
- Engine continues to work against old sidecars (the new fields are optional; ratios fall back to global).
- Optional: revert pyproject.toml + analyzer code via git.

No data is destroyed by this change. The schema is additive.

## Open Items For Implementation Plan

- Verify whether `dj-analyze` auto-re-analyzes on `analyzer_version` mismatch or needs an explicit force flag.
- Audit `mixability.py`'s madmom usage to decide whether logic simplifies under hard-required madmom.
- Confirm exact pin combo works on the analyzer venv with a one-fixture smoke test before committing to library-wide re-analysis.
