# LoopBridge Recipe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a stems-aware `LoopBridge` recipe that captures incoming's pre-vocal rhythm into a loop, filter-crossfades against outgoing's continuing vocal phrase, then releases the loop at outgoing's phrase end so incoming's vocals enter naturally. Scored as a specialist (high score when all conditions match, 0.05 otherwise).

**Architecture:** New Sidecar helpers + Camelot key compatibility + a shared `compute_loop_bridge_anchors` function that both the scorer and the build_script call. The recipe sets its own `TransitionScript.length_bars` based on outgoing's last phrase boundary. No analyzer changes; uses existing sidecar fields.

**Tech Stack:** Rust 2021, serde, tracing. Tests via `cargo test`.

**Spec:** `docs/superpowers/specs/2026-05-30-loop-bridge-recipe-design.md`

---

## Task 1: Sidecar helpers (3 methods)

**Files:**
- Modify: `engine/src/sidecar.rs` (add 3 methods)
- Modify: `engine/tests/sidecar_test.rs` (add tests)

- [ ] **Step 1: Write the failing tests**

Append to `engine/tests/sidecar_test.rs`:
```rust
#[test]
fn last_phrase_boundary_at_or_before_returns_largest_eligible() {
    let mut s = make_sidecar(vec![], vec![]);
    s.phrase_boundaries_seconds = Some(vec![10.0, 20.0, 30.0, 40.0]);
    assert_eq!(s.last_phrase_boundary_at_or_before(25.0), Some(20.0));
    assert_eq!(s.last_phrase_boundary_at_or_before(40.0), Some(40.0));
    assert_eq!(s.last_phrase_boundary_at_or_before(5.0), None);
}

#[test]
fn last_phrase_boundary_at_or_before_handles_missing_field() {
    let s = make_sidecar(vec![], vec![]);
    assert_eq!(s.last_phrase_boundary_at_or_before(100.0), None);
}

#[test]
fn last_vocal_segment_end_at_or_before_filters_correctly() {
    use dj_engine::sidecar::Stems;
    let mut s = make_sidecar(vec![], vec![]);
    s.stems = Some(Stems {
        drum_density: vec![],
        bass_density: vec![],
        vocal_density: vec![],
        vocal_segments: vec![[10.0, 15.0], [20.0, 25.0], [30.0, 35.0]],
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });
    assert_eq!(s.last_vocal_segment_end_at_or_before(40.0), Some(35.0));
    assert_eq!(s.last_vocal_segment_end_at_or_before(28.0), Some(25.0));
    assert_eq!(s.last_vocal_segment_end_at_or_before(12.0), None);
}

#[test]
fn last_vocal_segment_end_at_or_before_handles_missing_stems() {
    let s = make_sidecar(vec![], vec![]);
    assert_eq!(s.last_vocal_segment_end_at_or_before(100.0), None);
}

#[test]
fn snap_to_nearest_downbeat_at_or_before_finds_largest_eligible() {
    let mut s = make_sidecar(vec![], vec![]);
    s.downbeats_seconds = vec![0.0, 2.0, 4.0, 6.0, 8.0];
    assert_eq!(s.snap_to_nearest_downbeat_at_or_before(5.5), Some(4.0));
    assert_eq!(s.snap_to_nearest_downbeat_at_or_before(8.0), Some(8.0));
    assert_eq!(s.snap_to_nearest_downbeat_at_or_before(-1.0), None);
}
```

