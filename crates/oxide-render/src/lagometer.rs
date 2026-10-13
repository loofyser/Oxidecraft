//! The lagometer: the F3 overlay's frame-duration graph
//! (`GuiOverlayDebug.renderLagometer`, `GuiOverlayDebug.java:219-256`, fed
//! by `FrameTimer`, `util/FrameTimer.java`).
//!
//! The graph draws only inside the F3 overlay (`:48-51`) — the `lagometer`
//! flag implies the overlay text is up — and lives in the GUI-space
//! hud/overlay group, never the world-space overlay. The data is the last
//! 240 per-frame durations in ns, fed once per rendered frame at the client
//! frame loop (never the session, never ticks).
//!
//! Two deviations from the source, both per the binding resolutions: the
//! bar height clamps to 0..60 (the source draws the line past the
//! background's top when a frame runs long; R2 pins the clamp), and the
//! label chips — the small background rects the source draws behind "60"
//! and "30" (`:236`, `:242`) — are omitted (R2 enumerates the graph's
//! geometry without them).

use crate::hud::HudDraw;

/// The ring's length (`FrameTimer.java:5`).
pub const FRAME_RING_LEN: usize = 240;
/// The nanoseconds in the source's lagometer step (`FrameTimer.java:49`).
pub const LAGO_STEP_NS: f64 = 16_666_666.0;
/// The graph's background (`renderLagometer`, `:232`).
pub const LAGO_BG: i32 = -1_873_784_752;
/// The frame colours (`getFrameColor`, `:258-260`).
pub const LAGO_GREEN: i32 = -16_711_936;
/// The frame colours (`getFrameColor`, `:258-260`).
pub const LAGO_YELLOW: i32 = -256;
/// The frame colours (`getFrameColor`, `:258-260`).
pub const LAGO_RED: i32 = -65_536;
/// The framerate-limit line (`renderLagometer`, `:252`).
pub const LAGO_BLUE: i32 = -16_711_681;
/// The label grey (`renderLagometer`, `:237`).
pub const LAGO_LABEL: i32 = 14_737_632;
/// The frame colour's break: green→yellow below, yellow→red above
/// (`getFrameColor`, `:258-260`).
pub const LAGO_MID: u32 = 30;
/// The bar height's clamp (`getLagometerValue` is unclamped; R2 pins 60).
pub const LAGO_MAX_PX: u32 = 60;

/// The last 240 per-frame durations in ns (`util/FrameTimer.java:5-39`):
/// the ring `add_frame` feeds once per rendered frame.
#[derive(Debug, Clone)]
pub struct FrameTimer {
    frames: [u64; FRAME_RING_LEN],
    last_index: usize,
    counter: usize,
    index: usize,
}

impl FrameTimer {
    /// The empty ring.
    pub fn new() -> Self {
        FrameTimer {
            frames: [0; FRAME_RING_LEN],
            last_index: 0,
            counter: 0,
            index: 0,
        }
    }

    /// Store one frame's duration in ns at the next ring slot
    /// (`addFrame`, `:20-39`).
    pub fn add_frame(&mut self, ns: u64) {
        self.frames[self.index] = ns;
        self.index += 1;
        if self.index == FRAME_RING_LEN {
            self.index = 0;
        }
        if self.counter < FRAME_RING_LEN {
            self.last_index = 0;
            self.counter += 1;
        } else {
            self.last_index = self.index;
        }
    }

    /// How many frames the ring holds (at most 240).
    pub fn len(&self) -> usize {
        self.counter
    }

    /// Whether no frame has landed yet.
    pub fn is_empty(&self) -> bool {
        self.counter == 0
    }

    /// The stored durations, oldest first (`renderLagometer`, `:226-234`
    /// walks `lastIndex` to `index`).
    pub fn ordered(&self) -> Vec<u64> {
        (0..self.counter)
            .map(|n| self.frames[Self::parse_index(self.last_index + n)])
            .collect()
    }

    /// The ring's index wrap (`parseIndex`, `:71-74`).
    pub fn parse_index(raw: usize) -> usize {
        raw % FRAME_RING_LEN
    }

