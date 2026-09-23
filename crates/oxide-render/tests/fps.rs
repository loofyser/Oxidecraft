//! Frame-rate counter tests.

use std::time::Duration;

use oxide_render::fps::FpsCounter;

#[test]
fn counts_frames_over_one_second_windows() {
    let mut counter = FpsCounter::new(Duration::from_secs(1));
    for _ in 0..60 {
        counter.tick(Duration::from_micros(16_666));
    }
    assert!((counter.fps() - 60.0).abs() < 2.0, "got {}", counter.fps());
}

#[test]
fn reports_zero_before_a_window_completes() {
    let counter = FpsCounter::new(Duration::from_secs(1));
    assert_eq!(counter.fps(), 0.0);
}