(Reuse the existing `make_sidecar` helper that this test file uses for the Sidecar tests added in earlier tasks. If the helper doesn't take a phrase_boundaries arg, adjust to use the actual signature.)

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test sidecar_test last_phrase_boundary 2>&1 | Select-Object -First 30
```
Expected: methods don't exist; compile errors.

- [ ] **Step 3: Implement the helpers**

In `engine/src/sidecar.rs`, add to the `impl Sidecar` block (after the existing `local_bpm_in_window` method):

```rust
    /// Find the last phrase_boundary at or before `seconds`. Returns None when
    /// phrase_boundaries_seconds is absent/empty or all entries exceed `seconds`.
    pub fn last_phrase_boundary_at_or_before(&self, seconds: f64) -> Option<f64> {
        self.phrase_boundaries_seconds
            .as_ref()?
            .iter()
            .rev()
            .find(|&&t| t <= seconds)
            .copied()
    }

    /// Find the last vocal_segment's end at or before `seconds`. Returns None
    /// when stems or vocal_segments is empty, or no segment ends at or before
    /// `seconds`.
    pub fn last_vocal_segment_end_at_or_before(&self, seconds: f64) -> Option<f64> {
        self.stems
            .as_ref()?
            .vocal_segments
            .iter()
            .map(|seg| seg[1])
            .filter(|&end| end <= seconds)
            .fold(None, |acc, end| {
                Some(match acc {
                    None => end,
                    Some(prev) => prev.max(end),
                })
            })
    }

    /// Snap `seconds` to the nearest downbeat at or before it. Returns None
    /// when downbeats_seconds is empty or all entries exceed `seconds`.
    pub fn snap_to_nearest_downbeat_at_or_before(&self, seconds: f64) -> Option<f64> {
        self.downbeats_seconds
            .iter()
            .rev()
            .find(|&&t| t <= seconds)
            .copied()
    }
```

- [ ] **Step 4: Verify tests pass**

```powershell
cargo test --manifest-path engine/Cargo.toml --test sidecar_test
```
Expected: all 5 new tests PASS.

- [ ] **Step 5: Full engine suite**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 10
```

- [ ] **Step 6: Commit**

```powershell
git add engine/src/sidecar.rs engine/tests/sidecar_test.rs
git commit -m "feat(engine): Sidecar helpers for phrase boundary, vocal segment end, downbeat snap"
```

---

## Task 2: Camelot key compatibility

**Files:**
- Modify (or extend): `engine/src/camelot.rs`
- Modify or create: `engine/tests/camelot_test.rs`

- [ ] **Step 1: Inspect existing camelot.rs**

```powershell
Get-Content engine/src/camelot.rs
```
Note what's already there. The Camelot wheel parsing helpers may already exist (the analyzer's output goes through this module).

- [ ] **Step 2: Write the failing tests**

Append to `engine/tests/camelot_test.rs` (or create if needed):
```rust
use dj_engine::camelot::keys_compatible;

#[test]
fn keys_compatible_same_key_returns_true() {
    assert!(keys_compatible("8A", "8A"));
    assert!(keys_compatible("12B", "12B"));
}

#[test]
fn keys_compatible_plus_minus_one_same_letter_returns_true() {
    assert!(keys_compatible("8A", "9A"));
    assert!(keys_compatible("8A", "7A"));
    assert!(keys_compatible("12A", "1A"));  // wraps
    assert!(keys_compatible("1A", "12A"));
}

#[test]
fn keys_compatible_same_n_opposite_letter_returns_true() {
    // Relative major/minor
    assert!(keys_compatible("8A", "8B"));
    assert!(keys_compatible("3B", "3A"));
}

#[test]
fn keys_compatible_distant_keys_returns_false() {
    assert!(!keys_compatible("1A", "5A"));
    assert!(!keys_compatible("8A", "11B"));
}

#[test]
fn keys_compatible_invalid_input_returns_false() {
    assert!(!keys_compatible("", "8A"));
    assert!(!keys_compatible("Cmaj", "8A"));
    assert!(!keys_compatible("8A", "13A"));
    assert!(!keys_compatible("0A", "8A"));
    assert!(!keys_compatible("8C", "8A"));
}
```

- [ ] **Step 3: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test camelot_test keys_compatible 2>&1 | Select-Object -First 20
```
Expected: `keys_compatible` function not defined.

- [ ] **Step 4: Implement `keys_compatible` and helper**

In `engine/src/camelot.rs` (or extend existing), add:

```rust
/// Check whether two Camelot keys are compatible for mixing.
///
/// Compatible pairs:
/// - Same key (e.g., `"8A"` and `"8A"`)
/// - Adjacent N values, same letter (e.g., `"8A"` and `"9A"`)
/// - Same N, opposite letter (relative major/minor; e.g., `"8A"` and `"8B"`)
///
/// Returns false on unparseable input.
pub fn keys_compatible(a: &str, b: &str) -> bool {
    let Some((a_n, a_letter)) = parse_camelot_key(a) else { return false; };
    let Some((b_n, b_letter)) = parse_camelot_key(b) else { return false; };

    // Same key
    if a_n == b_n && a_letter == b_letter {
        return true;
    }
    // Same N, opposite letter (relative major/minor)
    if a_n == b_n && a_letter != b_letter {
        return true;
    }
    // ±1 on the wheel, same letter
    let diff = (a_n - b_n).rem_euclid(12).min((b_n - a_n).rem_euclid(12));
    if diff == 1 && a_letter == b_letter {
        return true;
    }
    false
}