    /// A duration's bar height in px before the clamp
    /// (`getLagometerValue`, `:47-53`).
    pub fn get_lagometer_value(ns: u64, multiplier: u32) -> u32 {
        ((ns as f64 / LAGO_STEP_NS) * f64::from(multiplier)) as u32
    }
}

impl Default for FrameTimer {
    fn default() -> Self {
        FrameTimer::new()
    }
}

/// One channel blend step (`blendColors`, `:263-278`).
fn blend_channel(first: i32, second: i32, f: f32) -> i32 {
    ((first as f32 + (second - first) as f32 * f) as i32).clamp(0, 255)
}

/// A bar height's colour: green→yellow across 0..30, yellow→red across
/// 30..60 (`getFrameColor`, `:255-261`).
pub fn frame_color(value: u32) -> i32 {
    let (first, second, f) = if value < LAGO_MID {
        (LAGO_GREEN, LAGO_YELLOW, value as f32 / LAGO_MID as f32)
    } else {
        (
            LAGO_YELLOW,
            LAGO_RED,
            (value - LAGO_MID) as f32 / (LAGO_MAX_PX - LAGO_MID) as f32,
        )
    };
    blend_channel(first >> 24 & 255, second >> 24 & 255, f) << 24
        | blend_channel(first >> 16 & 255, second >> 16 & 255, f) << 16
        | blend_channel(first >> 8 & 255, second >> 8 & 255, f) << 8
        | blend_channel(first & 255, second & 255, f)
}

/// A duration's bar height in px, clamped 0..60 (R2's clamp over
/// `getLagometerValue(ns, 30)`).
pub fn bar_height(ns: u64) -> u32 {
    FrameTimer::get_lagometer_value(ns, LAGO_MID).min(LAGO_MAX_PX)
}

/// A packed `0xAARRGGBB` source colour as straight RGBA floats.
pub fn argb_to_rgba(argb: i32) -> [f32; 4] {
    [
        ((argb >> 16) & 255) as f32 / 255.0,
        ((argb >> 8) & 255) as f32 / 255.0,
        (argb & 255) as f32 / 255.0,
        ((argb >> 24) & 255) as f32 / 255.0,
    ]
}

