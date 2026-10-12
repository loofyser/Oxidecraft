//! The music ticker: the source's schedule for background music, without a
//! fade.
//!
//! [`MusicTicker`] mirrors `MusicTicker.java`: it starts 100 ticks out, picks
//! the kind's event when the wait runs down, and re-arms a fresh wait in the
//! kind's pick range when the music ends or the kind changes. The source plays
//! and stops its music with no volume ramp — the negative fade search (no
//! `fade` in `client/audio/`, no paulscode fade wrapper in the tree) found no
//! site — so this ticker starts and stops its plays without one, and
//! [`MusicTicker::stop`] follows the same rule.
//!
//! The end of the music is read off the engine: no voice queued and none
//! mixing means the streamed play finished (a missing event never spawns one
//! and re-arms the same way). A foreign voice alive exactly at the music's
//! end holds the re-arm until it too finishes; the next pick only waits.

use rand::{RngExt, SeedableRng, rngs::StdRng};

use crate::audio::{AudioEngine, PlaySound, SoundCategory};

/// The initial wait before the first pick, in ticks (`MusicTicker.java:14`).
pub const INITIAL_DELAY: i32 = 100;

/// The music kind: the dimension and mode reduced to the four events this
/// client plays (`MusicTicker.java:66-74` lists seven; credits and the
/// dragon fight have no screen behind them here yet, and the nether — which
/// vanilla scores on its own (`NETHER`, `music.game.nether`, 1200–3600,
/// picked by `getAmbientMusicType`, `Minecraft.java:3112`) — has no
/// dimension machine behind it here yet, so it rides [`MusicKind::Game`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicKind {
    /// No world: the menu event.
    Menu,
    /// The overworld (and, for now, the nether): the game event.
    Game,
    /// Creative mode with flight: the creative event.
    Creative,
    /// The end dimension: the end event.
    End,
}

impl MusicKind {
    /// The sound event the kind plays (`MusicTicker.java:68-74`).
    #[must_use]
    pub fn event(&self) -> &'static str {
        match self {
            MusicKind::Menu => "music.menu",
            MusicKind::Game => "music.game",
            MusicKind::Creative => "music.game.creative",
            MusicKind::End => "music.game.end",
        }
    }

    /// The pick range in ticks the kind re-arms into (`MusicTicker.java:68-74`).
    #[must_use]
    pub fn delay_range(&self) -> (i32, i32) {
        match self {
            MusicKind::Menu => (20, 600),
            MusicKind::Game => (12000, 24000),
            MusicKind::Creative => (1200, 3600),
            MusicKind::End => (6000, 24000),
        }
    }
}

/// The music schedule: the wait left, the kind playing, and the pick-roll
/// source.
pub struct MusicTicker {
    /// Ticks left before the next pick; [`i32::MAX`] while music plays.
    delay: i32,
    /// The kind playing, if one was picked.
    current: Option<MusicKind>,
    /// The pick-roll source, seeded at construction so one seed replays one
    /// schedule.
    rng: StdRng,
}