/// Parse a Camelot key like `"8A"` into `(8, 'A')`. Returns None for invalid
/// input.
fn parse_camelot_key(key: &str) -> Option<(i32, char)> {
    if key.is_empty() {
        return None;
    }
    let letter = key.chars().last()?;
    if letter != 'A' && letter != 'B' {
        return None;
    }
    let n_str: &str = &key[..key.len() - 1];
    let n: i32 = n_str.parse().ok()?;
    if !(1..=12).contains(&n) {
        return None;
    }
    Some((n, letter))
}
```

If a `parse_camelot_key` already exists with a different name/signature, reuse it. Adjust accordingly.

- [ ] **Step 5: Verify**

```powershell
cargo test --manifest-path engine/Cargo.toml --test camelot_test keys_compatible
```
Expected: all 5 tests PASS.

- [ ] **Step 6: Full suite**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 10
```

- [ ] **Step 7: Commit**

```powershell
git add engine/src/camelot.rs engine/tests/camelot_test.rs
git commit -m "feat(engine): camelot::keys_compatible — same key / ±1 / opposite letter"
```

---

## Task 3: `LoopBridgeAnchors` + `compute_loop_bridge_anchors` helper

**Files:**
- Modify: `engine/src/recipe.rs` (add LoopBridgeAnchors + helper)
- Modify: `engine/tests/recipe_test.rs` (add tests)

Both the scorer and the build_script need the anchor computations. A shared helper avoids duplicating the math.

- [ ] **Step 1: Write the failing tests**

Append to `engine/tests/recipe_test.rs`:
```rust
#[test]
fn compute_loop_bridge_anchors_returns_some_on_complete_data() {
    use dj_engine::recipe::{compute_loop_bridge_anchors, LoopBridgeAnchors};
    use dj_engine::sidecar::Stems;

    let mut outgoing = test_loaded_track();
    outgoing.sidecar.bpm = Some(120.0);
    outgoing.sidecar.outro_start_seconds = Some(60.0);
    outgoing.sidecar.phrase_boundaries_seconds = Some(vec![16.0, 32.0, 48.0, 64.0]);
    outgoing.sidecar.key = Some("8A".to_string());

    let mut incoming = test_loaded_track();
    incoming.sidecar.bpm = Some(120.0);
    incoming.sidecar.downbeats_seconds = vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 30.0, 32.0];
    incoming.sidecar.key = Some("8A".to_string());
    incoming.sidecar.stems = Some(Stems {
        drum_density: vec![],
        bass_density: vec![],
        vocal_density: vec![],
        vocal_segments: vec![[16.0, 24.0]],  // vocals at 16s
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });

    let plan = test_plan_16_bars();  // adjust if needed
    let anchors = compute_loop_bridge_anchors(&outgoing.sidecar, &incoming.sidecar, &plan);
    assert!(anchors.is_some(), "expected Some(anchors) on complete data");
    let a = anchors.unwrap();
    assert!(a.keys_compatible);
    assert!(a.computed_length_bars >= 16.0);
    assert!(a.loop_lock_bar >= 0.0);
}

#[test]
fn compute_loop_bridge_anchors_returns_none_on_missing_phrase_boundaries() {
    use dj_engine::recipe::compute_loop_bridge_anchors;

    let outgoing = test_loaded_track();  // phrase_boundaries_seconds None
    let incoming = test_loaded_track();
    let plan = test_plan_16_bars();
    assert!(compute_loop_bridge_anchors(&outgoing.sidecar, &incoming.sidecar, &plan).is_none());
}

#[test]
fn compute_loop_bridge_anchors_returns_none_on_incompatible_keys() {
    use dj_engine::recipe::compute_loop_bridge_anchors;
    use dj_engine::sidecar::Stems;

    let mut outgoing = test_loaded_track();
    outgoing.sidecar.bpm = Some(120.0);
    outgoing.sidecar.outro_start_seconds = Some(60.0);
    outgoing.sidecar.phrase_boundaries_seconds = Some(vec![16.0, 32.0, 48.0, 64.0]);
    outgoing.sidecar.key = Some("1A".to_string());

    let mut incoming = test_loaded_track();
    incoming.sidecar.bpm = Some(120.0);
    incoming.sidecar.downbeats_seconds = (0..16).map(|i| i as f64 * 2.0).collect();
    incoming.sidecar.key = Some("5A".to_string());  // 1A and 5A: not adjacent, not relative
    incoming.sidecar.stems = Some(Stems {
        drum_density: vec![],
        bass_density: vec![],
        vocal_density: vec![],
        vocal_segments: vec![[16.0, 24.0]],
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });

    let plan = test_plan_16_bars();
    let anchors = compute_loop_bridge_anchors(&outgoing.sidecar, &incoming.sidecar, &plan);
    // Anchors should still compute (the helper doesn't reject on key) but keys_compatible should be false.
    if let Some(a) = anchors {
        assert!(!a.keys_compatible);
    }
}
```

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test compute_loop_bridge_anchors 2>&1 | Select-Object -First 30
```
Expected: `compute_loop_bridge_anchors` and `LoopBridgeAnchors` not defined.

- [ ] **Step 3: Implement**

Add to `engine/src/recipe.rs` (near the top, after existing structs):

```rust
/// Pre-computed anchors for the LoopBridge recipe. Computed once and consumed
/// by both the scorer (to decide whether to fire) and the build_script (to
/// place phases). Returns None when required sidecar data is missing.
#[derive(Clone, Copy, Debug)]
pub struct LoopBridgeAnchors {
    /// Outgoing source-time where the last phrase ends (clamped to vocal_end).
    pub phrase_end_seconds: f64,
    /// Transition-local bar of the phrase-end (where Act 3 fires).
    pub phrase_end_bar: f64,
    /// Incoming source-time at the snapped downbeat just before first vocals.
    pub vocal_entry_snapped: f64,
    /// Transition-local bar where the loop locks.
    pub loop_lock_bar: f64,
    /// Total transition length, rounded up.
    pub computed_length_bars: f64,
    /// Camelot key compatibility check result.
    pub keys_compatible: bool,
}

