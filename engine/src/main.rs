use anyhow::Context;
use clap::Parser;
use rand::seq::SliceRandom;
use tracing::{info, warn};

use dj_engine::buffer::{make_ring, Consumer};
use dj_engine::config::{Cli, IcecastEnv, LogFormat};
use dj_engine::energy_avg::EnergyTarget;
use dj_engine::ffmpeg::encoder::EncoderConfig;
use dj_engine::ffmpeg::SAMPLE_RATE;
use dj_engine::icecast_meta::{self, MetadataConfig};
use dj_engine::library::{load_library, LoadedTrack};
use dj_engine::logging_setup;
use dj_engine::recency::RecencyRing;
use dj_engine::recipe_recency::RecipeRecency;
use dj_engine::selector::Selector;
use dj_engine::supervisor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    logging_setup::init(matches!(cli.log_format, LogFormat::Json));

    let ice = IcecastEnv::from_env().map_err(|e| anyhow::anyhow!("config: {e}"))?;

    info!(sidecars = %cli.sidecars.display(), "loading library");
    let library = load_library(&cli.sidecars).context("load_library")?;
    info!(count = library.len(), "library loaded");

    let encoder_cfg = EncoderConfig {
        icecast_url: ice.url.clone(),
        user: ice.user.clone(),
        password: ice.password.clone(),
        format: ice.format,
        bitrate: ice.bitrate.clone(),
    };
    let metadata_cfg = MetadataConfig {
        admin_url: ice.admin_url.clone(),
        admin_user: ice.admin_user.clone(),
        admin_password: ice.admin_password.clone(),
        mount: ice.mount.clone(),
    };

    let encoder = supervisor::spawn_with_retries(&encoder_cfg).await?;

    // 10 second ring buffer.
    let ring_capacity = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, mut consumer) = make_ring(ring_capacity);

    // Take stdin out of the encoder for the writer task.
    // The encoder child is kept alive by the moved struct; we drop its handle below
    // intentionally — encoder stdout/stderr piping is handled by ffmpeg itself.
    let writer_stdin = encoder.stdin;
    let _encoder_child = encoder.child; // keep child alive until end of main

    // Writer task: ring -> encoder stdin.
    let writer_task = tokio::spawn(async move {
        let mut writer = writer_stdin;
        let mut buf = vec![0f32; 4096];
        use tokio::io::AsyncWriteExt;
        loop {
            let n = consumer.pop_slice(&mut buf);
            if n == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
            }
            let mut bytes = Vec::with_capacity(n * 4);
            for s in &buf[..n] {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            if let Err(e) = writer.write_all(&bytes).await {
                warn!(error = %e, "writer error; exiting writer task");
                break;
            }
        }
    });

    // Seed track
    let seed = pick_seed(&library, cli.seed_track.as_deref());
    info!(seed = %seed.sidecar.source_path, "seed track");

    // Mixer with two decks. Cue + play the seed on Deck A.
    // Eager stems mode: try cue_stems first so stem-aware recipes on later
    // transitions can manipulate this track's stems when it becomes outgoing.
    // Falls back to mixed-source cue when stem files are unavailable.
    let mut mixer = dj_engine::mixer::Mixer::new(producer);
    {
        let cue_in = seed.sidecar.cue_in_seconds.unwrap_or(0.0);
        let deck = mixer.deck(dj_engine::deck::DeckId::A);
        if seed.sidecar.stems.is_some() {
            match deck.cue_stems(seed.clone(), cue_in, 1.0).await {
                Ok(()) => {
                    info!(track = %short_name(&seed.sidecar.source_path), "seed cued in stems mode")
                }
                Err(e) => {
                    warn!(error = %e, "cue_stems failed for seed; falling back to mixed");
                    deck.cue(seed.clone(), cue_in, 1.0).await?;
                }
            }
        } else {
            deck.cue(seed.clone(), cue_in, 1.0).await?;
        }
        deck.play();
        // v1: poke fader to unity directly so the seed audio is audible from
        // tick 0. Cleaner alternative: schedule a Fader Step 0->1 at frame 0
        // on Deck A. Acceptable code smell pending later cleanup.
        deck.test_set_fader(1.0);
    }

    let mut current = seed;
    let mut current_deck = dj_engine::deck::DeckId::A;
    let mut other_deck = dj_engine::deck::DeckId::B;

    let mut recency = RecencyRing::new(20);
    recency.record(current.sidecar.source_path.clone());
    let recipe_recency_window = cli.recipe_recency_window.unwrap_or(5);
    let mut recipe_recency = RecipeRecency::new(recipe_recency_window);
    let mut energy = EnergyTarget::new(current.sidecar.energy_mean.unwrap_or(0.5));
    let http = reqwest::Client::new();
    icecast_meta::update_now_playing(&http, &metadata_cfg, &current.sidecar.source_path).await;

    loop {
        let next_track = {
            let mut sel = Selector::new(&library, &recency, energy.current());
            match sel.pick_next(&current) {
                Some(t) => t.clone(),
                None => {
                    warn!("selector returned nothing; ending set");
                    break;
                }
            }
        };
        let current_vocal_segs = current
            .sidecar
            .stems
            .as_ref()
            .map(|s| s.vocal_segments.len())
            .unwrap_or(0);
        let current_drum_density_mean = current
            .sidecar
            .stems
            .as_ref()
            .map(|s| mean_or_zero(&s.drum_density))
            .unwrap_or(0.0);
        let next_vocal_segs = next_track
            .sidecar
            .stems
            .as_ref()
            .map(|s| s.vocal_segments.len())
            .unwrap_or(0);
        let next_drum_density_mean = next_track
            .sidecar
            .stems
            .as_ref()
            .map(|s| mean_or_zero(&s.drum_density))
            .unwrap_or(0.0);
        info!(
            current = %short_name(&current.sidecar.source_path),
            current_bpm = current.sidecar.bpm.unwrap_or(0.0),
            current_key = current.sidecar.key.as_deref().unwrap_or("?"),
            current_energy = current.sidecar.energy_mean.unwrap_or(0.0),
            current_bass = current.sidecar.band_energy_means.as_ref().map(|b| b.bass).unwrap_or(0.0),
            current_phrase_aligned = current.sidecar.phrase_aligned(),
            current_vocal_segments = current_vocal_segs,
            current_drum_density_mean,
            next = %short_name(&next_track.sidecar.source_path),
            next_bpm = next_track.sidecar.bpm.unwrap_or(0.0),
            next_key = next_track.sidecar.key.as_deref().unwrap_or("?"),
            next_energy = next_track.sidecar.energy_mean.unwrap_or(0.0),
            next_bass = next_track.sidecar.band_energy_means.as_ref().map(|b| b.bass).unwrap_or(0.0),
            next_phrase_aligned = next_track.sidecar.phrase_aligned(),
            next_vocal_segments = next_vocal_segs,
            next_drum_density_mean,
            "selector picked"
        );

        let plan = dj_engine::transition::plan_transition(&current.sidecar, &next_track.sidecar);
        let planned_type = transition_type_label(plan.length_bars, plan.use_eq_swap);
        info!(
            from = %short_name(&current.sidecar.source_path),
            to = %short_name(&next_track.sidecar.source_path),
            planned_type,
            length_bars = plan.length_bars,
            use_eq_swap = plan.use_eq_swap,
            "transition plan"
        );

        // Play current track until its outro_start. The deck's actual source
        // position may NOT be at cue_in (it likely entered as incoming in the
        // previous transition, cued at intro_end - 1 bar), and its tempo may
        // not be 1.0 (cued at the previous BPM match). Compute the output
        // frames remaining from where the deck actually is.
        let outro_start = plan.outgoing_crossfade_start_seconds;
        let current_source = mixer.deck(current_deck).source_position_seconds();
        let tempo = mixer.deck(current_deck).state().tempo.max(0.001);
        let frames_until_outro = (((outro_start - current_source).max(0.0)
            * dj_engine::ffmpeg::SAMPLE_RATE as f64)
            / tempo) as u64;
        let outro_deadline = mixer.master_clock_frames() + frames_until_outro;
        info!(
            track = %short_name(&current.sidecar.source_path),
            deck = ?current_deck,
            source_pos_seconds = current_source,
            outro_start_seconds = outro_start,
            tempo,
            wait_seconds = frames_until_outro as f64 / dj_engine::ffmpeg::SAMPLE_RATE as f64,
            "playing master deck until outro_start"
        );
        dj_engine::mixer::tick_until(&mut mixer, outro_deadline).await?;

        // Arm the transition. If --force-recipe was passed, override
        // `pick_recipe`'s natural choice and log the divergence so the operator
        // can see they're not seeing planner behavior.
        let (natural_choice, scored) = dj_engine::recipe::pick_recipe_with_scores(
            &plan,
            &current,
            &next_track,
            &recipe_recency,
        );
        let choice = cli
            .force_recipe
            .map(|fr| fr.to_recipe_choice())
            .unwrap_or(natural_choice);
        if cli.force_recipe.is_some() && choice != natural_choice {
            info!(
                forced = ?choice,
                natural = ?natural_choice,
                "force_recipe override active"
            );
        }
        let recipe_name: &str = match choice {
            dj_engine::recipe::RecipeChoice::EqualPower => "equal_power_crossfade",
            dj_engine::recipe::RecipeChoice::HardCut => "hard_cut",
            dj_engine::recipe::RecipeChoice::FilterFade => "filter_fade",
            dj_engine::recipe::RecipeChoice::FilterKill => "filter_kill",
            dj_engine::recipe::RecipeChoice::LoopOut => "loop_out",
            dj_engine::recipe::RecipeChoice::Drop => "drop",
            dj_engine::recipe::RecipeChoice::RhythmSwap => "rhythm_swap",
            dj_engine::recipe::RecipeChoice::LoopBridge => "loop_bridge",
        };
        let outgoing_global_bpm = current.sidecar.bpm.unwrap_or(120.0);
        let incoming_global_bpm = next_track.sidecar.bpm.unwrap_or(outgoing_global_bpm);
        // Mirror scheduler::arm's local-section averaging window so the log
        // shows the same ratio the scheduler will actually use.
        let bar_seconds_log = (60.0 / outgoing_global_bpm) * 4.0;
        let length_seconds_log = (plan.length_bars as f64) * bar_seconds_log;
        let outgoing_section_bpm = current.sidecar.local_bpm_in_window(
            plan.outgoing_crossfade_start_seconds,
            plan.outgoing_crossfade_start_seconds + length_seconds_log,
        );
        let incoming_section_bpm = next_track.sidecar.local_bpm_in_window(
            plan.incoming_crossfade_start_seconds,
            plan.incoming_crossfade_start_seconds + length_seconds_log,
        );
        let section_ratio = dj_engine::recipe::compute_section_ratio(
            outgoing_section_bpm,
            outgoing_global_bpm,
            incoming_section_bpm,
            incoming_global_bpm,
        );
        let ratio = section_ratio.ratio_local;
        let transition_start_master = mixer.master_clock_frames();
        let transition_type = transition_type_label(plan.length_bars, plan.use_eq_swap);
        // Compact score string: "equal_power=0.51 rhythm_swap=0.84 ..."
        let scores_str: String = scored
            .iter()
            .map(|(c, s)| format!("{}={:.2}", recipe_choice_name(*c), s))
            .collect::<Vec<_>>()
            .join(" ");
        info!(
            outgoing = %short_name(&current.sidecar.source_path),
            incoming = %short_name(&next_track.sidecar.source_path),
            outgoing_deck = ?current_deck,
            incoming_deck = ?other_deck,
            transition_type,
            recipe = recipe_name,
            length_bars = plan.length_bars,
            use_eq_swap = plan.use_eq_swap,
            ratio_local = section_ratio.ratio_local,
            ratio_global = section_ratio.ratio_global,
            bpm_delta_pct = (section_ratio.ratio_local - section_ratio.ratio_global).abs()
                / section_ratio.ratio_global.max(1e-9)
                * 100.0,
            outgoing_section_bpm = outgoing_section_bpm.unwrap_or(f64::NAN),
            incoming_section_bpm = incoming_section_bpm.unwrap_or(f64::NAN),
            outgoing_outro_start = plan.outgoing_crossfade_start_seconds,
            incoming_intro_end = plan.incoming_crossfade_start_seconds,
            t_start_frames = transition_start_master,
            scores = %scores_str,
            "transition arming"
        );

        let handle = match choice {
            dj_engine::recipe::RecipeChoice::EqualPower => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::EqualPowerCrossfade,
                )
                .await?
            }
            dj_engine::recipe::RecipeChoice::HardCut => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::HardCut,
                )
                .await?
            }
            dj_engine::recipe::RecipeChoice::FilterFade => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::FilterFade,
                )
                .await?
            }
            dj_engine::recipe::RecipeChoice::FilterKill => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::FilterKill,
                )
                .await?
            }
            dj_engine::recipe::RecipeChoice::LoopOut => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::LoopOut,
                )
                .await?
            }
            dj_engine::recipe::RecipeChoice::Drop => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::Drop,
                )
                .await?
            }
            dj_engine::recipe::RecipeChoice::RhythmSwap => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::RhythmSwap,
                )
                .await?
            }
            dj_engine::recipe::RecipeChoice::LoopBridge => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::LoopBridge,
                )
                .await?
            }
        };

        // Tick through the transition.
        dj_engine::mixer::tick_until(&mut mixer, handle.end_master_clock_frames).await?;

        // Unload outgoing, swap roles.
        mixer.deck(current_deck).unload().await;
        std::mem::swap(&mut current_deck, &mut other_deck);
        let transition_duration_seconds = (handle.end_master_clock_frames - transition_start_master)
            as f64
            / dj_engine::ffmpeg::SAMPLE_RATE as f64;
        let energy_delta = next_track.sidecar.energy_mean.unwrap_or(0.5)
            - current.sidecar.energy_mean.unwrap_or(0.5);
        let pitch_shift_cents = 1200.0 * ratio.log2();
        info!(
            outgoing = %short_name(&current.sidecar.source_path),
            incoming = %short_name(&next_track.sidecar.source_path),
            master_deck_now = ?current_deck,
            duration_seconds = transition_duration_seconds,
            recipe = handle.recipe_name,
            transition_type,
            bpm_ratio = ratio,
            energy_delta,
            pitch_shift_cents,
            "transition complete"
        );

        recipe_recency.record(choice);
        recency.record(next_track.sidecar.source_path.clone());
        energy.advance();
        icecast_meta::update_now_playing(&http, &metadata_cfg, &next_track.sidecar.source_path)
            .await;
        current = next_track;
    }

    let _ = writer_task.await;
    Ok(())
}

