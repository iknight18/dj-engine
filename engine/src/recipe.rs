//! Recipe layer: named transition styles compile to lists of Automation primitives.

use crate::automation::Automation;
use crate::library::LoadedTrack;
use crate::transition::TransitionPlan;

pub use crate::deck::DeckId;

pub struct TransitionContext<'a> {
    pub outgoing: &'a LoadedTrack,
    pub incoming: &'a LoadedTrack,
    pub plan: &'a TransitionPlan,
    pub master_clock_frames: u64,
    pub outgoing_deck: DeckId,
    pub incoming_deck: DeckId,
    pub sample_rate: u32,
    /// Precomputed ratio used by recipes. Reflects local section BPM when
    /// available, else global. Logged as `ratio_local`.
    pub ratio: f64,
    /// The old global-BPM ratio, kept for observability logging.
    pub ratio_global: f64,
}

/// Static per-deck effect configuration applied by the scheduler after `cue()`
/// and before `play()`. Fields are `Option` so a recipe can configure only the
/// effects it actually uses (`LoopOut` configures the loop buffer). Existing
/// recipes that don't use effects set both `outgoing_init` and `incoming_init`
/// to `None`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DeckInitConfig {
    pub echo_delay_seconds: Option<f64>,
    pub echo_feedback: Option<f64>,
    pub loop_bars: Option<usize>,
    pub loop_bar_seconds: Option<f64>,
}

pub struct RecipeOutput {
    pub outgoing_automations: Vec<Automation>,
    pub incoming_automations: Vec<Automation>,
    pub outgoing_init: Option<DeckInitConfig>,
    pub incoming_init: Option<DeckInitConfig>,
    pub total_duration_frames: u64,
}

/// Result of computing both the new (local-section) and old (global) ratios
/// for a transition. The scheduler stashes both on TransitionContext and logs
/// them at arming.
#[derive(Clone, Copy, Debug)]
pub struct SectionRatio {
    pub ratio_local: f64,
    pub ratio_global: f64,
}

/// Compute the time-stretch ratio from local section BPMs with global fallback.
///
/// - When both section BPMs are `Some`, the local ratio is `outgoing / incoming`.
/// - When either is `None`, falls back to the global ratio for the local field.
/// - The global field is always `outgoing_global / incoming_global`.
pub fn compute_section_ratio(
    outgoing_section_bpm: Option<f64>,
    outgoing_global_bpm: f64,
    incoming_section_bpm: Option<f64>,
    incoming_global_bpm: f64,
) -> SectionRatio {
    let ratio_global = outgoing_global_bpm / incoming_global_bpm.max(1.0);
    let ratio_local = match (outgoing_section_bpm, incoming_section_bpm) {
        (Some(o), Some(i)) => o / i.max(1.0),
        _ => ratio_global,
    };
    SectionRatio {
        ratio_local,
        ratio_global,
    }
}

/// Pre-computed anchors for the LoopBridge recipe. Both the scorer (to decide
/// whether to fire) and the build_script (to place phases) consume this.
/// Returns None when required sidecar data is missing.
#[derive(Clone, Copy, Debug)]
pub struct LoopBridgeAnchors {
    /// Outgoing source-time where the last phrase ends (clamped to vocal_end safety).
    pub phrase_end_seconds: f64,
    /// Transition-local bar of the phrase end (where Act 3 fires).
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

pub trait Recipe {
    fn name(&self) -> &'static str;
    fn build_script(&self, ctx: &TransitionContext<'_>) -> crate::recipe_script::TransitionScript;
    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        crate::recipe_script::compile(&self.build_script(ctx), ctx)
    }