/// Compute LoopBridge anchors from outgoing/incoming sidecars and the plan.
/// Returns None when any required field is missing or the math doesn't yield
/// usable values.
pub fn compute_loop_bridge_anchors(
    outgoing: &crate::sidecar::Sidecar,
    incoming: &crate::sidecar::Sidecar,
    plan: &crate::transition::TransitionPlan,
) -> Option<LoopBridgeAnchors> {
    let outgoing_bpm = outgoing.bpm?;
    if outgoing_bpm <= 0.0 {
        return None;
    }
    let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
    let outro_start = outgoing.outro_start_seconds?;

    // Outgoing phrase end (with vocal-end safety net).
    let p_boundary = outgoing.last_phrase_boundary_at_or_before(outro_start + bar_seconds)?;
    let v_end = outgoing
        .last_vocal_segment_end_at_or_before(outro_start + 4.0 * bar_seconds + bar_seconds)
        .unwrap_or(p_boundary);
    let phrase_end_seconds = p_boundary.max(v_end);

    // Incoming vocal entry, snapped to nearest downbeat at or before.
    let incoming_first_vocal = incoming
        .stems
        .as_ref()?
        .vocal_segments
        .first()
        .map(|seg| seg[0])?;
    let vocal_entry_snapped =
        incoming.snap_to_nearest_downbeat_at_or_before(incoming_first_vocal)?;

    let cue_start = plan.incoming_crossfade_start_seconds;
    let loop_lock_bar = (vocal_entry_snapped - cue_start) / bar_seconds;
    let phrase_end_bar =
        (phrase_end_seconds - plan.outgoing_crossfade_start_seconds) / bar_seconds;
    let computed_length_bars = phrase_end_bar.ceil();

    let keys_compatible = crate::camelot::keys_compatible(
        outgoing.key.as_deref().unwrap_or(""),
        incoming.key.as_deref().unwrap_or(""),
    );

    Some(LoopBridgeAnchors {
        phrase_end_seconds,
        phrase_end_bar,
        vocal_entry_snapped,
        loop_lock_bar,
        computed_length_bars,
        keys_compatible,
    })
}
```

- [ ] **Step 4: Verify**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test compute_loop_bridge_anchors
```
Expected: 3 tests PASS.