/// The lagometer's GUI draws at the given screen height `h`: the
/// background, one 1-px bar per stored frame oldest→newest, the "60"/"30"
/// labels, the white frame lines all round, and the blue framerate-limit
/// line at `h−60+limit/2` when the cap is at most 120
/// (`renderLagometer`, `:219-256`). Empty unless the overlay is up and the
/// flag is on (`:48-51`).
pub fn lagometer_draws(
    overlay_up: bool,
    lagometer_on: bool,
    timer: &FrameTimer,
    h: f32,
    framerate_limit: u32,
) -> Vec<HudDraw> {
    if !overlay_up || !lagometer_on {
        return Vec::new();
    }
    let mut draws = Vec::new();
    draws.push(HudDraw::Rect {
        x: 0.0,
        y: h - 60.0,
        width: 240.0,
        height: 60.0,
        colour: argb_to_rgba(LAGO_BG),
    });
    for (i, ns) in timer.ordered().iter().enumerate() {
        let height = bar_height(*ns);
        draws.push(HudDraw::Rect {
            x: i as f32,
            y: h - height as f32,
            width: 1.0,
            height: height as f32,
            colour: argb_to_rgba(frame_color(height)),
        });
    }
    for (label, row) in [("60", h - 30.0), ("30", h - 60.0)] {
        draws.push(HudDraw::Text {
            text: label.to_string(),
            x: 2.0,
            y: row + 2.0,
            scale: 1.0,
            colour: argb_to_rgba(LAGO_LABEL),
            shadow: false,
            blend: false,
        });
    }
    let white = [1.0, 1.0, 1.0, 1.0];
    for (x, y, width, height) in [
        (0.0, h - 30.0, 240.0, 1.0),
        (0.0, h - 60.0, 240.0, 1.0),
        (0.0, h - 1.0, 240.0, 1.0),
        (0.0, h - 60.0, 1.0, 60.0),
        (239.0, h - 60.0, 1.0, 60.0),
    ] {
        draws.push(HudDraw::Rect {
            x,
            y,
            width,
            height,
            colour: white,
        });
    }
    if framerate_limit <= 120 {
        draws.push(HudDraw::Rect {
            x: 0.0,
            y: h - 60.0 + framerate_limit as f32 / 2.0,
            width: 240.0,
            height: 1.0,
            colour: argb_to_rgba(LAGO_BLUE),
        });
    }
    draws
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::HudDraw;

    /// A bar's centre row over the sky: the opaque fill comes back exact.
    fn bar_over_sky(fill: [u8; 3]) -> [u8; 3] {
        fill
    }

    #[test]
    fn the_ring_holds_240_frames_oldest_first() {
        // `FrameTimer`'s array (`FrameTimer.java`:5-7): the last 240
        // per-frame durations, oldest first, wrapping past the end.
        let mut timer = FrameTimer::new();
        for ns in 1u64..=240 {
            timer.add_frame(ns);
        }
        assert_eq!(timer.len(), 240, "the ring is bounded");
        let ordered: Vec<u64> = timer.ordered();
        assert_eq!(ordered.len(), 240);
        assert_eq!(ordered[0], 1, "the oldest kept frame");
        assert_eq!(ordered[239], 240, "the newest frame");
        timer.add_frame(241);
        let ordered = timer.ordered();
        assert_eq!(ordered.len(), 240, "the ring stays bounded past 240");
        assert_eq!(ordered[0], 2, "the wrap drops the oldest");
        assert_eq!(ordered[239], 241, "the newest frame");
    }

    #[test]
    fn the_ring_reports_its_partial_fill_in_order() {
        let mut timer = FrameTimer::new();
        assert!(timer.is_empty());
        assert!(timer.ordered().is_empty());
        timer.add_frame(7);
        timer.add_frame(9);
        assert_eq!(timer.len(), 2);
        assert_eq!(timer.ordered(), vec![7, 9]);
    }

    #[test]
    fn parse_index_wraps_at_240() {
        // `parseIndex` (`FrameTimer.java`:71-74): raw index mod 240.
        assert_eq!(FrameTimer::parse_index(0), 0);
        assert_eq!(FrameTimer::parse_index(239), 239);
        assert_eq!(FrameTimer::parse_index(240), 0);
        assert_eq!(FrameTimer::parse_index(241), 1);
    }

    #[test]
    fn the_bar_height_law_divides_by_a_sixtieth_step() {
        // `getLagometerValue(ns, 30)` (`FrameTimer.java`:49-53): a 60 Hz
        // frame reads 30 px, clamped 0..60.
        assert_eq!(FrameTimer::get_lagometer_value(0, 30), 0);
        assert_eq!(FrameTimer::get_lagometer_value(8_333_333, 30), 15);
        assert_eq!(FrameTimer::get_lagometer_value(16_666_666, 30), 30);
        assert_eq!(bar_height(16_666_666), 30);
        assert_eq!(bar_height(100_000_000), 60, "past a second clamps");
        assert_eq!(bar_height(0), 0);
    }

    #[test]
    fn the_frame_colours_break_at_thirty_pixels() {
        // `getFrameColor` (`GuiOverlayDebug.java`:258-262): green to
        // yellow across 0..30, yellow to red across 30..60.
        assert_eq!(frame_color(0), LAGO_GREEN);
        assert_eq!(frame_color(30), LAGO_YELLOW);
        assert_eq!(frame_color(60), LAGO_RED);
        assert_eq!(frame_color(15), -8_388_864, "halfway green to yellow");
        assert_eq!(frame_color(31), -2_560, "one step past yellow");
        assert_eq!(frame_color(90), LAGO_RED, "past the top stays red");
        let _ = bar_over_sky([0, 0, 0]);
    }

    #[test]
    fn the_graph_draws_only_with_the_overlay_up() {
        // `renderDebugInfo` draws the graph only inside the F3 overlay
        // (`GuiOverlayDebug.java`:48-51): the flag implies the overlay.
        let mut timer = FrameTimer::new();
        timer.add_frame(16_666_666);
        assert!(lagometer_draws(false, true, &timer, 120.0, 100).is_empty());
        assert!(lagometer_draws(true, false, &timer, 120.0, 100).is_empty());
        assert!(lagometer_draws(false, false, &timer, 120.0, 100).is_empty());
        assert!(!lagometer_draws(true, true, &timer, 120.0, 100).is_empty());
    }

    /// Whether `draws` holds a rect at the given geometry, within a texel.
    fn has_rect(draws: &[HudDraw], x: f32, y: f32, width: f32, height: f32) -> bool {
        draws.iter().any(|draw| match draw {
            HudDraw::Rect {
                x: dx,
                y: dy,
                width: dw,
                height: dh,
                ..
            } => {
                (dx - x).abs() < 1e-6
                    && (dy - y).abs() < 1e-6
                    && (dw - width).abs() < 1e-6
                    && (dh - height).abs() < 1e-6
            }
            _ => false,
        })
    }

    /// Whether `draws` holds `text` starting at the given pen.
    fn has_label(draws: &[HudDraw], text: &str, x: f32, y: f32) -> bool {
        draws.iter().any(|draw| match draw {
            HudDraw::Text {
                text: body,
                x: dx,
                y: dy,
                ..
            } => body == text && (dx - x).abs() < 1e-6 && (dy - y).abs() < 1e-6,
            _ => false,
        })
    }

    #[test]
    fn the_background_bars_and_labels_follow_the_sources_rects() {
        // `renderLagometer` (`GuiOverlayDebug.java`:219-256): the bg rect,
        // one 1-px bar per stored frame oldest to newest, the "60" row at
        // h−30 and the "30" row at h−60, the white frame lines all round.
        let mut timer = FrameTimer::new();
        timer.add_frame(0);
        timer.add_frame(8_333_333);
        timer.add_frame(16_666_666);
        timer.add_frame(100_000_000);
        let draws = lagometer_draws(true, true, &timer, 120.0, 255);
        assert!(
            has_rect(&draws, 0.0, 60.0, 240.0, 60.0),
            "the (0, h-60, 240, h) background"
        );
        assert!(has_rect(&draws, 0.0, 120.0, 1.0, 0.0), "the 0 ns bar");
        assert!(has_rect(&draws, 1.0, 105.0, 1.0, 15.0), "the 15 px bar");
        assert!(has_rect(&draws, 2.0, 90.0, 1.0, 30.0), "the 30 px bar");
        assert!(has_rect(&draws, 3.0, 60.0, 1.0, 60.0), "the clamped bar");
        assert!(has_label(&draws, "60", 2.0, 92.0), "the h-30 label");
        assert!(has_label(&draws, "30", 2.0, 62.0), "the h-60 label");
        assert!(has_rect(&draws, 0.0, 90.0, 240.0, 1.0), "the h-30 line");
        assert!(has_rect(&draws, 0.0, 60.0, 240.0, 1.0), "the h-60 line");
        assert!(has_rect(&draws, 0.0, 119.0, 240.0, 1.0), "the bottom line");
        assert!(has_rect(&draws, 0.0, 60.0, 1.0, 60.0), "the left line");
        assert!(has_rect(&draws, 239.0, 60.0, 1.0, 60.0), "the right line");
    }

    #[test]
    fn the_cap_line_needs_a_capped_framerate() {
        // The blue framerate-limit line at h−60+limit/2, only when the cap
        // is at most 120 (`GuiOverlayDebug.java`:250-253).
        let mut timer = FrameTimer::new();
        timer.add_frame(16_666_666);
        let blue = argb_to_rgba(LAGO_BLUE);
        let capped = lagometer_draws(true, true, &timer, 120.0, 100);
        assert!(
            capped.iter().any(|draw| matches!(
                draw,
                HudDraw::Rect { y, colour, .. }
                if (*y - 110.0).abs() < 1e-6 && *colour == blue
            )),
            "the 100 cap line sits at h-60+50"
        );
        let at_default = lagometer_draws(true, true, &timer, 120.0, 120);
        assert!(
            at_default.iter().any(|draw| matches!(
                draw,
                HudDraw::Rect { y, colour, .. }
                if (*y - 120.0).abs() < 1e-6 && *colour == blue
            )),
            "the default 120 cap line sits at the bottom row"
        );
        for limit in [121, 200, 255] {
            let draws = lagometer_draws(true, true, &timer, 120.0, limit);
            assert!(
                draws.iter().all(|draw| !matches!(
                    draw,
                    HudDraw::Rect { colour, .. } if *colour == blue
                )),
                "no blue line past 120 (limit {limit})"
            );
        }
    }
}