    /// True iff this recipe needs both decks loaded in stems mode (4 separate
    /// audio buffers per deck). Default `false` — existing recipes use the
    /// mixed source. The scheduler reads this and dispatches `cue_stems`
    /// instead of `cue` for incoming, so per-stem fader automations have an
    /// effect.
    fn requires_stems(&self) -> bool {
        false
    }
}

/// Build a script using the given recipe, inject any automatic post-processing
/// phases (currently: pitch-journey-back), then compile to a `RecipeOutput`.
pub fn build_and_compile(recipe: &dyn Recipe, ctx: &TransitionContext<'_>) -> RecipeOutput {
    let mut script = recipe.build_script(ctx);
    inject_pitch_journey(&mut script, ctx);
    crate::recipe_script::compile(&script, ctx)
}

/// Inject a pitch-journey-back `TempoRamp` into `script` when the pair has a
/// modest BPM shift (0.005 ≤ |ratio − 1.0| < 0.05) and `skip_tempo_align` is
/// false. The ramp runs from `ratio` back to 1.0 over 32 bars after the
/// transition window, so the incoming deck gradually returns to its natural BPM.
///
/// Calling this function multiple times on the same script is idempotent: if a
/// `TempoRamp` for `DeckRole::Incoming` already exists, the function returns
/// immediately without adding another.
pub fn inject_pitch_journey(
    script: &mut crate::recipe_script::TransitionScript,
    ctx: &TransitionContext<'_>,
) {
    use crate::automation::Curve;
    use crate::recipe_script::{bars, DeckRole, Phase};

    if ctx.plan.skip_tempo_align {
        return;
    }

    // Skip if a journey ramp already exists (idempotency).
    let already_injected = script.phases.iter().any(|p| {
        matches!(
            p,
            Phase::TempoRamp {
                deck: DeckRole::Incoming,
                ..
            }
        )
    });
    if already_injected {
        return;
    }

    // Find the TempoAlign on Incoming; its `ratio` is what we journey back from.
    let ratio = script.phases.iter().find_map(|p| match p {
        Phase::TempoAlign {
            deck: DeckRole::Incoming,
            ratio,
        } => Some(*ratio),
        _ => None,
    });
    let Some(ratio) = ratio else {
        return;
    };

    let delta = (ratio - 1.0).abs();
    if !(0.005..0.05).contains(&delta) {
        return;
    }

    let l = script.length_bars as f64;
    script.phases.push(Phase::TempoRamp {
        deck: DeckRole::Incoming,
        from: ratio,
        to: 1.0,
        span: bars(l, l + 32.0),
        curve: Curve::Linear,
    });
}

use crate::automation::Curve;
use crate::recipe_script::{bar, bars, DeckRole, Phase, TransitionScript};

pub struct EqualPowerCrossfade;

impl Recipe for EqualPowerCrossfade {
    fn name(&self) -> &'static str {
        "equal_power_crossfade"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let ratio = ctx.ratio;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

pub struct HardCut;

impl Recipe for HardCut {
    fn name(&self) -> &'static str {
        "hard_cut"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let ratio = ctx.ratio;

        let mut phases = vec![
            Phase::Fader {
                deck: DeckRole::Outgoing,
                from: 1.0,
                to: 0.0,
                span: bars(0.0, 0.0),
                curve: Curve::Step,
            },
            Phase::Fader {
                deck: DeckRole::Incoming,
                from: 0.0,
                to: 1.0,
                span: bars(0.0, 0.0),
                curve: Curve::Step,
            },
        ];
        if !ctx.plan.skip_tempo_align {
            phases.push(Phase::TempoAlign {
                deck: DeckRole::Incoming,
                ratio,
            });
        }

        TransitionScript {
            length_bars: 0,
            phases,
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `FilterFade`: incoming track filters in from the top (treble first).
/// outgoing.Fader equal-power fade-out; incoming.Fader equal-power fade-in;
/// incoming.Filter sweeps from +1.0 (full HP, treble only) to 0.0 (bypass)
/// across the full crossfade window. Outgoing has no filter automation.
pub struct FilterFade;

impl Recipe for FilterFade {
    fn name(&self) -> &'static str {
        "filter_fade"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let ratio = ctx.ratio;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::Linear,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `FilterKill`: outgoing filters out to silence (HP sweep) before fading.
/// outgoing.Filter sweeps 0 -> +1 over the first 60% of the crossfade;
/// outgoing.Fader equal-power fade-out over the full crossfade;
/// incoming.Fader equal-power fade-in over the full crossfade (no filter).
pub struct FilterKill;

impl Recipe for FilterKill {
    fn name(&self) -> &'static str {
        "filter_kill"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let ratio = ctx.ratio;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Filter {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, 0.6 * l),
                    curve: Curve::Linear,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `LoopOut`: outgoing locks its last few bars into a loop and rides that loop
/// to the end while the incoming track equal-power fades in over the top. A
/// LoopGate Step pins the loop at t0; another Step releases it at t1 so the
/// deck unloads cleanly on the post-transition swap. Loop length is 4 bars at
/// the outgoing track's tempo.
pub struct LoopOut;

impl Recipe for LoopOut {
    fn name(&self) -> &'static str {
        "loop_out"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let ratio = ctx.ratio;
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::LoopGate {
                    deck: DeckRole::Outgoing,
                    on_at: bar(0.0),
                    off_at: bar(l),
                },
            ],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: None,
                echo_feedback: None,
                loop_bars: Some(4),
                loop_bar_seconds: Some(bar_seconds),
            }),
            incoming_init: None,
        }
    }
}

/// `Drop`: incoming plays for 4 bars under a high-pass filter (treble-only
/// build-up), then hard-cuts to full bandwidth on the next downbeat while
/// outgoing fades to silence. The HP-filter sweeps from 0.85 → 0.55 over the
/// build, giving the listener a "riser" — gradually revealing the mid-band —
/// followed by a hard release. Outgoing plays untouched until bar 4.
pub struct Drop;

impl Recipe for Drop {
    fn name(&self) -> &'static str {
        "drop"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let ratio = ctx.ratio;
        let l = 4.0;