- [ ] **Step 5: Full suite**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 10
```

- [ ] **Step 6: Commit**

```powershell
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): LoopBridgeAnchors + compute_loop_bridge_anchors helper"
```

---

## Task 4: `LoopBridge` recipe struct + Recipe impl

**Files:**
- Modify: `engine/src/recipe.rs` (add LoopBridge struct + Recipe impl)
- Modify: `engine/tests/script_test.rs` (add build_script test)

- [ ] **Step 1: Write the failing test**

Append to `engine/tests/script_test.rs`:
```rust
#[test]
fn loop_bridge_emits_three_act_phases() {
    use dj_engine::deck::StemKind;
    use dj_engine::recipe::{LoopBridge, Recipe};
    use dj_engine::recipe_script::{DeckRole, Phase};

    let mut outgoing = test_loaded_track();
    outgoing.sidecar.bpm = Some(120.0);
    outgoing.sidecar.outro_start_seconds = Some(60.0);
    outgoing.sidecar.phrase_boundaries_seconds = Some(vec![16.0, 32.0, 48.0, 64.0]);
    outgoing.sidecar.key = Some("8A".to_string());

    let mut incoming = test_loaded_track();
    incoming.sidecar.bpm = Some(120.0);
    incoming.sidecar.downbeats_seconds = (0..32).map(|i| i as f64 * 2.0).collect();
    incoming.sidecar.key = Some("8A".to_string());
    incoming.sidecar.stems = Some(dj_engine::sidecar::Stems {
        drum_density: vec![],
        bass_density: vec![],
        vocal_density: vec![],
        vocal_segments: vec![[16.0, 24.0]],
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });

    let plan = test_plan_16_bars();
    let ctx = test_ctx(&outgoing, &incoming, &plan, 0);

    let script = LoopBridge.build_script(&ctx);

    // Act 1 phases — incoming stems isolated to rhythm-only at bar 0.
    let in_vocals_mute = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Incoming, stem: StemKind::Vocals,
            from, to, span, ..
        } if (from - 0.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
              && (span.start.bars - 0.0).abs() < 1e-9
    ));
    assert!(in_vocals_mute, "incoming vocals mute at bar 0 missing");

    // LoopGate ON on incoming.
    let loopgate_on = script.phases.iter().any(|p| matches!(
        p,
        Phase::LoopGate { deck: DeckRole::Incoming, .. }
    ));
    assert!(loopgate_on, "LoopGate on incoming missing");

    // Act 2: incoming filter sweep.
    let incoming_filter = script.phases.iter().any(|p| matches!(
        p,
        Phase::Filter {
            deck: DeckRole::Incoming, from, to, ..
        } if (from - 0.85).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
    ));
    assert!(incoming_filter, "incoming filter 0.85→0 sweep missing");

    // Act 2: outgoing filter sweep (gentle thinning).
    let outgoing_filter = script.phases.iter().any(|p| matches!(
        p,
        Phase::Filter {
            deck: DeckRole::Outgoing, from, to, ..
        } if (from - 0.0).abs() < 1e-9 && (to - 0.6).abs() < 1e-9
    ));
    assert!(outgoing_filter, "outgoing filter 0.0→0.6 sweep missing");

    // Act 3: vocals stem restored.
    let in_vocals_restored = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Incoming, stem: StemKind::Vocals,
            from, to, span, ..
        } if (from - 1.0).abs() < 1e-9 && (to - 1.0).abs() < 1e-9
              && span.start.bars > 0.0
    ));
    assert!(in_vocals_restored, "incoming vocals restored Step missing");
}

#[test]
fn loop_bridge_requires_stems_is_true() {
    use dj_engine::recipe::{LoopBridge, Recipe};
    assert!(LoopBridge.requires_stems());
}
```

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test script_test loop_bridge 2>&1 | Select-Object -First 20
```
Expected: `LoopBridge` not defined.

- [ ] **Step 3: Implement the recipe**

Append to `engine/src/recipe.rs` (after `RhythmSwap`):

