//! The overlay's exact text.

use oxide_game::hud::{HudState, debug_lines};

fn state() -> HudState {
    HudState {
        fps: 60.2,
        position: [-23.5, 71.0625, 118.5],
        yaw: 0.0,
        pitch: 12.34,
        dimension: 0,
        server: "127.0.0.1:25565".into(),
        entity_id: 20,
    }
}

#[test]
fn the_overlay_reports_the_vanilla_style_lines_in_order() {
    let lines = debug_lines(&state());
    assert_eq!(lines[0], "Oxidecraft 1.8.9");
    assert_eq!(lines[1], "60 fps");
    assert_eq!(lines[2], "x/y/z: -23.500 / 71.06250 / 118.500");
    assert_eq!(lines[3], "Block: -24 71 118");
    assert_eq!(lines[4], "Chunk: -2 7 in 8 6");
    assert_eq!(lines[5], "Facing: south (0.0 / 12.3)");
    assert_eq!(lines[6], "Dimension: Overworld");
    assert_eq!(lines[7], "Server: 127.0.0.1:25565 (protocol 47)");
    assert_eq!(lines[8], "Entity: 20");
}

#[test]
fn negative_coordinates_use_floor_and_euclidean_remainders() {
    let mut state = state();
    state.position = [-0.5, 64.0, -0.5];
    let lines = debug_lines(&state);
    assert_eq!(lines[3], "Block: -1 64 -1");
    assert_eq!(lines[4], "Chunk: -1 -1 in 15 15");
}

#[test]
fn facing_follows_the_vanilla_compass() {
    for (yaw, expected) in [
        (0.0, "south"),
        (90.0, "west"),
        (180.0, "north"),
        (-90.0, "east"),
    ] {
        let mut state = state();
        state.yaw = yaw;
        assert!(
            debug_lines(&state)[5].starts_with(&format!("Facing: {expected} ")),
            "yaw {yaw}"
        );
    }
}

#[test]
fn the_dimension_names_match_vanilla() {
    for (dimension, expected) in [(-1i8, "Nether"), (0, "Overworld"), (1, "The End")] {
        let mut state = state();
        state.dimension = dimension;
        assert_eq!(debug_lines(&state)[6], format!("Dimension: {expected}"));
    }
}
