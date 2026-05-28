# Analyzer BPM Octave-Error Correction

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** standalone (analyzer-only fix)

## Summary

`detect_bpm.py` uses `librosa.beat.tempo` with a median aggregate to estimate a track's global BPM. That estimator is octave-ambiguous: on a track that's genuinely 76 BPM, the global median locks onto 152 (double-time), and the per-window confidence check then reports high confidence because every window agrees on 152. Confidence measures *consistency*, not *correctness*. Add a second, independent BPM estimate from `librosa.beat.beat_track` (median inter-beat interval), compare it to the global estimate, and when they disagree by a factor of ~2, octave-resolve by preferring the candidate inside the perceptual range [70, 180] BPM.

## Motivation

Concrete observations from the engine's live run on the user's 24-track library:

- Five tracks at exactly 139.6748310810811 BPM (`60 * 44100 / (512 * 37)` — the quantized output of librosa with `sr=44100, hop=512`).
- Four tracks at exactly 151.99908088235293 BPM.
- One pair selected for transition arrived at a sync ratio of 1.5588 (152 → 97.5 BPM), audibly chipmunked the incoming. The 97.5-BPM track had `bpm_confidence=0.5` — the analyzer was already suspicious, but the engine had no signal that octave doubling was the cause.

The fix lives in the analyzer because the engine cannot recover BPM information that was discarded upstream. With a corrected BPM, the engine's existing low-confidence gate (added in the prior fix batch) handles the rest.

## Goals

- In `detect_bpm.py`, add a beat-position cross-check that detects octave errors in the global BPM estimate.
- When the two estimates disagree by ~2× or ~0.5×, pick the candidate that falls inside the perceptual BPM range [70, 180]. If both do, prefer the one closer to 120 BPM (the perceptual default).
- The function still returns `(bpm: float, confidence: float)` — no signature change, no sidecar schema change.
- The confidence calculation already in place stays (vote-based agreement after octave normalization). The corrected BPM is the *new* target the windows vote on.

## Non-Goals