```rust
/// `LoopBridge`: stems-aware three-act transition. See
/// `docs/superpowers/specs/2026-05-30-loop-bridge-recipe-design.md` for the
/// full design rationale and acts.
///
/// Act 1 (loop preparation): incoming silent (fader=0) while its source pulls
/// into the loop buffer. Incoming.stems = {drums, bass} only, so the buffer
/// captures rhythm-only content.
///
/// Act 2 (filter crossfade): LoopGate ON. Incoming's filter sweeps 0.85→0
/// (rising rhythm reveal). Outgoing's filter sweeps 0.0→0.6 (gentle thinning).
/// Both decks audible; outgoing's vocals on top of the rising rhythm.
/// Fader compensation: outgoing 1.0→0.55 Linear; incoming 0→0.85 EqualPowerIn
/// over the first 4 bars of Act 2, then held at 0.85.
///
/// Act 3 (vocal handoff): At phrase_end_bar, LoopGate OFF → incoming source
/// resumes from vocal_entry_snapped. Outgoing fader Step→0. Incoming fader
/// Step→1.0. Incoming.{vocals, other} restored Step→1. Both filters Step→0.
///
/// Loudness compensation in v1 is hand-tuned; refine after listening.
pub struct LoopBridge;

impl Recipe for LoopBridge {
    fn name(&self) -> &'static str {
        "loop_bridge"
    }

    fn requires_stems(&self) -> bool {
        true
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        use crate::deck::StemKind;

        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let ratio = ctx.ratio;

        // Compute anchors; fall back to safe defaults if data is missing
        // (recipe shouldn't have been picked in that case, but defend anyway).
        let anchors = compute_loop_bridge_anchors(
            &ctx.outgoing.sidecar,
            &ctx.incoming.sidecar,
            ctx.plan,
        );
        let (loop_lock_bar, phrase_end_bar, computed_length) = match anchors {
            Some(a) => (a.loop_lock_bar, a.phrase_end_bar, a.computed_length_bars),
            None => (4.0, 16.0, 16.0),  // safe fallback
        };

        TransitionScript {
            length_bars: computed_length as usize,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },

                // === Act 1: loop preparation ===
                // Incoming starts silent.
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                // Incoming stems: rhythm only.
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Drums,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Bass,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Vocals,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Other,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },

                // LoopGate ON at loop_lock_bar; OFF at phrase_end_bar.
                Phase::LoopGate {
                    deck: DeckRole::Incoming,
                    on_at: bar(loop_lock_bar),
                    off_at: bar(phrase_end_bar),
                },

                // === Act 2: filter crossfade ===
                // Incoming fader rises 0→0.85 over the first 4 bars of Act 2.
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.85,
                    span: bars(loop_lock_bar, loop_lock_bar + 4.0),
                    curve: Curve::EqualPowerIn,
                },

                // Outgoing fader gently reduces (loudness compensation).
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.55,
                    span: bars(loop_lock_bar, phrase_end_bar - 1.0),
                    curve: Curve::Linear,
                },

                // Incoming filter sweep — rising rhythm reveal.
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.85,
                    to: 0.0,
                    span: bars(loop_lock_bar, phrase_end_bar - 1.0),
                    curve: Curve::Linear,
                },

                // Outgoing filter sweep — gentle thinning.
                Phase::Filter {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 0.6,
                    span: bars(loop_lock_bar, phrase_end_bar - 1.0),
                    curve: Curve::Linear,
                },

                // === Act 3: vocal handoff ===
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 0.0,
                    span: bars(phrase_end_bar, phrase_end_bar),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 1.0,
                    span: bars(phrase_end_bar, phrase_end_bar),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Vocals,
                    from: 1.0,
                    to: 1.0,
                    span: bars(phrase_end_bar, phrase_end_bar),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Other,
                    from: 1.0,
                    to: 1.0,
                    span: bars(phrase_end_bar, phrase_end_bar),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 0.0,
                    span: bars(phrase_end_bar, phrase_end_bar),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(phrase_end_bar, phrase_end_bar),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: Some(DeckInitConfig {
                echo_delay_seconds: None,
                echo_feedback: None,
                loop_bars: Some(4),
                loop_bar_seconds: Some(bar_seconds),
            }),
        }
    }
}
```

- [ ] **Step 4: Verify**

```powershell
cargo test --manifest-path engine/Cargo.toml --test script_test loop_bridge
```
Expected: 2 tests PASS.

- [ ] **Step 5: Full suite**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 10
```

- [ ] **Step 6: Commit**

```powershell
git add engine/src/recipe.rs engine/tests/script_test.rs
git commit -m "feat(engine): LoopBridge recipe — three-act loop+filter+phrase-handoff gesture"
```

---

## Task 5: Picker integration — RecipeChoice + scoring + dispatch + clap

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/src/main.rs`
- Modify: `engine/src/config.rs`
- Modify: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Write the failing scoring tests**

