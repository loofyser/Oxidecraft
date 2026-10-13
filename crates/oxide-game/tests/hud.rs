//! The F3 overlay's exact field set: the left column in the source's order,
//! the right column in its order, and the reduced-debug collapse.

use oxide_game::hud::{
    HudState, additional_difficulty, bytes_to_mb, debug_left, debug_right, moon_factor,
    wrap_angle_180,
};

fn state() -> HudState {
    HudState {
        fps: 60.2,
        dimension: 0,
        position: [-23.5, 71.0625, 118.5],
        yaw: 0.0,
        pitch: 12.34,
        chunks_meshed: 12,
        chunks_total: 81,
        view_distance: 8,
        entities_rendered: 5,
        entities_total: 5,
        particles: 42,
        provider: "default".to_string(),
        difficulty: 2,
        world_time: 6000,
        moon_phase: 0,
        view_loaded: true,
        view_biome: 1,
        view_sky_light: 15,
        view_block_light: 7,
        aim: Some([-24, 70, 118]),
        aim_block: Some((1 << 4) | 3),
        server_reduced_debug: false,
        rustc_version: "1.85.0".to_string(),
        build_id: "0.1.0".to_string(),
        mem_used_bytes: Some(2 * 1024 * 1024),
        mem_max_bytes: Some(16 * 1024 * 1024),
        mem_alloc_bytes: Some(4 * 1024 * 1024),
        cpu_name: Some("Test CPU".to_string()),
        display_size: Some((1280, 720)),
        display_backend: "Vulkan".to_string(),
        adapter_name: "Test Adapter".to_string(),
        adapter_driver: "Test Driver".to_string(),
    }
}