        TransitionScript {
            length_bars: 4,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.7,
                    to: 0.7,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.7,
                    to: 1.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.85,
                    to: 0.85,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.85,
                    to: 0.55,
                    span: bars(0.0, l),
                    curve: Curve::Linear,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `RhythmSwap`: stems-aware transition. Two-phase gesture:
/// 1. Bars [0, L/2]: subtle rhythm reveal. Outgoing's drums + bass fade out;
///    incoming's drums + bass fade in simultaneously. Outgoing's vocals + other
///    play on top throughout (incoming's vocals + other held muted).
/// 2. Bar L/2 (phrase boundary): fast melody handoff. Outgoing's vocals + other
///    Step → 0; incoming's vocals + other Step → 1. The rhythm section has
///    already become incoming's, so the melody handoff feels like an arrival.
///
/// Only ONE melody is audible at any time. Avoids the layering-clash failure
/// mode by construction.
///
/// Replaces both `BassSwap` (EQ-based) and `BassSwapStemAware` (bass-only) in
/// the catalog consolidation.
pub struct RhythmSwap;

impl Recipe for RhythmSwap {
    fn name(&self) -> &'static str {
        "rhythm_swap"
    }

    fn requires_stems(&self) -> bool {
        true
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        use crate::deck::StemKind;

        let ratio = ctx.ratio;
        let l = ctx.plan.length_bars as f64;
        let swap_bar = l / 2.0;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(swap_bar, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Drums,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, swap_bar),
                    curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Bass,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, swap_bar),
                    curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Drums,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, swap_bar),
                    curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Bass,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, swap_bar),
                    curve: Curve::Linear,
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
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Vocals,
                    from: 0.0,
                    to: 0.0,
                    span: bars(swap_bar, swap_bar),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Other,
                    from: 0.0,
                    to: 0.0,
                    span: bars(swap_bar, swap_bar),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Vocals,
                    from: 1.0,
                    to: 1.0,
                    span: bars(swap_bar, swap_bar),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Other,
                    from: 1.0,
                    to: 1.0,
                    span: bars(swap_bar, swap_bar),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `LoopBridge`: stems-aware three-act transition. See
/// `docs/superpowers/specs/2026-05-30-loop-bridge-recipe-design.md` for the
/// full design rationale.
///
/// Act 1 (bars 0 -> loop_lock_bar): incoming silent (fader=0); source pulls
/// into loop buffer with stems = {drums, bass, vocals=0, other=0} so the
/// buffer captures rhythm-only content.
///
/// Act 2 (loop_lock_bar -> phrase_end_bar-1): LoopGate ON. Incoming filter
/// sweeps 0.85 -> 0 (rising rhythm reveal). Outgoing filter sweeps 0 -> 0.6
/// (gentle thinning). Incoming fader 0 -> 0.85 EqualPowerIn over first 4 bars.
/// Outgoing fader 1.0 -> 0.55 Linear (loudness compensation).
///
/// Act 3 (at phrase_end_bar): LoopGate OFF -> incoming source resumes from
/// vocal_entry_snapped. Outgoing fader Step -> 0. Incoming fader Step -> 1.0.
/// Incoming.{vocals, other} stems restored. Both filters Step -> 0.
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

