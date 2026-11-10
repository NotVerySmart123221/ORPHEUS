//! Worker-local cumulative timing for opt-in diagnostics.

use std::time::Duration;

use serde::Serialize;

const UPPER_US: [u64; 20] = [
    25, 50, 100, 150, 250, 500, 750, 1_000, 2_000, 4_000, 6_000, 8_000, 12_000, 16_000, 24_000,
    32_000, 48_000, 64_000, 128_000, 256_000,
];

#[derive(Clone, Default, Serialize)]
pub(crate) struct DurationBins {
    pub samples: u64,
    pub counts: [u64; UPPER_US.len() + 1],
    max_us: u64,
}

impl DurationBins {
    fn record(&mut self, duration: Duration) {
        let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        let bin = UPPER_US.partition_point(|&upper| micros >= upper);
        self.samples = self.samples.saturating_add(1);
        self.counts[bin] = self.counts[bin].saturating_add(1);
        self.max_us = self.max_us.max(micros);
    }
}

#[derive(Clone, Default, Serialize)]
pub(crate) struct InputBudget {
    pub turns: u64,
    pub used: u64,
    pub peer_limit_hits: u64,
    pub turn_limit_hits: u64,
    pub turns_with_backlog: u64,
}

#[derive(Clone, Serialize)]
pub(crate) struct WorkerTiming {
    pub bin_upper_exclusive_us: &'static [u64],
    pub tick_duration_us: DurationBins,
    pub schedule_lateness_us: DurationBins,
    pub missed_ticks: u64,
    pub input_budget: InputBudget,
}

impl WorkerTiming {
    pub(crate) fn new() -> Self {
        Self {
            bin_upper_exclusive_us: &UPPER_US,
            tick_duration_us: DurationBins::default(),
            schedule_lateness_us: DurationBins::default(),
            missed_ticks: 0,
            input_budget: InputBudget::default(),
        }
    }

    pub(crate) fn record_tick(&mut self, duration: Duration, lateness: Duration, missed: u64) {
        self.tick_duration_us.record(duration);
        self.schedule_lateness_us.record(lateness);
        self.missed_ticks = self.missed_ticks.saturating_add(missed);
    }

    pub(crate) fn record_input(
        &mut self,
        used: usize,
        peer_limit_hits: u64,
        turn_limit_hit: bool,
        backlog: bool,
    ) {
        if used == 0 && !backlog {
            return;
        }
        let input = &mut self.input_budget;
        input.turns = input.turns.saturating_add(1);
        input.used = input.used.saturating_add(used as u64);
        input.peer_limit_hits = input.peer_limit_hits.saturating_add(peer_limit_hits);
        input.turn_limit_hits = input
            .turn_limit_hits
            .saturating_add(u64::from(turn_limit_hit));
        input.turns_with_backlog = input.turns_with_backlog.saturating_add(u64::from(backlog));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_samples_enter_the_next_bin_and_the_last_bin_is_open() {
        let mut bins = DurationBins::default();
        for duration in [
            Duration::ZERO,
            Duration::from_nanos(24_999),
            Duration::from_micros(25),
            Duration::from_micros(255_999),
            Duration::from_micros(256_000),
            Duration::MAX,
        ] {
            bins.record(duration);
        }
        assert_eq!(bins.samples, 6);
        assert_eq!(&bins.counts[..2], &[2, 1]);
        assert_eq!(&bins.counts[UPPER_US.len() - 1..], &[1, 2]);
        assert_eq!(bins.counts.into_iter().sum::<u64>(), 6);
        assert_eq!(bins.max_us, u64::MAX);
    }

    #[test]
    fn snapshots_are_cumulative_and_produce_independent_interval_deltas() {
        let mut timing = WorkerTiming::new();
        timing.record_tick(Duration::from_micros(149), Duration::from_micros(400), 0);
        timing.record_input(0, 0, false, false);
        let before = timing.clone();
        timing.record_tick(Duration::from_micros(50), Duration::from_millis(16), 2);
        timing.record_tick(Duration::from_micros(500), Duration::from_millis(1), 0);
        timing.record_input(64, 4, true, true);
        timing.record_input(7, 0, false, false);

        assert_eq!(before.input_budget.turns, 0);
        assert_eq!(timing.input_budget.turns - before.input_budget.turns, 2);
        assert_eq!(timing.input_budget.used - before.input_budget.used, 71);
        assert_eq!(timing.input_budget.peer_limit_hits, 4);
        assert_eq!(timing.input_budget.turn_limit_hits, 1);
        assert_eq!(timing.input_budget.turns_with_backlog, 1);
        assert_eq!(timing.missed_ticks - before.missed_ticks, 2);
        assert_eq!(
            timing.tick_duration_us.samples - before.tick_duration_us.samples,
            2
        );
        assert_eq!(
            timing.tick_duration_us.counts[2] - before.tick_duration_us.counts[2],
            1
        );
        assert_eq!(timing.schedule_lateness_us.counts[14], 1);
        assert_eq!(timing.schedule_lateness_us.max_us, 16_000);
    }
}