#[test]
fn the_left_column_carries_the_full_field_set_in_order() {
    let lines = debug_left(&state(), false);
    assert_eq!(
        lines[0],
        format!(
            "Oxidecraft 1.8.9 ({}/Oxidecraft)",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(lines[1], "60 fps");
    assert_eq!(lines[2], "C: 12/81 D: 8");
    assert_eq!(lines[3], "E: 5/5");
    assert_eq!(lines[4], "P: 42. T: All: 5");
    assert_eq!(lines[5], "default");
    assert_eq!(lines[6], "");
    assert_eq!(lines[7], "XYZ: -23.500 / 71.06250 / 118.500");
    assert_eq!(lines[8], "Block: -24 71 118");
    assert_eq!(lines[9], "Chunk: 8 7 6 in -2 4 7");
    assert_eq!(lines[10], "Facing: south (Towards positive Z) (0.0 / 12.3)");
    assert_eq!(lines[11], "Biome: Plains");
    assert_eq!(lines[12], "Light: 15 (15 sky, 7 block)");
    assert_eq!(lines[13], "Local Difficulty: 1.50 (Day 0)");
    assert_eq!(lines[14], "Looking at: -24 70 118");
    assert_eq!(
        lines.len(),
        15,
        "the full variant's thirteen fields plus the separator"
    );
}

#[test]
fn the_right_column_carries_its_field_set_in_order() {
    let lines = debug_right(&state());
    assert_eq!(lines[0], "Java: rustc 1.85.0 64bit (0.1.0)");
    assert_eq!(lines[1], "Mem: 12% 002/016MB");
    assert_eq!(lines[2], "Allocated: 25% 004MB");
    assert_eq!(lines[3], "");
    assert_eq!(lines[4], "CPU: Test CPU");
    assert_eq!(lines[5], "");
    assert_eq!(lines[6], "Display: 1280x720 (Vulkan)");
    assert_eq!(lines[7], "Test Adapter");
    assert_eq!(lines[8], "Test Driver");
    assert_eq!(lines[9], "");
    assert_eq!(lines[10], "stone");
    assert_eq!(lines[11], "variant: diorite");
    assert_eq!(
        lines.len(),
        12,
        "nine environment rows plus the block section"
    );
}

#[test]
fn an_unloaded_view_block_omits_the_conditional_tail() {
    let mut fixture = state();
    fixture.view_loaded = false;
    let lines = debug_left(&fixture, false);
    assert_eq!(
        lines.len(),
        12,
        "no Biome, Light or Difficulty without a loaded block"
    );
    assert!(!lines.iter().any(|line| line.starts_with("Biome: ")));
    assert!(!lines.iter().any(|line| line.starts_with("Light: ")));
    assert!(
        !lines
            .iter()
            .any(|line| line.starts_with("Local Difficulty: "))
    );
}

#[test]
fn no_aim_omits_the_target_lines_on_both_columns() {
    let mut fixture = state();
    fixture.aim = None;
    fixture.aim_block = None;
    let left = debug_left(&fixture, false);
    assert_eq!(left.len(), 14, "no Looking at without a targeted block");
    assert!(!left.iter().any(|line| line.starts_with("Looking at: ")));
    let right = debug_right(&fixture);
    assert_eq!(right.len(), 9, "no block section without a targeted block");
}

#[test]
fn missing_environment_facts_read_as_not_available() {
    let mut fixture = state();
    fixture.mem_used_bytes = None;
    fixture.mem_max_bytes = None;
    fixture.mem_alloc_bytes = None;
    fixture.cpu_name = None;
    fixture.display_size = None;
    let lines = debug_right(&fixture);
    assert_eq!(lines[1], "Mem: n/a");
    assert_eq!(lines[2], "Allocated: n/a");
    assert_eq!(lines[4], "CPU: n/a");
    assert_eq!(lines[6], "Display: n/a");
}

#[test]
fn the_reduced_variant_collapses_the_left_column() {
    let lines = debug_left(&state(), true);
    assert_eq!(
        lines[0],
        format!(
            "Oxidecraft 1.8.9 ({}/Oxidecraft)",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert_eq!(lines[1], "60 fps");
    assert_eq!(lines[2], "C: 12/81 D: 8");
    assert_eq!(lines[3], "E: 5/5");
    assert_eq!(lines[4], "P: 42. T: All: 5");
    assert_eq!(lines[5], "default");
    assert_eq!(lines[6], "");
    assert_eq!(lines[7], "Chunk-relative: 8 7 6");
    assert_eq!(
        lines.len(),
        8,
        "the reduced collapse carries no position tail"
    );
    let right = debug_right(&state());
    assert_eq!(
        right.len(),
        12,
        "the right column is unchanged by the collapse"
    );
}

#[test]
fn negative_coordinates_use_floor_and_euclidean_remainders() {
    let mut fixture = state();
    fixture.position = [-0.5, 64.0, -0.5];
    let lines = debug_left(&fixture, false);
    assert_eq!(lines[8], "Block: -1 64 -1");
    assert_eq!(lines[9], "Chunk: 15 0 15 in -1 4 -1");
}

#[test]
fn facing_follows_the_vanilla_compass_with_the_towards_phrase() {
    for (yaw, expected, phrase) in [
        (0.0, "south", "Towards positive Z"),
        (90.0, "west", "Towards negative X"),
        (180.0, "north", "Towards negative Z"),
        (-90.0, "east", "Towards positive X"),
    ] {
        let mut fixture = state();
        fixture.yaw = yaw;
        let line = &debug_left(&fixture, false)[10];
        assert!(
            line.starts_with(&format!("Facing: {expected} ({phrase}) ")),
            "yaw {yaw}: {line}"
        );
    }
}

#[test]
fn the_displayed_angles_wrap_to_minus_180_to_180() {
    let mut fixture = state();
    fixture.yaw = 270.0;
    fixture.pitch = -190.0;
    let line = &debug_left(&fixture, false)[10];
    assert!(
        line.starts_with("Facing: east (Towards positive X) (-90.0 / 170.0)"),
        "{line}"
    );
}

#[test]
fn peaceful_difficulty_reads_zero_whatever_the_clock() {
    let mut fixture = state();
    fixture.difficulty = 0;
    fixture.world_time = 1_000_000;
    let lines = debug_left(&fixture, false);
    assert_eq!(lines[13], "Local Difficulty: 0.00 (Day 41)");
}

#[test]
fn the_byte_to_mb_rule_divides_twice_by_1024() {
    assert_eq!(bytes_to_mb(0), 0);
    assert_eq!(bytes_to_mb(1024 * 1024 - 1), 0);
    assert_eq!(bytes_to_mb(2 * 1024 * 1024), 2);
    assert_eq!(bytes_to_mb(16 * 1024 * 1024), 16);
}

#[test]
fn the_angle_wrap_matches_the_source_rule() {
    assert_eq!(wrap_angle_180(0.0), 0.0);
    assert_eq!(wrap_angle_180(270.0), -90.0);
    assert_eq!(wrap_angle_180(-190.0), 170.0);
    assert_eq!(wrap_angle_180(180.0), -180.0);
    assert_eq!(wrap_angle_180(360.0), 0.0);
}

#[test]
fn the_moon_factor_table_matches_the_source_values() {
    for (phase, expected) in [
        (0, 1.0),
        (1, 0.75),
        (2, 0.5),
        (3, 0.25),
        (4, 0.0),
        (5, 0.25),
        (6, 0.5),
        (7, 0.75),
    ] {
        assert_eq!(moon_factor(phase), expected, "phase {phase}");
    }
}

#[test]
fn the_difficulty_formula_matches_the_source_arithmetic() {
    assert_eq!(
        additional_difficulty(0, 1_000_000, 1.0),
        0.0,
        "peaceful reads zero"
    );
    assert_eq!(
        additional_difficulty(2, 6000, 1.0),
        1.5,
        "normal at dawn with a full moon"
    );
    assert_eq!(
        additional_difficulty(1, 6000, 1.0),
        0.75,
        "easy matches normal before the halving"
    );
    assert_eq!(
        additional_difficulty(3, 6000, 1.0),
        2.25,
        "hard takes the full second half"
    );
    assert!(
        additional_difficulty(2, 18_000_000, 1.0) > 2.0,
        "a late clock raises the time term"
    );
}
