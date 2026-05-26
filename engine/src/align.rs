//! Beat alignment math.

#[derive(Debug, Clone, Copy)]
pub struct AlignmentPlan {
    /// Where in the OUTPUT timeline the incoming track's `intro_end_seconds` lands.
    pub incoming_start_seconds: f64,
}

/// Compute when the incoming track's `intro_end_seconds` should be placed in the output
/// timeline so that its downbeat lands on the outgoing track's downbeat at
/// `outgoing_outro_start`.
///
/// If no downbeats are available, falls back to a simple offset that places
/// `incoming_intro_end_seconds` at `outgoing_outro_start_seconds`.
pub fn align_downbeats(
    outgoing_downbeats: &[f64],
    outgoing_outro_start_seconds: f64,
    _incoming_downbeats: &[f64],
    incoming_intro_end_seconds: f64,
) -> AlignmentPlan {
    let target = nearest_downbeat(outgoing_downbeats, outgoing_outro_start_seconds)
        .unwrap_or(outgoing_outro_start_seconds);
    AlignmentPlan {
        incoming_start_seconds: target - incoming_intro_end_seconds,
    }
}

fn nearest_downbeat(downbeats: &[f64], target: f64) -> Option<f64> {
    downbeats.iter().copied().min_by(|a, b| {
        (*a - target)
            .abs()
            .partial_cmp(&(*b - target).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}