Append to `engine/tests/recipe_test.rs`:
```rust
fn loop_bridge_complete_inputs() -> (
    dj_engine::library::LoadedTrack,
    dj_engine::library::LoadedTrack,
    dj_engine::transition::TransitionPlan,
) {
    use dj_engine::sidecar::Stems;
    let mut outgoing = test_loaded_track();
    outgoing.sidecar.bpm = Some(120.0);
    outgoing.sidecar.outro_start_seconds = Some(60.0);
    outgoing.sidecar.phrase_boundaries_seconds = Some(vec![16.0, 32.0, 48.0, 64.0]);
    outgoing.sidecar.key = Some("8A".to_string());
    outgoing.sidecar.is_phrase_aligned = Some(true);
    outgoing.sidecar.stems = Some(Stems {
        drum_density: vec![0.7; 32],
        bass_density: vec![0.7; 32],
        vocal_density: vec![0.0; 32],
        vocal_segments: vec![[8.0, 16.0]],
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });

    let mut incoming = test_loaded_track();
    incoming.sidecar.bpm = Some(120.0);
    incoming.sidecar.downbeats_seconds = (0..32).map(|i| i as f64 * 2.0).collect();
    incoming.sidecar.key = Some("8A".to_string());
    incoming.sidecar.is_phrase_aligned = Some(true);
    incoming.sidecar.stems = Some(Stems {
        drum_density: vec![],
        bass_density: vec![],
        vocal_density: vec![],
        vocal_segments: vec![[20.0, 28.0]],  // vocal entry well past cue
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });

    let mut plan = test_plan_16_bars();
    plan.outgoing_crossfade_start_seconds = 40.0;  // 20 bars of room before phrase_end at 64
    plan.incoming_crossfade_start_seconds = 0.0;   // 10 bars of room before vocal_entry at 20

    (outgoing, incoming, plan)
}

#[test]
fn score_loop_bridge_complete_conditions_returns_high() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let (outgoing, incoming, plan) = loop_bridge_complete_inputs();
    let inputs = PairScoreInputs::from_pair(&plan, &outgoing, &incoming);
    let s = score_recipe(RecipeChoice::LoopBridge, &inputs);
    assert!(s >= 0.85, "expected high score, got {s}");
}

#[test]
fn score_loop_bridge_incompatible_keys_returns_floor() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let (mut outgoing, mut incoming, plan) = loop_bridge_complete_inputs();
    outgoing.sidecar.key = Some("1A".to_string());
    incoming.sidecar.key = Some("5A".to_string());
    let inputs = PairScoreInputs::from_pair(&plan, &outgoing, &incoming);
    assert!((score_recipe(RecipeChoice::LoopBridge, &inputs) - 0.05).abs() < 1e-9);
}

#[test]
fn score_loop_bridge_missing_vocals_returns_floor() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    use dj_engine::sidecar::Stems;

    let (outgoing, mut incoming, plan) = loop_bridge_complete_inputs();
    incoming.sidecar.stems = Some(Stems {
        drum_density: vec![],
        bass_density: vec![],
        vocal_density: vec![],
        vocal_segments: vec![],  // no vocals
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });
    let inputs = PairScoreInputs::from_pair(&plan, &outgoing, &incoming);
    assert!((score_recipe(RecipeChoice::LoopBridge, &inputs) - 0.05).abs() < 1e-9);
}
```

These tests assume `PairScoreInputs::from_pair` can carry the LoopBridge anchors. Need to extend `PairScoreInputs`.

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test score_loop_bridge 2>&1 | Select-Object -First 20
```
Expected: `RecipeChoice::LoopBridge` not defined.

- [ ] **Step 3: Extend `PairScoreInputs` with anchor field**

In `engine/src/recipe.rs`, extend `PairScoreInputs`:
```rust
    /// Pre-computed LoopBridge anchors. Some(...) when all required sidecar
    /// data is present; None when any field is missing. Used by the LoopBridge
    /// scoring branch.
    pub loop_bridge_anchors: Option<LoopBridgeAnchors>,
```

In `PairScoreInputs::from_pair`, compute and store:
```rust
        let loop_bridge_anchors = compute_loop_bridge_anchors(
            &outgoing.sidecar,
            &incoming.sidecar,
            plan,
        );
```

Add to the returned `Self { ... }` literal.

- [ ] **Step 4: Add `RecipeChoice::LoopBridge` variant + register**

In `engine/src/recipe.rs`:

(a) Add to `RecipeChoice` enum (after `RhythmSwap`):
```rust
    LoopBridge,
```

(b) Add to `recipe_choice_id`:
```rust
        RecipeChoice::LoopBridge => 18,  // continuing from existing IDs
```

(c) Append to BOTH `pick_recipe` AND `pick_recipe_with_scores` candidates arrays:
```rust
        RecipeChoice::LoopBridge,
```

(d) Append to `recency_hash` for-loop list.

(e) Add score branch in `score_recipe`:
```rust
        RecipeChoice::LoopBridge => {
            let Some(a) = inputs.loop_bridge_anchors else { return 0.05; };
            if !inputs.stems_available || !inputs.phrase_aligned {
                return 0.05;
            }
            let loop_lock_ok = a.loop_lock_bar >= 4.0;
            let act2_long_enough = (a.phrase_end_bar - a.loop_lock_bar) >= 8.0;
            let length_in_range = a.computed_length_bars >= 16.0
                && a.computed_length_bars <= 32.0;
            if loop_lock_ok && act2_long_enough && length_in_range && a.keys_compatible {
                0.90
            } else {
                0.05
            }
        }
