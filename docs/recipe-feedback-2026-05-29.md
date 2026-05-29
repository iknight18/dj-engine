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
| 6 | `echo-out` | No | tested | keep; consider bumping wet/feedback for more audible character |
| 7 | `loop-out` | No | tested | keep + enhance with filter sweep on loop + incoming rhythm leak-in |
| 8 | `three-band-swap` | No | tested | drop or radically rework — bad on multiple sessions |
| 9 | `echo-throw` | No | tested | drop — echo effect not audibly working |
| 10 | `highs-first` | No | tested | drop — unnatural |
| 11 | `drop` | No | tested | keep, trust the picker — only fires on right energy/phrase context |
| 12 | `breakdown-bridge` | No | tested | drop |
| 13 | `double-drop` | No | tested | drop entirely |
| 14 | `vocal-overlay` | Yes | tested (historical) | drop — fundamental "layering ambiguity" gesture rejected |
| 15 | `bass-swap-stem-aware` | Yes | tested (historical) | refactor to drums+bass swap (RhythmSwap) |
| 16 | `breakdown-into-drop` | Yes | tested | drop — outgoing-without-drums sounds bad |

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
**Tested on transitions:** 001→006 (L=8, duration 15.3s)
**Feedback:** "Not very bad. Actually didn't feel too much of an echo. Not so bad." — echo character was too subtle to register as a distinctive gesture. Transition felt like a regular crossfade with a faint hint of echo.
**Decision:** Keep. **Tunable:** consider bumping max wet from 0.55 to 0.65–0.75 and/or feedback from 0.35 to 0.45 to make the echo more audible. Not a hero gesture as-is.

---

### 7. loop-out

