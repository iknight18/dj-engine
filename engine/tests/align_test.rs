use dj_engine::align::{align_downbeats, AlignmentPlan};

#[test]
fn aligns_to_nearest_outgoing_downbeat() {
    let outgoing_downbeats = vec![0.0, 2.0, 4.0, 6.0, 8.0];
    let incoming_downbeats = vec![0.5, 2.5, 4.5];
    let outgoing_outro_start = 4.0;
    let incoming_intro_end = 0.5;

    let plan = align_downbeats(
        &outgoing_downbeats,
        outgoing_outro_start,
        &incoming_downbeats,
        incoming_intro_end,
    );
    // Outgoing downbeat at 4.0 should match incoming's 0.5 -> incoming offset = 4.0 - 0.5 = 3.5
    assert!((plan.incoming_start_seconds - 3.5).abs() < 1e-6);
}

#[test]
fn falls_back_to_intro_end_when_no_downbeats_near() {
    let plan: AlignmentPlan = align_downbeats(&[], 4.0, &[], 0.5);
    assert!((plan.incoming_start_seconds - 3.5).abs() < 1e-6);
}
