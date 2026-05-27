//! TransitionScheduler: cues the incoming deck, builds automations from a
//! recipe, schedules them on both decks, returns a handle with the end time.

use crate::deck::DeckId;
use crate::error::Result;
use crate::ffmpeg::SAMPLE_RATE;
use crate::library::LoadedTrack;
use crate::mixer::Mixer;
use crate::recipe::{Recipe, RecipeOutput, TransitionContext};
use crate::transition::TransitionPlan;

const CUE_HEADROOM_BARS: f64 = 1.0;

pub struct TransitionHandle {
    pub end_master_clock_frames: u64,
    pub recipe_name: &'static str,
}

pub struct TransitionScheduler;

impl TransitionScheduler {
    pub async fn arm<R: Recipe>(
        mixer: &mut Mixer,
        outgoing_deck: DeckId,
        incoming_deck: DeckId,
        outgoing: &LoadedTrack,
        incoming: &LoadedTrack,
        plan: &TransitionPlan,
        recipe: &R,
    ) -> Result<TransitionHandle> {
        let outgoing_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let cue_headroom_seconds = CUE_HEADROOM_BARS * bar_seconds;
        let cue_start = (plan.incoming_crossfade_start_seconds - cue_headroom_seconds).max(0.0);

        // 1) Cue the incoming deck at the BPM-match tempo so its decoder
        //    actually time-stretches and its source-position tracking stays
        //    consistent with the playback rate.
        tracing::info!(
            deck = ?incoming_deck,
            track = %incoming
                .sidecar
                .source_path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&incoming.sidecar.source_path),
            cue_source_seconds = cue_start,
            tempo = ratio,
            cue_headroom_seconds,
            "scheduler cueing incoming"
        );
        mixer
            .deck(incoming_deck)
            .cue(incoming.clone(), cue_start, ratio)
            .await?;
        mixer.deck(incoming_deck).play();

        // 2) Compute t_start: master_clock + (outgoing_outro_start - outgoing's
        //    current source position) * SAMPLE_RATE. The main loop is responsible
        //    for sequencing so the outgoing deck reaches outro_start at
        //    `master_clock_frames`.
        let master_clock = mixer.master_clock_frames();
        let t_start = master_clock;

        // 3) Build automations.
        let ctx = TransitionContext {
            outgoing,
            incoming,
            plan,
            master_clock_frames: t_start,
            outgoing_deck,
            incoming_deck,
            sample_rate: SAMPLE_RATE,
        };
        let RecipeOutput {
            outgoing_automations,
            incoming_automations,
            total_duration_frames,
        } = recipe.build(&ctx);

        // 4) Schedule on the respective decks.
        for a in outgoing_automations {
            mixer.deck(outgoing_deck).schedule(a);
        }
        for a in incoming_automations {
            mixer.deck(incoming_deck).schedule(a);
        }

        Ok(TransitionHandle {
            end_master_clock_frames: t_start + total_duration_frames,
            recipe_name: recipe.name(),
        })
    }
}