        // Compute anchors via the shared helper. Fall back to safe defaults
        // if data is missing (the recipe shouldn't have been picked in that
        // case, but defend anyway).
        let anchors = compute_loop_bridge_anchors(
            &ctx.outgoing.sidecar,
            &ctx.incoming.sidecar,
            ctx.plan,
        );
        let (loop_lock_bar, phrase_end_bar, computed_length, anchors_used) = match anchors {
            // Accept anchors only when:
            //   - computed length is meaningful
            //   - loop_lock_bar >= 4 (buffer needs 4 bars to fill — see loop_bars below)
            //   - Act 2 has at least 4 bars after the lock for the filter crossfade
            // Otherwise fall back to safe defaults.
            Some(a) if a.computed_length_bars >= 8.0
                && a.loop_lock_bar >= 4.0
                && (a.phrase_end_bar - a.loop_lock_bar) >= 4.0 =>
                (a.loop_lock_bar, a.phrase_end_bar, a.computed_length_bars, "data_driven"),
            _ => (4.0, 16.0, 16.0, "fallback"),
        };

        tracing::info!(
            recipe = "loop_bridge",
            anchors_used,
            loop_lock_bar,
            phrase_end_bar,
            computed_length_bars = computed_length,
            bar_seconds,
            loop_bars = 4,
            "LoopBridge anchors"
        );

        TransitionScript {
            length_bars: computed_length as usize,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },

                // === Act 1: loop preparation ===
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
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

                Phase::LoopGate {
                    deck: DeckRole::Incoming,
                    on_at: bar(loop_lock_bar),
                    off_at: bar(phrase_end_bar),
                },

                // === Act 2: filter crossfade ===
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.85,
                    span: bars(loop_lock_bar, loop_lock_bar + 4.0),
                    curve: Curve::EqualPowerIn,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.55,
                    span: bars(loop_lock_bar, phrase_end_bar - 1.0),
                    curve: Curve::Linear,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.85,
                    to: 0.0,
                    span: bars(loop_lock_bar, phrase_end_bar - 1.0),
                    curve: Curve::Linear,
                },
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeChoice {
    EqualPower,
    HardCut,
    FilterFade,
    FilterKill,
    LoopOut,
    Drop,
    RhythmSwap,
    LoopBridge,
}

/// Legacy plan-only picker. Kept for tests and as a reference baseline.
pub fn pick_recipe_plan_only(plan: &TransitionPlan) -> RecipeChoice {
    if plan.length_bars == 0 {
        RecipeChoice::HardCut
    } else if plan.use_eq_swap {
        RecipeChoice::RhythmSwap
    } else {
        RecipeChoice::EqualPower
    }
}

/// Like `pick_recipe`, but also returns the score for every candidate (after
/// recency penalty and jitter). For logging at transition-arming time.
pub fn pick_recipe_with_scores(
    plan: &TransitionPlan,
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    recency: &crate::recipe_recency::RecipeRecency,
) -> (RecipeChoice, Vec<(RecipeChoice, f64)>) {
    if plan.length_bars == 0 {
        return (RecipeChoice::HardCut, vec![(RecipeChoice::HardCut, 1.0)]);
    }
    let inputs = PairScoreInputs::from_pair(plan, outgoing, incoming);
    let candidates = [
        RecipeChoice::EqualPower,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::LoopOut,
        RecipeChoice::Drop,
        RecipeChoice::RhythmSwap,
        RecipeChoice::LoopBridge,
    ];
    let jitter_range = 0.04;
    let mut scored: Vec<(RecipeChoice, f64)> = Vec::with_capacity(candidates.len());
    let mut best = (RecipeChoice::EqualPower, f64::MIN);
    for (idx, choice) in candidates.iter().enumerate() {
        let mut score = score_recipe(*choice, &inputs);
        if recency.contains(*choice) {
            score *= 0.5;
        }
        let salt = (idx as u64)
            .wrapping_mul(2654435761)
            .wrapping_add(recency_hash(recency));
        let jitter = (salt % 1000) as f64 / 1000.0 * jitter_range;
        score += jitter;
        scored.push((*choice, score));
        if score > best.1 {
            best = (*choice, score);
        }
    }
    (best.0, scored)
}

