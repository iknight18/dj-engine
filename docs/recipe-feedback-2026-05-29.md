# Recipe Feedback — 2026-05-29 (madmom analyzer 0.5.0 build)

Each recipe is force-tested via `--force-recipe <name>` and evaluated by ear. Notes are captured live during listening. Engine streams to `http://localhost:8000/dj.mp3`; expect ~5–15s of playout delay between log and ear.

## Listening Context

- Sidecars: `analyzed-real/` (analyzer 0.5.0, madmom-based)
- Engine: `engine/target/release/dj-engine.exe` (commits up to `f2e69c5`)
- Library: 18 tracks at `data/`
- Listener: solo focused indoor listening (not dance floor)

## Recipes

| # | Recipe (CLI flag) | Stems? | Status | Decision |
|---|---|---|---|---|
| 1 | `equal-power` | No | tested | deprioritize, keep as final fallback |
| 2 | `hard-cut` | No | tested | keep for genre mismatch; design "punctuated hard-cut" variant with outgoing FX |
| 3 | `bass-swap` | No | tested (historical) | refactor — drums also overlap during EQ kill |
| 4 | `filter-fade` | No | tested | promote — preferred over equal-power |
| 5 | `filter-kill` | No | tested | keep for energy-down; not as preferred as filter-fade |
| 6 | `echo-out` | No | pending | — |
| 7 | `loop-out` | No | pending | — |
| 8 | `three-band-swap` | No | pending | — |
| 9 | `echo-throw` | No | pending | — |
| 10 | `highs-first` | No | pending | — |
| 11 | `drop` | No | pending | — |
| 12 | `breakdown-bridge` | No | pending | — |
| 13 | `double-drop` | No | pending | — |
| 14 | `vocal-overlay` | Yes | pending | — |
| 15 | `bass-swap-stem-aware` | Yes | pending | — |
| 16 | `breakdown-into-drop` | Yes | pending | — |

---

## Per-recipe notes

Format: one block per recipe. Filled in during testing.

### 1. equal-power

**What it does:** Equal-power crossfade across the full window. Both faders use cosine curves so power stays constant. No EQ, no filter, no effects.
**Tested on transitions:** 001→013 (L=8, ratio_local=1.0035, ratio_global=1.000, duration 15.3s)
**Feedback:** "Seemed radio-like. A basic transition." Sounds like FM radio crossfade — smooth, generic, nothing distinctive. No clash heard at the midpoint but also no character.
**Decision:** Deprioritize. Keep as final fallback when no other recipe scores well.

---

### 2. hard-cut

**What it does:** Instantaneous Step crossfade at the downbeat. Outgoing fader 1→0 Step, incoming fader 0→1 Step at the same moment. Length 0 bars.
**Tested on transitions:** 001→015 (instant, duration 0s, bpm_ratio 1.0012)
**Feedback:** Useful for **genre mismatch** — when the two songs don't blend well, a clean cut is the right move. But a pure hard-cut is harsh. Should be softened with an outgoing-only effect first (echo out, filter sweep, tape stop, etc.) to signal "this song is ending" before the cut lands.
**Decision:** Keep hard-cut for genre-mismatch cases. **Design follow-up:** a "punctuated hard-cut" variant that applies an outgoing effect over the last few bars, then hard-cuts to incoming.

---

### 3. bass-swap

**What it does:** Equal-power crossfade with EQ swap. Outgoing held at fader=1 until L/2, incoming enters at bar 0 with EqLow killed (-40dB). Both EqLow Step-swap at L/2 (outgoing→killed, incoming→released). Outgoing equal-power-fades out over [L/2, L].
**Tested on transitions:** Historical feedback from earlier in the day on multiple transitions.
**Feedback (historical):**
- Step-on-downbeat refactor: "not so bad but didn't sound cool." Incoming's vocals layered over outgoing's bass during [0, L/2] — the vocals don't match outgoing's key.
- "Two melodies is ambiguous" — incoming's mids/highs/vocals fighting outgoing's mids/highs/vocals during the layering window.
- EQ kill only removes low band; drums (snares, hats, percussion) overlap on BOTH decks during the swap window. Cluttered.
**Decision:** Refactor required. Either:
  (a) Replace with stem-aware version that mutes outgoing's drums + bass and incoming's vocals + other during the layering window (the gesture we sketched as RhythmHandoff but with a single-downbeat swap instead of phrase handoff), OR
  (b) Drop the layering window entirely — just do an EQ swap on a single downbeat with no overlap.

---

### 4. filter-fade

**What it does:** Equal-power crossfade with incoming high-pass filter. Outgoing fades out equal-power. Incoming fader fades in equal-power. Incoming's one-knob filter sweeps from +1.0 (full high-pass, treble only) to 0.0 (bypass) across the window. Outgoing untouched.
**Tested on transitions:** 001→011 (L=8, ratio_local=1.0093, duration 15.3s)
**Feedback:** "Better than the normal crossfade." User prefers this to equal-power and wants it to be the default crossfade if possible.
**Decision:** Promote — raise score floor so filter-fade is the default plain-crossfade gesture instead of equal-power. Consider deprecating equal-power entirely or relegating it to the no-stems / no-filter-CPU fallback. **Caveat:** filter-fade has a "build" character (rising treble→bass reveal). Appropriate for energy-up and energy-flat. For energy-DOWN transitions, the build feel may be wrong — keep equal-power as the fallback for those.