/// Last path component of a source path. Used to keep log lines short.
fn short_name(source_path: &str) -> &str {
    source_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(source_path)
}

/// Mean of a slice, or 0.0 for the empty case. Used to flatten per-bar
/// density arrays into a single number for log lines.
fn mean_or_zero(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Maps a `RecipeChoice` to its canonical name string. Mirrors `recipe_name` above.
fn recipe_choice_name(c: dj_engine::recipe::RecipeChoice) -> &'static str {
    match c {
        dj_engine::recipe::RecipeChoice::EqualPower => "equal_power",
        dj_engine::recipe::RecipeChoice::HardCut => "hard_cut",
        dj_engine::recipe::RecipeChoice::FilterFade => "filter_fade",
        dj_engine::recipe::RecipeChoice::FilterKill => "filter_kill",
        dj_engine::recipe::RecipeChoice::LoopOut => "loop_out",
        dj_engine::recipe::RecipeChoice::Drop => "drop",
        dj_engine::recipe::RecipeChoice::RhythmSwap => "rhythm_swap",
        dj_engine::recipe::RecipeChoice::LoopBridge => "loop_bridge",
    }
}

/// Human-readable label for the transition variant a `TransitionPlan` represents.
/// `length_bars` is the canonical signal: 0 = hard cut, 8 = quick cut, 16 = standard
/// blend, 32 = long blend. `use_eq_swap=true` now hints the planner toward a
/// rhythm-swap-style variant, so the label reflects the realized intent rather
/// than the pre-refactor recipe-specific naming.
fn transition_type_label(length_bars: usize, use_eq_swap: bool) -> &'static str {
    match (length_bars, use_eq_swap) {
        (0, _) => "hard_cut",
        (8, false) => "quick_cut",
        (8, true) => "eq_swap_quick_cut",
        (16, false) => "standard_blend",
        (16, true) => "eq_swap_standard_blend",
        (32, false) => "long_blend",
        (32, true) => "eq_swap_long_blend",
        _ => "custom_blend",
    }
}

fn pick_seed(library: &[LoadedTrack], requested: Option<&str>) -> LoadedTrack {
    if let Some(want) = requested {
        if let Some(t) = library
            .iter()
            .find(|t| t.sidecar.source_path.contains(want))
        {
            return t.clone();
        }
        warn!(seed = %want, "seed track not found; falling back to mid-energy random");
    }
    let candidates: Vec<&LoadedTrack> = library
        .iter()
        .filter(|t| {
            let e = t.sidecar.energy_mean.unwrap_or(0.5);
            t.sidecar.is_mixable == Some(true) && (0.3..=0.6).contains(&e)
        })
        .collect();
    let chosen = candidates
        .choose(&mut rand::thread_rng())
        .copied()
        .unwrap_or(&library[0]);
    chosen.clone()
}