- **No sidecar schema change.** No new `bpm_alt` / `bpm_candidates` field. (That would be enhancement #2 if needed later.)
- **No manual override CSV.** (Enhancement #3 if needed later.)
- **No swap of `librosa` for `madmom` / `aubio` / other libraries.** The user's Python 3.13 environment doesn't support madmom.
- **No change to `detect_downbeats`, `detect_beats`, or any other detector.** Only `detect_bpm.py`.
- **No automatic re-run of the analyzer.** Re-analyzing the user's library to refresh sidecars is a manual step the user invokes after this lands.
- **No tempo-curve detection** (variable-BPM tracks). The estimator still reports a single global value.

## Algorithm

Pseudocode of the new `detect_bpm`:

```
def detect_bpm(audio, sr):
    onset_env = librosa.onset.onset_strength(audio, sr)

    # Primary estimate (unchanged).
    global_tempo = librosa.beat.tempo(onset_env, sr, aggregate=np.median)
    primary = float(global_tempo[0])

    # Secondary estimate from beat positions.
    _, beat_frames = librosa.beat.beat_track(onset_envelope=onset_env, sr=sr)
    secondary = bpm_from_beats(beat_frames, sr)  # 60.0 / median IBI; None if too few beats

    # Resolve octave by combining both estimates.
    resolved = resolve_octave(primary, secondary)

    confidence = _tempo_stability(audio, sr, resolved)
    return resolved, confidence


def bpm_from_beats(beat_frames, sr):
    if beat_frames is None or len(beat_frames) < 2:
        return None
    beat_times = librosa.frames_to_time(beat_frames, sr=sr)
    intervals = np.diff(beat_times)
    if len(intervals) == 0:
        return None
    median_ibi = float(np.median(intervals))
    if median_ibi <= 0:
        return None
    return 60.0 / median_ibi


PERCEPTUAL_LOW = 70.0
PERCEPTUAL_HIGH = 180.0
PERCEPTUAL_DEFAULT = 120.0


def resolve_octave(primary, secondary):
    # If beat_track gave us nothing, trust the primary.
    if secondary is None:
        return _snap_to_perceptual(primary)

    # Generate candidate octaves for each estimate.
    primary_candidates = (primary / 2.0, primary, primary * 2.0)
    secondary_candidates = (secondary / 2.0, secondary, secondary * 2.0)

    # Each candidate inside [70, 180] is "perceptually plausible".
    primary_in_range = [c for c in primary_candidates if PERCEPTUAL_LOW <= c <= PERCEPTUAL_HIGH]
    secondary_in_range = [c for c in secondary_candidates if PERCEPTUAL_LOW <= c <= PERCEPTUAL_HIGH]

    # Pair every plausible primary candidate with every plausible secondary candidate,
    # find the pair with smallest relative difference. If that pair agrees within 6%,
    # the agreed-on value is the answer.
    AGREEMENT_TOLERANCE = 0.06
    best_pair = None
    best_rel_diff = float("inf")
    for p in primary_in_range:
        for s in secondary_in_range:
            rel = abs(p - s) / max(p, s, 1e-9)
            if rel < best_rel_diff:
                best_rel_diff = rel
                best_pair = (p, s)

    if best_pair is not None and best_rel_diff <= AGREEMENT_TOLERANCE:
        # Agreed: pick the average of the agreed octaves.
        return float((best_pair[0] + best_pair[1]) / 2.0)

    # No agreement: pick the in-range candidate (across both estimates)
    # closest to the perceptual default of 120.
    all_in_range = primary_in_range + secondary_in_range
    if all_in_range:
        return float(min(all_in_range, key=lambda c: abs(c - PERCEPTUAL_DEFAULT)))

    # Nothing in perceptual range — return primary as-is, snapped to range if any
    # nearby octave gets there; otherwise just give up and report the raw primary.
    return _snap_to_perceptual(primary)


def _snap_to_perceptual(bpm):
    """If `bpm` is outside [70, 180] but an integer octave shift would land it inside,
    apply that shift. Otherwise return `bpm` unchanged."""
    if bpm <= 0:
        return bpm
    while bpm < PERCEPTUAL_LOW and bpm > 0:
        bpm *= 2.0
        if bpm > PERCEPTUAL_HIGH * 2.0:  # safety: never loop forever
            break
    while bpm > PERCEPTUAL_HIGH:
        bpm /= 2.0
        if bpm < PERCEPTUAL_LOW / 2.0:
            break
    return float(bpm)
```

## Edge cases and tradeoffs

- **`librosa.beat.beat_track` returns 0 or 1 beats.** `bpm_from_beats` returns `None`; resolver falls back to the primary estimate (snapped to perceptual range).
- **Tracks genuinely outside [70, 180].** A 60 BPM ballad will get its primary octave-shifted to 120 if `_snap_to_perceptual` is reached, which is *wrong* for that track. Mitigation: the AGREEMENT_TOLERANCE check happens first. If the primary is 60 and `beat_track` also says 60, both candidates' "×2 octaves" land at 120; both estimates' "×1" land at 60 (outside range, filtered out). The pair (120, 120) agrees, so the function returns 120 — still wrong. This is the cost of optimizing for dance music. Manual override (future enhancement) is the escape hatch.
- **AGREEMENT_TOLERANCE of 6%.** Matches the `_VOTE_TOLERANCE` constant already in `_tempo_stability` (`±6%` for window votes). Keeps the two checks consistent.
- **Choice of perceptual default 120.** Matches common DJ-software defaults. A 95 BPM and a 145 BPM candidate are both "in range" and equidistant from 120 (25 BPM away); the tiebreaker is arbitrary but harmless because both are musically valid.
- **What if both estimates agree but on the wrong octave?** e.g., librosa returns 152 on a 76 BPM track AND `beat_track` also locks onto 152. Both candidates fall in perceptual range, agree at 152, and we return 152. No automatic fix in that case — this is the failure mode the manual override CSV (future enhancement) handles. But the current `confidence` calculation is unaffected and may still be lower, signaling to the engine that the BPM is suspect.

## Components and file layout

```
analyzer/src/dj_analyze/
  detect_bpm.py            ← all changes here
                              + bpm_from_beats() helper
                              + resolve_octave() function
                              + _snap_to_perceptual() helper
                              + PERCEPTUAL_LOW / HIGH / DEFAULT constants
                              + detect_bpm() body extended with beat_track call

analyzer/tests/
  test_detect_bpm.py        ← extend (or create if absent) with octave-resolution tests
                              against synthesized click tracks
```

Other files (`sidecar.py`, `pipeline.py`, `__main__.py`) are unchanged. `detect_bpm` still returns `(bpm: float, confidence: float)`.

## Testing strategy

Add unit tests under `analyzer/tests/test_detect_bpm.py`:

1. **`bpm_from_beats` math.** Hand-construct a `beat_frames` array at known BPM (e.g., beats at frames `[0, 22050, 44100, 66150]` at sr=44100 → 60/0.5=120 BPM). Assert returned value within 0.5 BPM.
2. **`bpm_from_beats` handles empty/single beat.** Returns `None`.
3. **`resolve_octave` exact agreement.** primary=120, secondary=120 → returns 120.
4. **`resolve_octave` exact octave doubling.** primary=152, secondary=76 → returns 76 (in-range secondary wins; if both in range, agreement check should match primary÷2=76 with secondary=76).
5. **`resolve_octave` exact octave halving.** primary=60, secondary=120 → returns 120.
6. **`resolve_octave` disagreement, prefer closer-to-120.** primary=95, secondary=145 → returns 95 (closer to 120 by 25 vs 25; tie broken by `min(...)` — document the deterministic tiebreaker).
7. **`resolve_octave` secondary None.** primary=140, secondary=None → returns 140.
8. **`_snap_to_perceptual` below range.** 40 BPM → 80 BPM (one ×2).
9. **`_snap_to_perceptual` above range.** 240 BPM → 120 BPM (one ÷2).
10. **`_snap_to_perceptual` in range.** 100 BPM → 100 BPM.

End-to-end test (in `test_detect_bpm.py` if the analyzer's test infrastructure already loads audio, otherwise skip this layer):
- Synthesize a 5-second click track at 76 BPM using `librosa.clicks` or numpy directly.
- Call `detect_bpm(audio, sr=22050)`.
- Assert the returned BPM is within 6% of 76 (not 152).
- Repeat for a 152 BPM click track — assert returned BPM is within 6% of 152.

If the analyzer doesn't currently have audio-based tests, the unit tests above are sufficient; add the audio test only if the test infrastructure already supports it.

## Risks

- **The fix can regress correct cases.** Some librosa primary estimates are right; if `beat_track` is noisy and reports a wildly different value, `resolve_octave` might still pick the correct one (because the agreement check requires both candidates to be in range AND match within 6%). The risk is the "disagreement → pick closer-to-120" branch: a primary of 140 with a secondary of 90 (no octave relationship) would return 90 (closer to 120 by 30 vs 20 — wait, 140 is also 20 away). The tiebreaker shrugs and picks one. Acceptable: confidence will be lowered by `_tempo_stability` since 90% of windows likely won't vote for 90 if the track is genuinely 140.
- **Performance.** `librosa.beat.beat_track` runs another pass over the onset envelope. For a 4-minute track at sr=22050 with hop=512, that's ~10ms extra. Negligible.
- **Re-analyzing the user's library** will rewrite all 24 sidecars and change some BPM values. The engine should be restarted afterward so it reads the new sidecars. No incremental-update logic needed — the analyzer's incremental check (`source_mtime`, `source_size_bytes`) already handles it: bumping the analyzer version invalidates all sidecars on next run. Either set `analyzer_version` to a new value or delete `analyzed-real/` and re-run.

## Open questions

- After this lands, if a track is *still* mis-detected (e.g., the track is genuinely 95 BPM but `librosa.beat.tempo` returns 145 and `beat_track` agrees), the manual override CSV (enhancement #3) is the next escape hatch. Out of scope for this spec.

## Roadmap (after this sub-project)

- Enhancement #2 (sidecar schema bump: `bpm_candidates`) — only if listening reveals the engine could benefit from knowing the alternative octave.
- Enhancement #3 (manual override CSV) — if specific tracks are reliably wrong.
- Variable-BPM tracks (tempo curve in the sidecar) — orthogonal future feature.