---

### 5. filter-kill

**What it does:** Outgoing high-passes itself OUT (filter sweep from 0 → +1 over the first 60% of the window). Outgoing fader equal-power fades out over the full window. Incoming fader equal-power fades in over the full window (no filter on incoming).
**Tested on transitions:** Forced session 2026-05-29 14:xx (filter-kill build)
**Feedback:** "Not so good but yeah I'm liking filter-based transitions for now." Has its use cases — keep it but not as a primary pick.
**Decision:** Keep for energy-down transitions (where outgoing thinning out musically makes sense). Lower priority than filter-fade. General signal: filter-based gestures are working well for this listener.

---

### 6. echo-out

**What it does:** Equal-power crossfade. Outgoing's echo wet-mix ramps 0→0.55 over the first half of the window. Beat-length delay, feedback 0.35. The echo tail rings under the incoming as outgoing's dry signal closes.
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 7. loop-out

**What it does:** Equal-power crossfade with outgoing in loop mode. LoopGate Step ON at bar 0 (locks outgoing's last 4 bars into a loop). LoopGate OFF at end. Outgoing rides the locked loop while incoming fades in over it.
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 8. three-band-swap

**What it does:** Equal-power crossfade with staged EQ swaps. Incoming highs ramp in over [0, L/8]. Mids cross at L/2: outgoing.mid Step→-40dB at L/2, incoming.mid was Step→-40dB at bar 0, then Step→0dB at L/2+1. Bass swap single Step at 7L/8 (outgoing.low→-40dB, incoming.low→0dB).
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 9. echo-throw

**What it does:** Equal-power crossfade with a 1-bar echo flourish on the outgoing deck. Triangle wet envelope over the last 2 bars: 0→0.6 over 0.25 bar, then 0.6→0 over 0.75 bar. Beat-length delay, feedback 0.75 (high).
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 10. highs-first

**What it does:** Equal-power crossfade with incoming entering treble-only. Incoming fader rises fast (equal-power over [0, L/8]). Incoming starts with EqLow=-40dB and EqMid=-40dB. Mids Step→0 at L/4. Bass Step swap at 3L/4. Outgoing equal-power fades out over the full window.
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 11. drop

**What it does:** Incoming plays for 4 bars with high-pass filter (treble-only riser, knob 0.85→0.55). At bar 4, both faders hard-cut: outgoing→0, incoming→full and filter→0 (bypass). Length fixed at 4 bars.
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 12. breakdown-bridge

**What it does:** Outgoing's mids + bass linearly ramp to -40dB over bars [0, 3]. At bar 4: outgoing fader Step→0, incoming fader Step→1. Outgoing's highs ring through the breakdown alone. Length fixed at 4 bars.
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 13. double-drop

**What it does:** Outgoing fader Linear 1→0 over [0, 0.5]. Both decks silent for 0.5 bar (anticipation gap). At bar 1: both faders Step→1 (both at full volume together). Both play 1 bar layered. At bar 2: outgoing Step→0. Length fixed at 2 bars.
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 14. vocal-overlay

**What it does:** Stems-aware, 16 bars. Outgoing equal-power fades out over [12, 16]. Incoming mixed fader Step→1 at bar 0 (all stems muted to 0). Incoming vocals stem Linear 0→1 over [0, 4]. Incoming drums/bass/other stems Linear 0→1 over [12, 16].
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 15. bass-swap-stem-aware

**What it does:** Stems-aware, 16 bars. Equal-power crossfade on mixed faders. Incoming bass stem muted from bar 0. At the bass-density-valley bar (default 8, picked from incoming.bass_density), outgoing bass stem killed AND incoming bass stem released simultaneously. Other stems crossfade naturally via mixed fader.
**Tested on transitions:**
**Feedback:**
**Decision:**

---

### 16. breakdown-into-drop

**What it does:** Stems-aware, 8 bars. Outgoing drums stem Linear 1→0 over [0, drop_bar]. Outgoing mixed fader Step→0 at drop_bar. Incoming starts silent (fader=0); incoming fader Step→1 at drop_bar. drop_bar is picked from incoming.drum_density (first bar ≥0.85; fallback 4).
**Tested on transitions:**
**Feedback:**
**Decision:**

---

## Cross-cutting observations

(Things that span recipes — beat alignment, loudness, key clashes, etc.)

- **Filter-based recipes are landing well.** filter-fade preferred over plain equal-power. filter-kill acceptable as energy-down variant. Suggests we should expand the filter-based recipe family.
- **Layering with vocals/mids on top causes the "ambiguous" problem.** bass-swap, vocal-overlay, etc. all suffer from this. Stems-aware drums+bass swaps avoid the issue.
- **Madmom local section BPMs differ from globals by 0.1-0.4% on most transitions.** Real measurable signal, though small. Will continue showing up in `bpm_delta_pct` log field.

## Session pause notes (2026-05-29)

Paused after recipe #5 of 16. Tested so far: equal-power, hard-cut, bass-swap (historical), filter-fade, filter-kill. Remaining: echo-out, loop-out, three-band-swap, echo-throw, highs-first, drop, breakdown-bridge, double-drop, vocal-overlay, bass-swap-stem-aware, breakdown-into-drop.

Engine is stopped. Resume by force-restarting with `--force-recipe echo-out` (next in the list).

## Decisions summary

(After all recipes tested, what stays / what goes / what gets refactored.)

-