impl MusicTicker {
    /// Builds the ticker over a seed: the wait starts at the initial delay.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            delay: INITIAL_DELAY,
            current: None,
            rng: StdRng::seed_from_u64(seed),
        }
    }

    /// Runs one tick: a kind change stops the old music and re-arms half a
    /// minimum wait, a finished music re-arms a fresh pick, and a wait run
    /// down picks the kind's event (`MusicTicker.java:24-54`).
    pub fn tick(&mut self, kind: MusicKind, volume_scale: f32, engine: &AudioEngine) {
        if let Some(current) = self.current {
            if current.event() != kind.event() {
                engine.stop_async();
                let (min, _) = kind.delay_range();
                self.current = None;
                self.delay = self.pick_in(0, min / 2);
            } else if engine.voices() == 0 && engine.pending() == 0 {
                let (min, max) = kind.delay_range();
                self.current = None;
                self.delay = self.pick_in(min, max).min(self.delay);
            }
        }
        if self.current.is_none() {
            if self.delay <= 0 {
                self.play(kind, volume_scale, engine);
                self.current = Some(kind);
                self.delay = i32::MAX;
            } else {
                self.delay -= 1;
            }
        }
    }

    /// Stops the music and re-arms an immediate pick
    /// (`MusicTicker.java:56-64`).
    pub fn stop(&mut self, engine: &AudioEngine) {
        if self.current.is_some() {
            engine.stop_async();
            self.current = None;
            self.delay = 0;
        }
    }

    /// The ticks left before the next pick; [`i32::MAX`] while music plays.
    #[must_use]
    pub fn delay(&self) -> i32 {
        self.delay
    }

    /// The kind playing, if one was picked.
    #[must_use]
    pub fn current(&self) -> Option<MusicKind> {
        self.current
    }

    /// Picks one integer in a closed range from the schedule's roll source.
    fn pick_in(&mut self, min: i32, max: i32) -> i32 {
        let _ = (min, max);
        self.rng.random_range(min..=max)
    }

    /// Stages the kind's event, unpositioned at full pitch through the music
    /// hint.
    fn play(&self, kind: MusicKind, volume_scale: f32, engine: &AudioEngine) {
        engine.play(PlaySound {
            name: kind.event().to_string(),
            category_hint: SoundCategory::Music,
            pos: None,
            volume: volume_scale,
            pitch: 1.0,
            listener: [0.0; 3],
            yaw: 0.0,
        });
    }
}

#[cfg(test)]
mod tests {
    //! The ticker schedule: the seeded waits and transitions.

    use std::sync::Arc;

    use oxide_assets::sound::SoundIndex;

    use crate::audio::decode::DecodedSound;
    use crate::audio::{AudioDevice, AudioEngine, SilentDevice};

    use super::*;

    /// A one-second 440 Hz full-scale mono sine at 44100 Hz.
    fn sine_440() -> DecodedSound {
        let samples: Vec<f32> = (0..44100)
            .map(|n| ((f64::from(n) * 440.0 * std::f64::consts::TAU) / 44100.0).sin() as f32)
            .collect();
        DecodedSound::from_samples(1, 44100, samples)
    }

