//! Bounded wall-clock telemetry. It never changes session delay or state.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct Stats {
    start: Instant,
    frames: VecDeque<Instant>,
    rollbacks: VecDeque<(Instant, u32)>,
    leads: VecDeque<(Instant, usize)>,
    pub rtt: Option<Duration>,
}

impl Stats {
    pub fn new(now: Instant) -> Self {
        Self {
            start: now,
            frames: VecDeque::new(),
            rollbacks: VecDeque::new(),
            leads: VecDeque::new(),
            rtt: None,
        }
    }

    pub fn advance(&mut self, now: Instant, depth: u32, input_lead: usize) {
        self.frames.push_back(now);
        self.leads.push_back((now, input_lead));
        if depth > 0 {
            self.rollbacks.push_back((now, depth));
        }
        self.prune(now);
    }

    fn prune(&mut self, now: Instant) {
        while self
            .frames
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_millis(500))
        {
            self.frames.pop_front();
        }
        while self
            .rollbacks
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) >= Duration::from_secs(60))
        {
            self.rollbacks.pop_front();
        }
        while self
            .leads
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) >= Duration::from_secs(10))
        {
            self.leads.pop_front();
        }
    }

    pub fn lines(&mut self, now: Instant, game: &crate::netplay_game::Game) -> Vec<String> {
        self.prune(now);
        let elapsed = now.duration_since(self.start).as_secs_f64().min(0.5);
        let fps = if elapsed > 0.0 {
            self.frames.len() as f64 / elapsed
        } else {
            0.0
        };
        let recommended =
            if now.duration_since(self.start) < Duration::from_secs(5) || self.leads.len() < 60 {
                "Measuring...".into()
            } else {
                let (delay, capped) =
                    recommendation(self.leads.iter().map(|(_, lead)| *lead).collect());
                format!(
                    "{delay} frames{}",
                    if capped { " (high lateness)" } else { "" }
                )
            };
        let recent_depth = self.rollbacks.back().map(|(_, d)| *d).unwrap_or(0);
        vec![
            "NETPLAY STATS - F1 hides".into(),
            format!(
                "Ping RTT: {}",
                self.rtt
                    .map(|d| format!("{:.1} ms", d.as_secs_f64() * 1000.0))
                    .unwrap_or("Measuring...".into())
            ),
            format!("Game FPS (500ms): {fps:.1}"),
            format!("Delay: {} frames", game.session.present_delay()),
            format!("Rollbacks total: {}", game.corrections),
            format!("Rollbacks last 60s: {}", self.rollbacks.len()),
            format!("Depth last / max: {recent_depth} / {}", game.max_depth),
            format!(
                "Prediction: {} frames",
                game.session.speculation_balance().max(0)
            ),
            format!("Input queue: {} / 10", game.session.local_queue_length()),
            format!(
                "Waiting for inputs: {}",
                if game.can_advance() { "No" } else { "Yes" }
            ),
            format!("Synced frame: {}", game.matched_hash_tick),
            format!("Recommended delay: {recommended}"),
        ]
    }
}

// Recent p95 input lead, allowing two frames of prediction. This is a
// responsiveness/smoothness heuristic, not a latency guarantee or negotiation.
fn recommendation(mut leads: Vec<usize>) -> (usize, bool) {
    leads.sort_unstable();
    let p95 = leads[(leads.len() * 95).div_ceil(100).saturating_sub(1)];
    let target = p95.saturating_sub(2).max(1);
    (target.min(4), target > 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rolling_windows_expire_even_when_game_stalls() {
        let start = Instant::now();
        let mut stats = Stats::new(start);
        stats.advance(start, 3, 5);
        stats.advance(start + Duration::from_millis(100), 0, 2);
        stats.prune(start + Duration::from_secs(1));
        assert!(stats.frames.is_empty());
        assert_eq!(stats.rollbacks.len(), 1);
        stats.prune(start + Duration::from_secs(11));
        assert!(stats.leads.is_empty());
        stats.prune(start + Duration::from_secs(61));
        assert!(stats.rollbacks.is_empty());
    }
    #[test]
    fn recommendation_handles_lan_distance_and_outliers() {
        assert_eq!(recommendation(vec![1; 100]), (1, false));
        assert_eq!(recommendation(vec![5; 100]), (3, false));
        assert_eq!(recommendation(vec![6; 100]), (4, false));
        assert_eq!(recommendation(vec![10; 100]), (4, true));
        let mut samples = vec![4; 99];
        samples.push(10);
        assert_eq!(recommendation(samples), (2, false));
    }
}
