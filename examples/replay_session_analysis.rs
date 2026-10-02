//! Recompute analysis from retained game events; print aggregate counts only.
use akagi::{analysis, game_state::tracker::GameTracker, schema::InspectorEntry};
use std::io::{BufRead, BufReader};
fn run() -> Option<(usize, usize)> {
    let path = std::env::args_os().nth(1)?;
    let file = std::fs::File::open(path).ok()?;
    let mut tracker = GameTracker::new();
    let (mut events, mut analyses) = (0, 0);
    for line in BufReader::new(file).lines() {
        let line = line.ok()?;
        let entry: InspectorEntry = serde_json::from_str(&line).ok()?;
        let InspectorEntry::MjaiEvent { event, .. } = entry else {
            continue;
        };
        tracker.handle(&event).ok()?;
        events += 1;
        let Some(snapshot) = tracker.snapshot() else {
            continue;
        };
        let Some(seat) = snapshot.our_seat else {
            continue;
        };
        let info = analysis::snapshot_adapter::to_player_info(&snapshot, seat).ok()?;
        if ![4, 5, 7, 8, 10, 11, 13, 14].contains(&info.hand_size()) {
            continue;
        }
        let result = analysis::analyze(&info);
        if !(-1..=13).contains(&result.shanten) || !result.mixed_risk.iter().all(|v| v.is_finite())
        {
            return None;
        }
        analyses += 1;
    }
    if events == 0 || analyses == 0 {
        return None;
    }
    Some((events, analyses))
}
fn main() {
    std::panic::set_hook(Box::new(|_| eprintln!("SESSION_ANALYSIS_REPLAY_FAILED")));
    if let Some((events, analyses)) = run() {
        println!("SESSION_ANALYSIS_REPLAY_PASSED events={events} analyses={analyses}");
    } else {
        println!("SESSION_ANALYSIS_REPLAY_FAILED");
        std::process::exit(1);
    }
}
