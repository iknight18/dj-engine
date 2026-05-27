//! Track selection: filter candidates, score them, with a fallback chain.

use rand::seq::SliceRandom;

use crate::camelot::{CamelotKey, KeyMatch};
use crate::library::LoadedTrack;
use crate::recency::RecencyRing;

const BPM_RELAX_TIGHT: f64 = 0.06;
const BPM_RELAX_LOOSE: f64 = 0.10;
const TOP_K: usize = 5;

pub struct Selector<'a> {
    library: &'a [LoadedTrack],
    recency: &'a RecencyRing,
    target_energy: f64,
}

#[derive(Debug, Clone, Copy)]
struct Constraints {
    bpm_window: f64,
    require_key_match: bool,
    require_mixable: bool,
}

impl<'a> Selector<'a> {
    pub fn new(library: &'a [LoadedTrack], recency: &'a RecencyRing, target_energy: f64) -> Self {
        Self {
            library,
            recency,
            target_energy,
        }
    }

    /// Pick the next track. Walks the fallback chain until something matches.
    pub fn pick_next(&mut self, current: &LoadedTrack) -> Option<&'a LoadedTrack> {
        let chain = [
            Constraints {
                bpm_window: BPM_RELAX_TIGHT,
                require_key_match: true,
                require_mixable: true,
            },
            Constraints {
                bpm_window: BPM_RELAX_TIGHT,
                require_key_match: false,
                require_mixable: true,
            },
            Constraints {
                bpm_window: BPM_RELAX_LOOSE,
                require_key_match: false,
                require_mixable: true,
            },
            Constraints {
                bpm_window: BPM_RELAX_LOOSE,
                require_key_match: false,
                require_mixable: false,
            },
            // Final fallback: accept anything that's not the current track and not recent.
            Constraints {
                bpm_window: f64::INFINITY,
                require_key_match: false,
                require_mixable: false,
            },
        ];
        for constraints in &chain {
            let mut scored: Vec<(f64, &'a LoadedTrack)> = self
                .filter(current, *constraints)
                .map(|t| (self.score(current, t), t))
                .collect();
            if !scored.is_empty() {
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                let top: Vec<&(f64, &LoadedTrack)> = scored.iter().take(TOP_K).collect();
                let chosen = top.choose(&mut rand::thread_rng()).copied()?;
                return Some(chosen.1);
            }
        }
        None
    }

    fn filter(
        &self,
        current: &LoadedTrack,
        c: Constraints,
    ) -> impl Iterator<Item = &'a LoadedTrack> + '_ {
        let current_bpm = current.sidecar.bpm.unwrap_or(120.0);
        let current_key = current.sidecar.key.as_deref().and_then(CamelotKey::parse);
        let current_path = current.sidecar.source_path.clone();

        self.library.iter().filter(move |t| {
            if t.sidecar.source_path == current_path {
                return false;
            }
            if self.recency.was_recent(&t.sidecar.source_path) {
                return false;
            }
            if c.require_mixable && t.sidecar.is_mixable != Some(true) {
                return false;
            }
            let cand_bpm = match t.sidecar.bpm {
                Some(b) => b,
                None => return false,
            };
            let rel_diff = (cand_bpm - current_bpm).abs() / current_bpm.max(1.0);
            if rel_diff > c.bpm_window {
                return false;
            }
            if c.require_key_match {
                let cand_key = t.sidecar.key.as_deref().and_then(CamelotKey::parse);
                match (current_key, cand_key) {
                    (Some(a), Some(b)) => {
                        if matches!(a.compatibility(&b), KeyMatch::None) {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
            true
        })
    }

    fn score(&self, current: &LoadedTrack, candidate: &LoadedTrack) -> f64 {
        let current_bpm = current.sidecar.bpm.unwrap_or(120.0);
        let cand_bpm = candidate.sidecar.bpm.unwrap_or(120.0);
        let rel = (cand_bpm - current_bpm).abs() / current_bpm.max(1.0);
        let bpm_score = (1.0 - rel / BPM_RELAX_TIGHT).max(0.0);

        let key_score = match (
            current.sidecar.key.as_deref().and_then(CamelotKey::parse),
            candidate.sidecar.key.as_deref().and_then(CamelotKey::parse),
        ) {
            (Some(a), Some(b)) => a.compatibility(&b).score(),
            _ => 0.0,
        };

        let cand_energy = candidate.sidecar.energy_mean.unwrap_or(0.5);
        let energy_score = (1.0 - (cand_energy - self.target_energy).abs() / 0.15).max(0.0);

        // Transition-aware term — look ahead at what plan_transition would produce.
        let transition_score = match crate::transition::plan_transition(
            &current.sidecar,
            &candidate.sidecar,
        )
        .length_bars
        {
            32 => 1.0,
            16 => 0.85,
            8 => 0.6,
            0 => 0.2,
            _ => 0.5,
        };

        bpm_score * 1.0 + key_score * 1.0 + energy_score * 0.5 + transition_score * 1.0
    }
}
