//! The fixed-step tick scheduler: how many 20 Hz steps a play-loop pass owes.
//!
//! The source drives its ticks from a `Timer` polled once per rendered frame:
//! elapsed time accumulates in ticks and at most ten whole ticks are run per
//! frame, the rest dropped (`Timer.updateTimer`, `Timer.java:65-109`;
//! `Minecraft.run`'s `for (int j = 0; j < this.timer.elapsedTicks; ++j)`,
//! `Minecraft.java:1096-1108`). [`Ticker`] is that scheduler reduced to the
//! deadline arithmetic a play loop needs: a step comes due on a fixed
//! deadline, and a poll reports every whole step that has come due since the
//! last one.
//!
//! The cap and the debt rule are the source's own. `Timer.updateTimer`
//! clamps the accumulated elapsed time to one second before it becomes ticks
//! (`MathHelper.clamp_double(d2, 0.0D, 1.0D)`, `Timer.java:98`), drains the
//! whole-tick part from its accumulator (`:100-101`) before it caps what it
//! reports at ten (`:103-106`), and resets the high-resolution clock outright
//! when the system clock jumped by more than a second (`:72-93`). A capped
//! catch-up therefore carries no debt: the surplus is discarded and the next
//! poll starts from now.

use std::time::{Duration, Instant};

/// The most whole steps one poll reports.
///
/// The source caps a frame's tick run at ten: `if (this.elapsedTicks > 10) {
/// this.elapsedTicks = 10; }` (`Timer.updateTimer`, `Timer.java:103-106`),
/// and `Minecraft.run` runs exactly `timer.elapsedTicks` ticks per frame
/// (`Minecraft.java:1096-1108`). The excess had already been drained from the
/// source's accumulator (`Timer.java:100-106`), so it is dropped rather than
/// carried into the next frame — the debt rule this scheduler keeps.
pub const TICK_CATCHUP_CAP: u32 = 10;

/// A fixed-step scheduler: a deadline that reports the whole steps due.
#[derive(Debug)]
pub struct Ticker {
    /// The length of one step.
    step: Duration,
    /// The instant the next step comes due.
    next: Instant,
}

impl Ticker {
    /// A ticker whose first step is due one `step` from now.
    pub fn new(step: Duration) -> Ticker {
        Ticker {
            step,
            next: Instant::now(),
        }
    }

    /// Reports the whole steps due at `now` and advances the deadline.
    ///
    /// The deadline advances by exactly the steps reported, so a poll between
    /// steps reports nothing and the sub-step remainder stays with the next
    /// step. A catch-up that would exceed [`TICK_CATCHUP_CAP`] reports the
    /// cap and re-anchors the deadline at `now`: the source drops the surplus
    /// rather than owing it (`Timer.java:98-106`), and a long pause must not
    /// leave a debt the session would pay off forever.
    pub fn due(&mut self, now: Instant) -> u32 {
        if now < self.next {
            return 0;
        }
        // A zero step would divide by zero; a ticker is only ever built with
        // a real period, and one nanosecond keeps a degenerate one harmless.
        let step = self.step.as_nanos().max(1);
        let elapsed = now.duration_since(self.next).as_nanos();
        let steps = elapsed / step;
        if steps > u128::from(TICK_CATCHUP_CAP) {
            self.next = now;
            return TICK_CATCHUP_CAP;
        }
        let steps = steps as u32;
        self.next += self.step * steps;
        steps
    }
}

#[cfg(test)]
mod tests {
    //! The deadline arithmetic, against synthetic instants.

    use std::time::{Duration, Instant};

    use super::Ticker;

    /// One step: twenty ticks per second (`new Timer(20.0F)`,
    /// `Minecraft.java:223`; `Minecraft.run` runs its ticks from it).
    const STEP: Duration = Duration::from_millis(50);

    /// A ticker and the instant its deadline was set.
    ///
    /// [`Ticker::new`] reads its own `Instant::now()`, which lies between this
    /// helper's entry and the `start` capture, so `start` is at or after the
    /// deadline: every synthetic instant below is measured from there, and a
    /// poll exactly at `start + n * STEP` reports at least `n` steps.
    fn ticker() -> (Ticker, Instant) {
        let ticker = Ticker::new(STEP);
        let start = Instant::now();
        (ticker, start)
    }

    #[test]
    fn a_fresh_ticker_reports_nothing_before_its_step_elapses() {
        let (mut ticker, start) = ticker();
        assert_eq!(ticker.due(start), 0, "the deadline has not passed");
        assert_eq!(
            ticker.due(start + Duration::from_millis(1)),
            0,
            "a poll a millisecond in is still inside the first step"
        );
    }

    #[test]
    fn one_and_two_whole_steps_are_reported_as_time_advances() {
        let (mut ticker, start) = ticker();
        // A hair past one step: the whole step, and only it.
        assert_eq!(ticker.due(start + STEP + Duration::from_millis(5)), 1);
        // Two steps further on from the advanced deadline: two steps.
        assert_eq!(ticker.due(start + STEP * 3 + Duration::from_millis(5)), 2);
    }

    #[test]
    fn polls_between_steps_report_nothing() {
        let (mut ticker, start) = ticker();
        assert_eq!(ticker.due(start + STEP + Duration::from_millis(5)), 1);
        assert_eq!(ticker.due(start + STEP + Duration::from_millis(6)), 0);
        assert_eq!(ticker.due(start + STEP + Duration::from_millis(7)), 0);
        // The sub-step remainder is kept: the second step is due 50 ms after
        // the first, not 50 ms after the poll that reported it.
        assert_eq!(ticker.due(start + STEP * 2 + Duration::from_millis(5)), 1);
    }

    #[test]
    fn a_long_pause_reports_ten_steps_and_leaves_no_debt() {
        let (mut ticker, start) = ticker();
        let paused = start + STEP * 50;
        // The cap's literal value: a frame's tick run is clamped to ten
        // (`if (this.elapsedTicks > 10) { this.elapsedTicks = 10; }`,
        // `Timer.java:103-106`).
        assert_eq!(
            ticker.due(paused),
            10,
            "a fifty-step pause reports ten steps, not fifty"
        );
        assert_eq!(
            ticker.due(paused),
            0,
            "the capped surplus is dropped, not owed to the next poll"
        );
        assert_eq!(ticker.due(paused + Duration::from_millis(5)), 0);
        assert_eq!(
            ticker.due(paused + STEP + Duration::from_millis(5)),
            1,
            "the deadline runs on from the re-anchor, not from the capped point"
        );
    }
}
