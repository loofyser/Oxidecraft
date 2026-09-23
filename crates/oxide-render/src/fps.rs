//! Frame-rate accounting for the render loop.

use std::time::{Duration, Instant};

/// Averages frame intervals over a fixed window.
///
/// Feed one [`FpsCounter::tick`] per rendered frame with the time since the previous
/// frame, or one [`FpsCounter::record_frame`] per frame with the instant it was drawn.
/// The reported value is the average over the window currently filling; once a window
/// completes, its average stands until the next window starts filling.
#[derive(Debug)]
pub struct FpsCounter {
    /// The length of one measurement window.
    window: Duration,
    /// Frames recorded in the window currently filling.
    frames: u32,
    /// Time recorded for the window currently filling.
    elapsed: Duration,
    /// The average of the most recently completed window, 0.0 until one completes.
    last: f32,
    /// The instant of the previous frame, once one has been recorded.
    last_frame: Option<Instant>,
}

impl FpsCounter {
    /// Creates a counter that averages over `window`.
    ///
    /// A window of zero length is accepted: every tick that carries any elapsed time then
    /// completes a window. The reported value stays 0.0 until frames arrive.
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            frames: 0,
            elapsed: Duration::ZERO,
            last: 0.0,
            last_frame: None,
        }
    }

    /// Records one frame that took `elapsed` since the previous frame.
    ///
    /// The accounting divides by the full accumulated time of the window rather than by
    /// the window's nominal length, so a stall drags the average down instead of being
    /// hidden.
    pub fn tick(&mut self, elapsed: Duration) {
        self.frames = self.frames.saturating_add(1);
        self.elapsed += elapsed;
        if self.elapsed >= self.window && !self.elapsed.is_zero() {
            self.last = self.frames as f32 / self.elapsed.as_secs_f32();
            self.frames = 0;
            self.elapsed = Duration::ZERO;
        }
    }

    /// Records one frame drawn at `now`, measuring the interval from the previous frame.
    ///
    /// The first call only starts the clock: there is no earlier instant to measure from,
    /// so it contributes no time and no frame.
    pub fn record_frame(&mut self, now: Instant) {
        if let Some(previous) = self.last_frame {
            self.tick(now.saturating_duration_since(previous));
        }
        self.last_frame = Some(now);
    }

    /// The average frame rate: over the window currently filling, or the last completed one.
    ///
    /// The value is 0.0 until a frame with a measurable interval has been recorded.
    pub fn fps(&self) -> f32 {
        if self.frames > 0 && !self.elapsed.is_zero() {
            self.frames as f32 / self.elapsed.as_secs_f32()
        } else {
            self.last
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the window accounting and its behaviour across a stall.

    use std::time::{Duration, Instant};

    use super::FpsCounter;

    /// The interval between frames of a 60 Hz stream.
    const FRAME_60_HZ: Duration = Duration::from_micros(16_666);

    #[test]
    fn a_completed_window_reads_back_its_rate() {
        let mut counter = FpsCounter::new(Duration::from_secs(1));
        for _ in 0..61 {
            counter.tick(FRAME_60_HZ);
        }
        assert!((counter.fps() - 60.0).abs() < 0.1, "got {}", counter.fps());
    }

    #[test]
    fn a_stall_drags_the_window_average_down() {
        let mut counter = FpsCounter::new(Duration::from_secs(1));
        for _ in 0..30 {
            counter.tick(FRAME_60_HZ);
        }
        // One frame arrives after a five-second freeze: 31 frames over 5.49998 s.
        counter.tick(Duration::from_secs(5));
        let fps = counter.fps();
        // Dividing by the one-second window instead would report 31 fps.
        assert!(fps > 5.0 && fps < 6.0, "got {fps}");
    }

    #[test]
    fn the_window_after_a_completed_one_measures_on_its_own() {
        let mut counter = FpsCounter::new(Duration::from_secs(1));
        for _ in 0..61 {
            counter.tick(FRAME_60_HZ);
        }
        let first = counter.fps();
        assert!((first - 60.0).abs() < 0.1, "first window: {first}");
        for _ in 0..61 {
            counter.tick(FRAME_60_HZ);
        }
        let second = counter.fps();
        assert!((second - 60.0).abs() < 0.1, "second window: {second}");
    }

    #[test]
    fn record_frame_measures_the_interval_between_instants() {
        let mut counter = FpsCounter::new(Duration::from_secs(1));
        let base = Instant::now();
        counter.record_frame(base);
        assert_eq!(counter.fps(), 0.0, "the first frame only starts the clock");
        for step in 1..=60 {
            counter.record_frame(base + FRAME_60_HZ * step);
        }
        assert!((counter.fps() - 60.0).abs() < 0.1, "got {}", counter.fps());
    }

    #[test]
    fn a_tick_with_no_elapsed_time_does_not_disturb_the_average() {
        let mut counter = FpsCounter::new(Duration::ZERO);
        counter.tick(FRAME_60_HZ);
        counter.tick(Duration::ZERO);
        let fps = counter.fps();
        assert!(fps.is_finite() && (fps - 60.0).abs() < 0.1, "got {fps}");
    }
}