    /// An engine over the silent device with the four music events injected.
    fn engine() -> AudioEngine {
        let bytes = br#"{
            "music.menu": { "category": "music", "sounds": ["music/menu/menu1"] },
            "music.game": { "category": "music", "sounds": ["music/game/calm1"] },
            "music.game.creative": { "category": "music", "sounds": ["music/game/creative/creative1"] },
            "music.game.end": { "category": "music", "sounds": ["music/game/end/end1"] }
        }"#;
        let index = Arc::new(SoundIndex::parse(bytes).expect("the fixture parses"));
        let device = Box::new(SilentDevice::open()) as Box<dyn AudioDevice>;
        let mut engine = AudioEngine::new(index, None, Some(device));
        for key in [
            "music/menu/menu1",
            "music/game/calm1",
            "music/game/creative/creative1",
            "music/game/end/end1",
        ] {
            engine.inject(&AudioEngine::cache_key(key), sine_440());
        }
        engine
    }

    /// Runs one ticker tick, then drains the engine the way the client's
    /// frame does.
    fn step(ticker: &mut MusicTicker, kind: MusicKind, engine: &mut AudioEngine) {
        ticker.tick(kind, 1.0, engine);
        engine.tick(crate::audio::ListenerPose {
            pos: [0.0; 3],
            yaw: 0.0,
        });
    }

    #[test]
    fn the_first_pick_lands_past_the_initial_delay() {
        let mut ticker = MusicTicker::new(7);
        let mut engine = engine();
        for _ in 0..100 {
            ticker.tick(MusicKind::Game, 1.0, &engine);
            assert_eq!(
                engine.pending(),
                0,
                "no pick inside the first 100 ticks (delay {})",
                ticker.delay()
            );
            engine.tick(crate::audio::ListenerPose {
                pos: [0.0; 3],
                yaw: 0.0,
            });
        }
        ticker.tick(MusicKind::Game, 1.0, &engine);
        assert_eq!(
            engine.pending(),
            1,
            "the wait runs down on the 101st tick and picks the game event"
        );
        assert_eq!(ticker.current(), Some(MusicKind::Game));
    }

    #[test]
    fn the_game_rearm_lands_inside_its_pick_range() {
        let mut first = MusicTicker::new(11);
        let mut second = MusicTicker::new(11);
        let mut engine = engine();
        for _ in 0..101 {
            step(&mut first, MusicKind::Game, &mut engine);
        }
        assert_eq!(first.current(), Some(MusicKind::Game));
        // The music ends: the engine stops and the ticker re-arms.
        engine.stop_all();
        step(&mut first, MusicKind::Game, &mut engine);
        assert_eq!(first.current(), None, "the finished music clears");
        let delay = first.delay();
        assert!(
            (12000..=24000).contains(&delay),
            "the game re-arm lands in 12000-24000 ticks: {delay}"
        );
        // One seed replays one schedule.
        for _ in 0..101 {
            step(&mut second, MusicKind::Game, &mut engine);
        }
        engine.stop_all();
        step(&mut second, MusicKind::Game, &mut engine);
        assert_eq!(second.delay(), delay, "the same seed re-arms the same wait");
    }

    #[test]
    fn a_mode_change_stops_and_rearms_short() {
        let mut ticker = MusicTicker::new(23);
        let mut engine = engine();
        for _ in 0..101 {
            step(&mut ticker, MusicKind::Game, &mut engine);
        }
        assert_eq!(ticker.current(), Some(MusicKind::Game));
        assert!(engine.voices() > 0, "the game music mixes");
        step(&mut ticker, MusicKind::Creative, &mut engine);
        assert_eq!(
            ticker.current(),
            None,
            "the kind change drops the old music"
        );
        assert_eq!(engine.voices(), 0, "the kind change stops the mix");
        assert!(
            (0..=600).contains(&ticker.delay()),
            "the creative re-arm lands in half its 1200 minimum: {}",
            ticker.delay()
        );
    }

    #[test]
    fn stop_clears_and_rearms_an_immediate_pick() {
        let mut ticker = MusicTicker::new(29);
        let mut engine = engine();
        for _ in 0..101 {
            step(&mut ticker, MusicKind::Menu, &mut engine);
        }
        assert_eq!(ticker.current(), Some(MusicKind::Menu));
        ticker.stop(&engine);
        engine.tick(crate::audio::ListenerPose {
            pos: [0.0; 3],
            yaw: 0.0,
        });
        assert_eq!(ticker.current(), None, "stop drops the music");
        assert_eq!(engine.voices(), 0, "stop silences the mix");
        step(&mut ticker, MusicKind::Menu, &mut engine);
        assert_eq!(
            ticker.current(),
            Some(MusicKind::Menu),
            "the re-armed wait picks at once"
        );
    }

    #[test]
    fn the_menu_cycles_inside_its_short_range() {
        let mut ticker = MusicTicker::new(41);
        let mut engine = engine();
        for _ in 0..101 {
            step(&mut ticker, MusicKind::Menu, &mut engine);
        }
        assert_eq!(ticker.current(), Some(MusicKind::Menu));
        engine.stop_all();
        step(&mut ticker, MusicKind::Menu, &mut engine);
        let delay = ticker.delay();
        assert!(
            (20..=600).contains(&delay),
            "the menu re-arm lands in 20-600 ticks: {delay}"
        );
        // The cycle completes: the re-armed wait picks again.
        for _ in 0..delay {
            assert_eq!(
                ticker.current(),
                None,
                "nothing plays inside the re-armed wait"
            );
            step(&mut ticker, MusicKind::Menu, &mut engine);
        }
        step(&mut ticker, MusicKind::Menu, &mut engine);
        assert_eq!(
            ticker.current(),
            Some(MusicKind::Menu),
            "the menu wait picks again at its end"
        );
    }
}
