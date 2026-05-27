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
    let mut mixer = dj_engine::mixer::Mixer::new(producer);
    {
        let cue_in = seed.sidecar.cue_in_seconds.unwrap_or(0.0);
        let deck = mixer.deck(dj_engine::deck::DeckId::A);
        deck.cue(seed.clone(), cue_in).await?;
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

        let plan = dj_engine::transition::plan_transition(&current.sidecar, &next_track.sidecar);

        // Play current track until its outro_start (then the recipe takes over).
        // outro_start in source-time -> master-clock frames.
        let cue_in = current.sidecar.cue_in_seconds.unwrap_or(0.0);
        let outro_start = plan.outgoing_crossfade_start_seconds;
        let frames_until_outro =
            ((outro_start - cue_in).max(0.0) * dj_engine::ffmpeg::SAMPLE_RATE as f64) as u64;
        let outro_deadline = mixer.master_clock_frames() + frames_until_outro;
        dj_engine::mixer::tick_until(&mut mixer, outro_deadline).await?;

        // Arm the transition.
        let choice = dj_engine::recipe::pick_recipe(&plan);
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
        };

        // Tick through the transition.
        dj_engine::mixer::tick_until(&mut mixer, handle.end_master_clock_frames).await?;

        // Unload outgoing, swap roles.
        mixer.deck(current_deck).unload().await;
        std::mem::swap(&mut current_deck, &mut other_deck);
        recency.record(next_track.sidecar.source_path.clone());
        energy.advance();
        icecast_meta::update_now_playing(&http, &metadata_cfg, &next_track.sidecar.source_path)
            .await;
        current = next_track;
    }

    let _ = writer_task.await;
    Ok(())
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