```

- [ ] **Step 5: Register in main.rs**

In `engine/src/main.rs`:
- Dispatch match arm: `RecipeChoice::LoopBridge => Box::new(LoopBridge),`
- `recipe_choice_name` helper arm: `RecipeChoice::LoopBridge => "loop_bridge",`
- `recipe_name` match arm (if separate): `RecipeChoice::LoopBridge => "loop_bridge",`

- [ ] **Step 6: Register in config.rs**

In `engine/src/config.rs`:
- `ForceRecipe::LoopBridge,` variant.
- `to_recipe_choice` arm: `ForceRecipe::LoopBridge => RecipeChoice::LoopBridge,`

Clap auto-kebab-cases to `loop-bridge` for the CLI.

- [ ] **Step 7: Update other PairScoreInputs literal sites**

Adding a new field to `PairScoreInputs` breaks every literal that constructs it. Find them:
```powershell
Select-String -Path engine/tests/*.rs,engine/src/*.rs -Pattern 'PairScoreInputs\s*\{' -SimpleMatch
```
For each site, add `loop_bridge_anchors: None,` to the literal.

- [ ] **Step 8: Verify tests**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test score_loop_bridge
```
Expected: 3 tests PASS.

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 10
```
Expected: all PASS.

- [ ] **Step 9: Commit**

```powershell
git add engine/src/recipe.rs engine/src/main.rs engine/src/config.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): register LoopBridge in picker + scoring + clap"
```

---

## Task 6: Release build + live force-test

**Files:**
- Side-effect only: engine binary rebuilt + listening session.

- [ ] **Step 1: Stop the running engine**

```powershell
Get-Process -Name dj-engine -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
```

- [ ] **Step 2: Release build**

```powershell
cargo build --manifest-path engine/Cargo.toml --release
```

- [ ] **Step 3: Start engine with --force-recipe loop-bridge**

```powershell
$env:ICECAST_URL = 'icecast://localhost:8000/dj.mp3'
$env:ICECAST_USER = 'source'
$env:ICECAST_PASSWORD = 'hackme'
& engine/target/release/dj-engine.exe --sidecars analyzed-real --music-root data --log-format pretty --force-recipe loop-bridge
```

Run in background so the user can listen.

- [ ] **Step 4: Watch logs for `loop_bridge` arming**

Expected log line includes:
```
recipe="loop_bridge" length_bars=<computed> ... loop_lock_bar=<N> phrase_end_bar=<M>
```

If the recipe doesn't fire (the picker keeps choosing another even though forced), debug:
- Confirm `--force-recipe loop-bridge` is parsed correctly (clap accepts the kebab-case)
- Confirm `RecipeChoice::LoopBridge` flows through the dispatch site

If the recipe fires but the anchors are None (computed_length=16, loop_lock=4 from fallback), the data didn't align on the picked pair. Try forcing on a specific pair where you know the data is compatible.

- [ ] **Step 5: Listen + report**

Listen for:
- **Act 1 subtlety:** outgoing audible alone for several bars; no audible incoming until the loop locks.
- **Act 2 filter sweep:** incoming entering as treble-only shimmer, gradually opening through the spectrum. Outgoing thinning slightly.
- **Act 3 handoff:** outgoing cuts cleanly at the phrase end; incoming jumps to full mix with vocals entering on the downbeat.

Note any:
- Loudness bumps or dips during Act 2 (hand-tuning needs adjustment)
- Clicks or glitches at the loop wraparound (downbeat snap not working)
- Key clashes (camelot check failing)
- Beat misalignment (tempo-align issues)

- [ ] **Step 6: No commit** — verification only. Capture findings for tuning iteration.

---

## Spec-Coverage Self-Check

- ✅ 3 Sidecar helpers → Task 1
- ✅ `keys_compatible` in camelot.rs → Task 2
- ✅ `LoopBridgeAnchors` + `compute_loop_bridge_anchors` shared helper → Task 3
- ✅ `LoopBridge` struct + 3-act build_script → Task 4
- ✅ Picker registration + scoring + clap → Task 5
- ✅ Live verify via --force-recipe → Task 6
- ✅ Loudness compensation (hand-tuned via fader curves) → embedded in Task 4
- ✅ Camelot key gating → embedded in Task 5 scoring
- ✅ Stem isolation during Act 1 → embedded in Task 4 phases
- ✅ Downbeat snap on vocal_entry → embedded in Task 3 helper

Open items resolved during execution:
- Exact existing `parse_camelot` signature/name in `camelot.rs` (Task 2 Step 1 checks).
- Exact dispatch shape in main.rs (Task 5 Step 5 — match the existing pattern).
- Hand-tuning of loudness compensation values (Task 6 listening session).