/// Scored picker: ranks all candidate recipes against the pair's properties and a
/// recency penalty. Hard-cut shortcuts when `plan.length_bars == 0`.
pub fn pick_recipe(
    plan: &TransitionPlan,
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    recency: &crate::recipe_recency::RecipeRecency,
) -> RecipeChoice {
    if plan.length_bars == 0 {
        return RecipeChoice::HardCut;
    }
    let inputs = PairScoreInputs::from_pair(plan, outgoing, incoming);
    let candidates = [
        RecipeChoice::EqualPower,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::LoopOut,
        RecipeChoice::Drop,
        RecipeChoice::RhythmSwap,
        RecipeChoice::LoopBridge,
    ];

    let mut best = (RecipeChoice::EqualPower, f64::MIN);
    let jitter_range = 0.04;
    for (idx, choice) in candidates.iter().enumerate() {
        let mut score = score_recipe(*choice, &inputs);
        if recency.contains(*choice) {
            score *= 0.5;
        }
        // Deterministic jitter that varies per call: hash the recency state
        // mixed with the candidate index. Stable for tests with a fixed
        // recency state.
        let salt = (idx as u64)
            .wrapping_mul(2654435761)
            .wrapping_add(recency_hash(recency));
        let jitter = (salt % 1000) as f64 / 1000.0 * jitter_range;
        score += jitter;
        if score > best.1 {
            best = (*choice, score);
        }
    }
    best.0
}

fn recipe_choice_id(c: RecipeChoice) -> u64 {
    // Stable, ordered id used in the deterministic-jitter mix below.
    match c {
        RecipeChoice::EqualPower => 1,
        RecipeChoice::HardCut => 2,
        RecipeChoice::FilterFade => 4,
        RecipeChoice::FilterKill => 5,
        RecipeChoice::LoopOut => 7,
        RecipeChoice::Drop => 11,
        RecipeChoice::RhythmSwap => 17,
        RecipeChoice::LoopBridge => 18,
    }
}

fn recency_hash(recency: &crate::recipe_recency::RecipeRecency) -> u64 {
    // Fold the recency's current contents into a u64. Since RecipeRecency
    // doesn't expose iteration, mix in the "contains" result for each choice.
    let mut h: u64 = 0x9e3779b97f4a7c15;
    for c in [
        RecipeChoice::EqualPower,
        RecipeChoice::HardCut,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::LoopOut,
        RecipeChoice::Drop,
        RecipeChoice::RhythmSwap,
        RecipeChoice::LoopBridge,
    ] {
        let bit = if recency.contains(c) { 1 } else { 0 };
        h = h
            .wrapping_mul(0x100000001b3)
            .wrapping_add(recipe_choice_id(c) * 31 + bit);
    }
    h
}

/// Pre-computed inputs the recipe scorer needs. Factoring out the sidecar
/// dereference makes the scorer trivially unit-testable.
#[derive(Clone, Debug)]
pub struct PairScoreInputs {
    pub length_bars: usize,
    pub use_eq_swap: bool,
    /// `incoming.energy_mean - outgoing.energy_mean` (positive = ramp up).
    pub energy_delta: f64,
    /// `band_energy_means.bass` from each sidecar; defaults to 0.5 when absent.
    pub outgoing_bass: f64,
    pub incoming_bass: f64,
    /// True iff BOTH tracks are marked `is_phrase_aligned`.
    pub phrase_aligned: bool,
    /// True iff BOTH sidecars carry a `stems` block (drum/bass/vocal density,
    /// vocal segments). Stem-aware recipes refuse to fire without this.
    pub stems_available: bool,
    /// Mean of the per-bar drum_density array for outgoing. 0.0 when stems absent.
    pub outgoing_drum_density_mean: f64,
    /// Mean of the per-bar drum_density array for incoming. 0.0 when stems absent.
    pub incoming_drum_density_mean: f64,
    /// Pre-computed LoopBridge anchors. Some(...) when all required sidecar
    /// data is present; None when missing. Used by the LoopBridge scoring branch.
    pub loop_bridge_anchors: Option<LoopBridgeAnchors>,
}

