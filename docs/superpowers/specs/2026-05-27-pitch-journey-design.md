# Pitch Journey — Design

**Status:** Approved 2026-05-27.

## Problem

When a crossfade finishes today, the incoming track is still atempo-stretched to the outgoing's BPM (we keep the stretched buffer running for a 1-bar post-roll). At the moment we swap to the long-lived "playing" decoder for the new track, the audio snaps from outgoing-BPM to the incoming track's native BPM in one sample. On BPM-mismatched pairs (120 → 128 is ~7%) this snap is audible and feels mechanical — the opposite of how a human DJ rides the pitch fader gently down to native after a transition.

## Goal

Insert a gradual atempo ramp between the end of the crossfade and the handoff to the native-tempo decoder. The incoming track lands musically aligned with the outgoing's groove, then drifts to its true tempo over the next 16 bars, so the listener never hears a discontinuity.

## Approach: chunked atempo ramp

After the crossfade region (and the existing 1-bar post-roll) ends, we keep decoding the incoming track via short-lived ffmpeg+atempo subprocesses, splitting the ramp duration into K equal chunks. Each chunk uses its own atempo ratio interpolated between the crossfade's ratio (`outgoing_bpm / incoming_bpm`) and 1.0. The new long-lived "playing" decoder is spawned to start at the end of the ramp instead of the end of the post-roll.

**Why chunked, not continuous:**
- ffmpeg's `atempo` filter does not expose its `tempo` argument as runtime-mutable via `sendcmd` in standard builds — fragile cross-platform.
- The `rubberband` filter supports time-varying tempo with better quality, but is not built into every ffmpeg binary (especially Windows builds via winget). Save for a later "v2".
- 4 chunks × 4 bars at typical BPMs is ~32 seconds total. Each chunk seam is one sample-boundary where the atempo ratio jumps a small delta (~0.5–2%). WSOLA's window is short enough that the perceived artifact at a 1% ratio step is at or below the audible floor.

**Chunk math.** Output duration per chunk: `T = ramp_seconds / K`. For chunk `i ∈ [0, K)`:
- Ratio: `r_i = lerp(start_ratio, 1.0, (i + 0.5) / K)` (midpoint of the chunk's slice of the ramp — smoother than endpoint interpolation).
- Input-time start for chunk 0: end of the existing post-roll in track time.
- Input-time start for chunk `i+1`: `start_i + T * r_i` (atempo at ratio r consumes `T*r` seconds of input to produce `T` seconds of output).

This keeps the chunks sample-continuous in track time, so each chunk's accurate seek (`-ss after -i`) lands on the next sample of the source.

## Scope

**In scope:**
- New `decode_stretched_ramp` helper in `engine/src/ffmpeg/mixwindow.rs` that takes start_ratio, end_ratio, ramp_seconds, chunk_bars, and source-time start; produces a single concatenated PCM buffer.
- Extension of `execute_crossfade` in `engine/src/mixer.rs` to push the ramp buffer between the existing post-roll and the new-decoder swap, and to spawn the new decoder at `ramp_end` instead of `stretched_end`.
- Default constants: `RAMP_BARS = 16`, `RAMP_CHUNK_BARS = 4` → 4 chunks. Defined as `const` in `mixer.rs`. Configurable later if listening reveals issues.
- Skip ramp when `|start_ratio − 1.0| < 0.005` (same-as-existing identity threshold).
- Skip ramp on hard cuts (`length_bars == 0`) — those paths don't run `execute_crossfade`.

**Out of scope:**
- Outgoing-track pitch ramp before the crossfade (DJs sometimes pitch *up* incoming before the transition; we keep current convention of incoming pitched down to outgoing).
- Non-linear ramp shapes (exponential, logarithmic). Linear midpoint first; revisit only if a real listen shows audible "tail" issues.
- `rubberband` migration.
- Plumbing `RAMP_BARS` through the config file. Hardcode now, tune later.

## Edge cases

1. **`length_bars == 0` (hard cut):** Never runs `execute_crossfade`. No code path change required.
2. **`|start_ratio − 1.0| < 0.005`:** Skip the ramp call entirely; spawn new decoder at the current `stretched_end` (today's behavior).
3. **Source `cue_out` falls inside the ramp region:** If the incoming track's `cue_out_seconds` is shorter than `ramp_end`, we'd be asking ffmpeg to decode past the cue. Behavior: clamp `ramp_end` to `cue_out` and shorten the ramp proportionally; if the clamped ramp is < 1 bar, skip the ramp and just spawn the new decoder at `stretched_end` as today. This case is rare (intros are usually >16 bars) but we degrade gracefully rather than failing.
4. **`incoming` not beatmatchable:** Selector already routes these to length-16 non-EQ crossfades, but `ratio` is still computed from BPMs. If `bpm_confidence` is low the ratio may be nonsense — but the ramp can only help (or be a no-op) since we end at 1.0 regardless. Leave it on.

## Risks

- **Chunk boundary clicks.** The 0.5–2% atempo step per boundary could in theory produce a faint pop. Mitigation: midpoint interpolation reduces the per-chunk step, and chunks land on bar boundaries which musically mask anything subtle. If it's audible on listen, fall back to fewer/larger chunks (2 × 8 bars).
- **Total ramp duration too long.** 16 bars at 128 BPM is 30 seconds. That's a long time during which the listener is hearing a continuously drifting pitch. If it feels seasick, shorten to 8 bars; if it feels too abrupt, lengthen to 32. Tunable via the const.
- **Selector-driven repeated re-stretching cost.** Each chunk is its own ffmpeg subprocess (4 × ~150ms startup = ~600ms total). All chunks decode in parallel (we already pre-spawn the new playing decoder concurrently), so wall-clock cost is just the longest chunk's decode, dominated by I/O. No new latency on the streaming path.

## Test plan

- **Listen test (primary):** Restart engine, queue transitions with known BPM mismatches (≥5%). Listen for: (a) no audible snap at the post-roll→ramp seam, (b) no audible clicks at chunk boundaries, (c) the incoming track audibly relaxing toward its native tempo over the ramp.
- **Unit tests for `decode_stretched_ramp`:** Output sample count matches expected (ramp_seconds × SR × 2). With `start_ratio = 1.0`, output is sample-identical to a single `decode_stretched(start, start+ramp_seconds, 1.0)` call.
- **Unit test for input-time advancement:** Given `start_ratio = 1.05`, `end_ratio = 1.0`, `K = 4`, the cumulative input time consumed across all chunks equals `T * sum(r_i)`.
- **Existing crossfade tests still pass** — no change to `crossfade.rs`.

## Out

The change ships as one PR-equivalent commit. Listening determines whether the default 16-bar ramp is the right size before committing to further tuning work.
