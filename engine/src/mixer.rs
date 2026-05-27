//! Mixer: ticks two persistent decks, sums their PCM, pushes to the ring.

use crate::buffer::{Producer, SampleProducer};
use crate::deck::{Deck, DeckId};
use crate::error::Result;
use crate::ffmpeg::CHANNELS;

pub const TICK_FRAMES: usize = 4096;

pub struct Mixer {
    deck_a: Deck,
    deck_b: Deck,
    producer: SampleProducer,
    master_clock_frames: u64,
}

impl Mixer {
    pub fn new(producer: SampleProducer) -> Self {
        Self {
            deck_a: Deck::new(DeckId::A),
            deck_b: Deck::new(DeckId::B),
            producer,
            master_clock_frames: 0,
        }
    }

    pub fn deck(&mut self, id: DeckId) -> &mut Deck {
        match id {
            DeckId::A => &mut self.deck_a,
            DeckId::B => &mut self.deck_b,
        }
    }

    pub fn master_clock_frames(&self) -> u64 {
        self.master_clock_frames
    }

    /// Pull from both decks in parallel, sum, push to the ring buffer.
    pub async fn tick(&mut self) -> Result<()> {
        // We can't borrow both decks mutably for `tokio::join!` because the
        // borrow checker rejects two simultaneous mutable borrows of fields of
        // the same `Mixer`. Split-borrow the two decks first.
        let Mixer {
            deck_a,
            deck_b,
            producer,
            master_clock_frames,
        } = self;
        let (a, b) = tokio::join!(deck_a.pull(TICK_FRAMES), deck_b.pull(TICK_FRAMES));
        let a = a?;
        let b = b?;
        debug_assert_eq!(a.len(), TICK_FRAMES * (CHANNELS as usize));
        debug_assert_eq!(b.len(), TICK_FRAMES * (CHANNELS as usize));

        let mut mixed = vec![0.0f32; a.len()];
        for i in 0..mixed.len() {
            mixed[i] = a[i] + b[i];
        }
        push_all(producer, &mixed).await;
        *master_clock_frames += TICK_FRAMES as u64;
        Ok(())
    }
}

async fn push_all(producer: &mut SampleProducer, samples: &[f32]) {
    let mut offset = 0;
    while offset < samples.len() {
        let n = producer.push_slice(&samples[offset..]);
        if n == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        } else {
            offset += n;
        }
    }
}

/// Convenience entry-point: tick until `master_clock_frames >= end_frames`.
/// Used by the main loop to play out a transition or until the next decision.
pub async fn tick_until(mixer: &mut Mixer, end_frames: u64) -> Result<()> {
    while mixer.master_clock_frames() < end_frames {
        mixer.tick().await?;
    }
    tracing::debug!(end = end_frames, "tick_until complete");
    Ok(())
}