impl PairScoreInputs {
    pub fn from_pair(
        plan: &TransitionPlan,
        outgoing: &LoadedTrack,
        incoming: &LoadedTrack,
    ) -> Self {
        let outgoing_energy = outgoing.sidecar.energy_mean.unwrap_or(0.5);
        let incoming_energy = incoming.sidecar.energy_mean.unwrap_or(0.5);
        let outgoing_bass = outgoing
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);
        let incoming_bass = incoming
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);
        let phrase_aligned = outgoing.sidecar.phrase_aligned() && incoming.sidecar.phrase_aligned();
        let stems_available = outgoing.sidecar.stems.is_some() && incoming.sidecar.stems.is_some();
        let outgoing_drum_density_mean = outgoing
            .sidecar
            .stems
            .as_ref()
            .map(|s| {
                if s.drum_density.is_empty() {
                    0.0
                } else {
                    s.drum_density.iter().sum::<f64>() / s.drum_density.len() as f64
                }
            })
            .unwrap_or(0.0);
        let incoming_drum_density_mean = incoming
            .sidecar
            .stems
            .as_ref()
            .map(|s| {
                if s.drum_density.is_empty() {
                    0.0
                } else {
                    s.drum_density.iter().sum::<f64>() / s.drum_density.len() as f64
                }
            })
            .unwrap_or(0.0);
        let loop_bridge_anchors = compute_loop_bridge_anchors(
            &outgoing.sidecar,
            &incoming.sidecar,
            plan,
        );
        Self {
            length_bars: plan.length_bars,
            use_eq_swap: plan.use_eq_swap,
            energy_delta: incoming_energy - outgoing_energy,
            outgoing_bass,
            incoming_bass,
            phrase_aligned,
            stems_available,
            outgoing_drum_density_mean,
            incoming_drum_density_mean,
            loop_bridge_anchors,
        }
    }
}

/// Score a recipe's fit for the pair. Higher = better. Score range [0, 1].
pub fn score_recipe(choice: RecipeChoice, inputs: &PairScoreInputs) -> f64 {
    match choice {
        RecipeChoice::HardCut => {
            if inputs.length_bars == 0 {
                1.0
            } else {
                0.0
            }
        }
        RecipeChoice::EqualPower => 0.10,
        RecipeChoice::FilterFade => {
            // Always-selectable default crossfade. Higher score on energy-up,
            // but never falls to 0 — beats EqualPower's 0.10 floor.
            if inputs.energy_delta > 0.20 {
                0.85
            } else if inputs.energy_delta > 0.05 {
                0.70
            } else if inputs.energy_delta > -0.05 {
                0.55
            } else {
                0.30
            }
        }
        RecipeChoice::FilterKill => {
            if inputs.energy_delta < -0.20 {
                0.70
            } else if inputs.energy_delta < -0.05 {
                0.40
            } else {
                0.0
            }
        }
        RecipeChoice::LoopOut => 0.30,
        RecipeChoice::Drop => {
            let big_ramp = inputs.energy_delta > 0.20 && inputs.phrase_aligned;
            let modest_ramp = inputs.energy_delta > 0.10 && inputs.phrase_aligned;
            if big_ramp {
                0.75
            } else if modest_ramp {
                0.40
            } else {
                0.10
            }
        }
        RecipeChoice::RhythmSwap => {
            let drum_density_delta =
                (inputs.outgoing_drum_density_mean - inputs.incoming_drum_density_mean).abs();
            let drums_heavy =
                inputs.outgoing_drum_density_mean > 0.5 && inputs.incoming_drum_density_mean > 0.5;
            let drums_similar = drum_density_delta < 0.15;
            let bass_heavy = inputs.outgoing_bass > 0.5 && inputs.incoming_bass > 0.5;
            let energy_moderate = inputs.energy_delta.abs() < 0.20;
            let layering_long_enough = inputs.length_bars >= 8;
            let strong = inputs.stems_available
                && inputs.phrase_aligned
                && layering_long_enough
                && drums_heavy
                && drums_similar
                && energy_moderate;
            if strong && bass_heavy {
                0.92
            } else if strong {
                0.75
            } else if inputs.stems_available && layering_long_enough && drums_heavy {
                0.40
            } else if inputs.stems_available && layering_long_enough {
                0.20
            } else {
                0.05
            }
        }
        RecipeChoice::LoopBridge => {
            let Some(a) = inputs.loop_bridge_anchors else { return 0.05; };
            if !inputs.stems_available || !inputs.phrase_aligned {
                return 0.05;
            }
            let loop_lock_ok = a.loop_lock_bar >= 4.0;
            let act2_long_enough = (a.phrase_end_bar - a.loop_lock_bar) >= 8.0;
            let length_in_range =
                a.computed_length_bars >= 16.0 && a.computed_length_bars <= 32.0;
            if loop_lock_ok && act2_long_enough && length_in_range && a.keys_compatible {
                0.90
            } else {
                0.05
            }
        }
    }
}