**What it does:** Equal-power crossfade with outgoing in loop mode. LoopGate Step ON at bar 0 (locks outgoing's last 4 bars into a loop). LoopGate OFF at end. Outgoing rides the locked loop while incoming fades in over it.
**Tested on transitions:** 001→006 (L=8, ~15s), 006→002 (L=16, ~30s with 4 loop cycles)
**Feedback:** "Liked the loop-out, but we needed to add some filters maybe and maybe start hearing the incoming bass and rhythm or something like that. Mainly not bad."
**Decision:** **Loop = good primitive, equal-power crossfade with incoming = bad combination.** Keep the looping mechanism; replace the fade-portion of the recipe. User: "Loop is nice, we can reuse the looping method, but the fade was a little bit bad. But I liked the looping effect." **Enhancements to consider (all replace the equal-power fade-in with something better):**
  - **Filter sweep on the looping outgoing** (each cycle gets thinner). Outgoing eventually disappears as shimmer.
  - **Layer incoming's drums+bass stems UNDER the loop** during the second half. Listener hears outgoing's loop thinning while incoming's rhythm builds.
  - **filter-fade on incoming** instead of equal-power-fade. Incoming reveals through the spectrum while outgoing loops.
  - Combined: the "loop + rhythm under" gesture as the spiritual cousin of RhythmHandoff with the loop as the bridge.

---

### 8. three-band-swap

**What it does:** Equal-power crossfade with staged EQ swaps. Incoming highs ramp in over [0, L/8]. Mids cross at L/2: outgoing.mid Step→-40dB at L/2, incoming.mid was Step→-40dB at bar 0, then Step→0dB at L/2+1. Bass swap single Step at 7L/8 (outgoing.low→-40dB, incoming.low→0dB).
**Tested on transitions:** Forced session 2026-05-29 + historical feedback.
**Feedback:** "This transition is so bad." Earlier in the day: "felt bad, BPM seems not synced, power not the same." Consistently negative.
**Decision:** **Drop** or radically rework. The "thin moment" at L/2 + late bass swap at 7L/8 combination doesn't work. Too many moving parts producing a convoluted gesture. If anything survives the rework, it should be just the early highs reveal portion.

---

### 9. echo-throw

**What it does:** Equal-power crossfade with a 1-bar echo flourish on the outgoing deck. Triangle wet envelope over the last 2 bars: 0→0.6 over 0.25 bar, then 0.6→0 over 0.75 bar. Beat-length delay, feedback 0.75 (high).
**Tested on transitions:** Forced session 2026-05-29 16:xx
**Feedback:** "So bad. I think also the echo effects are not working — it seemed as a fast crossfade. Remove it."
**Decision:** **Drop.** Combined with echo-out's "didn't feel too much of an echo" feedback, the echo subsystem appears not to be delivering audible echo character at any wet/feedback level we've tried. Either there's a bug in the EchoLine implementation OR the wet mix levels are masked by the dry signal. **Investigate the echo subsystem before adding any new echo-based recipes.**

---

### 10. highs-first

**What it does:** Equal-power crossfade with incoming entering treble-only. Incoming fader rises fast (equal-power over [0, L/8]). Incoming starts with EqLow=-40dB and EqMid=-40dB. Mids Step→0 at L/4. Bass Step swap at 3L/4. Outgoing equal-power fades out over the full window.
**Tested on transitions:** 001→011 (L=8), plus a second transition.
**Feedback:** "Bad. Seemed unnatural. Remove." Initial reaction was uncertain ("maybe because of the tracks"), but after a second listen confirmed it doesn't work.
**Decision:** **Drop.** Same step-based EQ-reveal family as three-band-swap (also dropped). Pattern: discrete Step-based EQ reveals across multiple bands feel unnatural for this listener. Continuous filter sweeps (filter-fade) work better than discrete Step EQ reveals.

---

### 11. drop

**What it does:** Incoming plays for 4 bars with high-pass filter (treble-only riser, knob 0.85→0.55). At bar 4, both faders hard-cut: outgoing→0, incoming→full and filter→0 (bypass). Length fixed at 4 bars.
**Tested on transitions:** Forced session 2026-05-29.
**Feedback:** "When I heard it now it wasn't that good. Well, we can keep it and see later when it gets selected naturally."
**Decision:** **Keep.** Context-specific recipe — designed for energy_delta > 0.10 + phrase_aligned. Forced-testing on arbitrary pairs is unfair. Trust the picker to fire it only when conditions are right. Re-evaluate after a few natural selections.

---

### 12. breakdown-bridge

**What it does:** Outgoing's mids + bass linearly ramp to -40dB over bars [0, 3]. At bar 4: outgoing fader Step→0, incoming fader Step→1. Outgoing's highs ring through the breakdown alone. Length fixed at 4 bars.
**Tested on transitions:** Forced session 2026-05-29.
**Feedback:** "No remove it." Immediate rejection.
**Decision:** **Drop.** Same Step-based EQ-ramp pattern as the other rejected recipes (three-band-swap, highs-first). The "outgoing thins down to highs then hard-cut" gesture doesn't work for this listener.

---

### 13. double-drop

**What it does:** Outgoing fader Linear 1→0 over [0, 0.5]. Both decks silent for 0.5 bar (anticipation gap). At bar 1: both faders Step→1 (both at full volume together). Both play 1 bar layered. At bar 2: outgoing Step→0. Length fixed at 2 bars.
**Tested on transitions:** Forced session 2026-05-29.
**Feedback:** "Nah remove it entirely."
**Decision:** **Drop entirely.** Shared-bar layering of two full-volume tracks is exactly the layering-clash failure mode the user has rejected repeatedly.

---

### 14. vocal-overlay

**What it does:** Stems-aware, 16 bars. Outgoing equal-power fades out over [12, 16]. Incoming mixed fader Step→1 at bar 0 (all stems muted to 0). Incoming vocals stem Linear 0→1 over [0, 4]. Incoming drums/bass/other stems Linear 0→1 over [12, 16].
**Tested on transitions:** Historical feedback on multiple sessions (handoff document + earlier in current session).
**Feedback (historical):**
- Handoff bug: "VocalOverlay fired on 009→013, incoming was cued in stems mode (cued_in_stems_mode=true), but listener heard incoming's full mix from bar 0 instead of just vocals." Stem isolation not audibly working.
- Conceptual critique: "Listening to two melodies is ambiguous." Incoming vocals over outgoing's continuing music = harmonic / vocal-line clash that doesn't resolve until bar 12.
**Decision:** **Drop.** Even if the stem-isolation bug is fixed by the madmom re-analysis (uncertain), the fundamental gesture of layering vocals over a continuing outgoing track is rejected by this listener. The "vocal on top" layering pattern doesn't work for focused indoor listening.

---

### 15. bass-swap-stem-aware

**What it does:** Stems-aware, 16 bars. Equal-power crossfade on mixed faders. Incoming bass stem muted from bar 0. At the bass-density-valley bar (default 8, picked from incoming.bass_density), outgoing bass stem killed AND incoming bass stem released simultaneously. Other stems crossfade naturally via mixed fader.
**Tested on transitions:** Multiple natural firings during the listening session + extensive prior discussion.
**Feedback (historical):**
- "Seemed better but a little flaky in the first part" (handoff document).
- Conceptual: "Real DJs don't just swap bass — they swap the rhythm section (drums + bass)." Bass-only stem swap leaves both decks' drum kits overlapping during the layering window. Cluttered.
**Decision:** **Refactor to RhythmSwap** — extend the stem manipulation to also swap drums together with bass at the same moment (or at a slightly different moment for a more elaborate gesture). The bass-only stem swap is a half-measure that exposes the drum-doubling problem.

---

### 16. breakdown-into-drop

**What it does:** Stems-aware, 8 bars. Outgoing drums stem Linear 1→0 over [0, drop_bar]. Outgoing mixed fader Step→0 at drop_bar. Incoming starts silent (fader=0); incoming fader Step→1 at drop_bar. drop_bar is picked from incoming.drum_density (first bar ≥0.85; fallback 4).
**Tested on transitions:** 001→011 (L=8, duration 15.3s)
**Feedback:** "The breakdown felt sooo bad." The pre-drop breakdown segment (outgoing without drums but with everything else) sounds wrong — like a karaoke version.
**Decision:** **Drop.** Removing only the drum stem while keeping bass/vocals/other audible creates an unmusical "missing drums" sound. If the breakdown gesture is wanted, it needs more aggressive stem manipulation (e.g., kill drums + bass to leave only melody as the breakdown), or just a clean fade.

---

## Cross-cutting observations

(Things that span recipes — beat alignment, loudness, key clashes, etc.)

- **Filter-based recipes are landing well.** filter-fade preferred over plain equal-power. filter-kill acceptable as energy-down variant. Suggests we should expand the filter-based recipe family.
- **Layering with vocals/mids on top causes the "ambiguous" problem.** bass-swap, vocal-overlay, etc. all suffer from this. Stems-aware drums+bass swaps avoid the issue.
- **Madmom local section BPMs differ from globals by 0.1-0.4% on most transitions.** Real measurable signal, though small. Will continue showing up in `bpm_delta_pct` log field.
- **Echo subsystem appears non-audible.** Both echo-out (subtle wet ramp) and echo-throw (aggressive triangle envelope, feedback=0.75) failed to deliver audible echo character. Either a bug in `engine/src/effects.rs::EchoLine` or the wet mix is masked by dry. **Action item:** investigate echo implementation before any new echo recipes. Test with a one-deck unit test that captures dry + wet output and verifies the wet contains delayed copies of dry.

## Session pause notes (2026-05-29)

Paused after recipe #5 of 16. Tested so far: equal-power, hard-cut, bass-swap (historical), filter-fade, filter-kill. Remaining: echo-out, loop-out, three-band-swap, echo-throw, highs-first, drop, breakdown-bridge, double-drop, vocal-overlay, bass-swap-stem-aware, breakdown-into-drop.

Engine is stopped. Resume by force-restarting with `--force-recipe echo-out` (next in the list).

## Decisions summary

**Kept (7 of 16):**
- `filter-fade` — **promoted to default crossfade** (preferred over equal-power)
- `filter-kill` — keep for energy-down transitions
- `equal-power` — deprioritize, keep as final fallback
- `hard-cut` — keep for genre mismatch; design "punctuated" variant with outgoing FX
- `echo-out` — keep with caveats; echo subsystem suspect
- `loop-out` — loop = good primitive; replace the equal-power fade with better incoming-arrival
- `drop` — keep, context-specific (trust the picker)

**Dropped (7 of 16):**
- `three-band-swap` — convoluted multi-band Step swap, consistently bad
- `echo-throw` — echo character not audible; remove
- `highs-first` — unnatural Step-based EQ reveal
- `breakdown-bridge` — Step-based bass+mid kill, doesn't work
- `double-drop` — full-volume layering of two tracks = clash
- `vocal-overlay` — layering vocals over outgoing = ambiguity (fundamental gesture rejected)
- `breakdown-into-drop` — outgoing-without-drums sounds bad (karaoke effect)

**Refactor (2 of 16):**
- `bass-swap` — EQ kill leaves drums overlapping; rework as stems-aware drums+bass swap
- `bass-swap-stem-aware` — bass-only stem swap leaves drums overlapping; rework as drums+bass together (RhythmSwap)

**Net direction:**

| Recipe family | Status |
|---|---|
| Continuous filter sweeps | **WORK** — filter-fade is the new hero |
| Equal-power crossfades | Acceptable but unexciting |
| Step-based EQ reveals | **DON'T WORK** (4 of 4 dropped: three-band-swap, highs-first, breakdown-bridge, breakdown-into-drop) |
| Layering (vocals/full mix on top) | **DON'T WORK** — fundamental ambiguity rejection |
| Stems-aware drums+bass swap (RhythmSwap) | **POTENTIAL** — not implemented yet, design proposed |
| Loops | **GOOD PRIMITIVE** — combine with better arrival |
| Echo effects | **SUSPECT** — investigate audibility before keeping |
| Hard-cut variants | Keep, expand with outgoing FX |

**Action items in priority order:**

1. **Investigate echo subsystem** — the EchoLine implementation appears not to deliver audible echo character. Test with a one-deck unit test that captures dry + wet and verifies wet contains delayed dry.
2. **Promote filter-fade** to default crossfade — raise its score floor, demote equal-power to fallback.
3. **Drop the dropped recipes** — remove or comment-out `three-band-swap`, `echo-throw`, `highs-first`, `breakdown-bridge`, `double-drop`, `vocal-overlay`, `breakdown-into-drop` from the picker candidates list. Refactor scoring.
4. **Design "punctuated hard-cut"** variant — apply outgoing FX (echo, filter sweep, tape stop) before the cut, for genre-mismatch use.
5. **Design RhythmSwap** — stems-aware drums+bass swap together. Replaces both `bass-swap` (EQ) and `bass-swap-stem-aware` (bass-only).
6. **Enhance loop-out** — replace equal-power fade with filter-fade on incoming, OR layer incoming's rhythm stems under the looping outgoing.
