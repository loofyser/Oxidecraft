//! Tests for the block behaviour table: its coverage, the metadata-to-blockstate
//! mapping it builds, the liquid surface rule, and the table's own invariants.
//!
//! The literal keys asserted here are the ones the decompiled source's
//! `getMetaFromState`/`getStateFromMeta` pairs produce (`refs/_src/MCP-919`,
//! `src/minecraft/net/minecraft/block/*`), written in the order the client's own
//! `StateMapperBase.getPropertyString` writes them: the property names alphabetically
//! (`BlockState`'s constructor sorts its property array by name — `block/state/BlockState.java`).

use oxide_world::behaviour::{
    LiquidKind, Material, RenderKind, RenderLayer, TintKind, behaviour, covered_ids,
    liquid_height_percent, liquid_kind, variant_key,
};

/// The covered ids, sorted: the M1 palette table's ids (`oxide-game`'s palette at
/// M1's head carried all 73) union the non-air ids of the M1 acceptance world scan
/// (`refs/rig/evidence/m1/task12-world-id-scan.txt`). The scan added no id the
/// palette did not already carry, so the union is exactly this list.
const COVERED: [u16; 73] = [
    1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 24, 31, 32, 35, 37, 38, 39,
    40, 41, 42, 43, 45, 46, 47, 48, 49, 50, 52, 53, 54, 56, 57, 58, 59, 60, 61, 62, 64, 65, 67, 72,
    73, 79, 80, 81, 82, 83, 85, 86, 87, 88, 89, 98, 99, 100, 102, 110, 129, 141, 142, 155, 161,
    162, 175,
];

/// The non-air ids the M1 acceptance world scan reported, in the scan's order.
const SCAN_IDS: [u16; 55] = [
    1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 21, 24, 31, 32, 35, 37, 38, 39, 40,
    43, 48, 49, 50, 52, 53, 54, 56, 58, 59, 60, 64, 65, 67, 72, 73, 81, 82, 83, 85, 86, 99, 100,
    102, 141, 142, 161, 162, 175,
];

fn block(id: u16) -> &'static oxide_world::behaviour::BlockBehaviour {
    behaviour(id).unwrap_or_else(|| panic!("id {id} is covered, so it has a table entry"))
}

#[test]
fn covered_ids_is_the_palette_and_scan_union() {
    assert_eq!(covered_ids(), COVERED.as_slice());
}

#[test]
fn covered_ids_is_sorted_with_no_duplicates() {
    let ids = covered_ids();
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "covered_ids() must be sorted and unique: {ids:?}"
    );
}

#[test]
fn every_covered_id_has_an_entry() {
    for &id in covered_ids() {
        let entry = behaviour(id).unwrap_or_else(|| panic!("id {id} has an entry"));
        assert_eq!(entry.id, id, "the entry answers for its own id");
        assert!(!entry.name.is_empty(), "id {id} has a name");
        assert!(
            entry.properties.len() <= 8,
            "id {id} has at most the eight properties a 1.8 state carries"
        );
        assert!(
            entry.light_filter == entry.light_opacity.min(15),
            "id {id}: the light engine's filter is the source opacity clamped to 0..=15"
        );
        assert_eq!(
            entry.liquid,
            liquid_kind(id),
            "id {id}: the row's liquid column and liquid_kind agree"
        );
        assert!(
            entry.render != RenderKind::Liquid || liquid_kind(id).is_some(),
            "id {id}: only the two liquids take the liquid path"
        );
    }
}

#[test]
fn the_acceptance_world_scan_ids_are_covered() {
    for &id in &SCAN_IDS {
        assert!(
            behaviour(id).is_some(),
            "the acceptance world carries id {id}, so the table covers it"
        );
    }
}

#[test]
fn ids_outside_the_set_have_no_entry() {
    // 44 is the short stone slab: the M1 palette and the acceptance world scan
    // carry the double slab (43), so the short slab is outside the covered set.
    for id in [
        0, 6, 19, 22, 23, 30, 44, 74, 92, 125, 143, 163, 197, 255, 4096,
    ] {
        assert!(
            behaviour(id).is_none(),
            "id {id} is outside the covered set"
        );
    }
}

#[test]
fn air_has_no_entry() {
    assert!(behaviour(0).is_none(), "air (0) is not a covered block");
}

#[test]
fn a_block_with_no_properties_has_an_empty_key() {
    assert_eq!(variant_key(block(7), 0), "");
    assert_eq!(variant_key(block(7), 15), "");
    assert_eq!(variant_key(block(4), 3), "");
}

#[test]
fn the_pinned_variant_keys_match_the_source() {
    let cases: &[(u16, u8, &str)] = &[
        // The plan's list, each checked against the source while writing this test.
        (35, 14, "color=red"),
        (17, 1, "axis=y,variant=spruce"),
        (17, 4, "axis=x,variant=oak"),
        // meta 3 is DIORITE: the enum's metadata order is stone, granite,
        // smooth_granite, diorite, smooth_diorite, andesite, smooth_andesite
        // (BlockStone.EnumType).
        (1, 3, "variant=diorite"),
        (5, 2, "variant=birch"),
        (98, 2, "variant=cracked_stonebrick"),
        (2, 0, "snowy=false"),
        (9, 0, "level=0"),
        // meta 2: the half bit is clear, and the facing is getFront(5 - (meta & 3))
        // = getFront(3) = south (BlockStairs.getStateFromMeta; EnumFacing's
        // declaration order is down, up, north, south, west, east).
        (53, 2, "facing=south,half=bottom,shape=straight"),
        // The plan pins `44:8 -> half=top`; id 44 (the short stone slab) is not a
        // covered id — the M1 palette and the acceptance world scan carry 43, the
        // double slab — so the slab's pins are 43's states. The short slab's own
        // state is HALF + VARIANT and its mapper key would be `half=top`, as the
        // plan says; the table gains 44 when its id first appears.
        (43, 0, "seamless=false,variant=stone"),
        (43, 8, "seamless=true,variant=stone"),
        // id 24 is sandstone, and its property is named `type`.
        (24, 1, "type=chiseled_sandstone"),
        // The rest pin the shapes the plan's list does not reach.
        (3, 0, "snowy=false,variant=dirt"),
        (3, 2, "snowy=false,variant=podzol"),
        // DirtType's metadata is 0..=2; the lookup clamps the rest to dirt.
        (3, 4, "snowy=false,variant=dirt"),
        (18, 0, "check_decay=false,decayable=true,variant=oak"),
        (18, 12, "check_decay=true,decayable=false,variant=oak"),
        (
            64,
            0,
            "facing=east,half=lower,hinge=left,open=false,powered=false",
        ),
        (
            64,
            1,
            "facing=south,half=lower,hinge=right,open=false,powered=false",
        ),
        (
            64,
            9,
            "facing=north,half=upper,hinge=right,open=false,powered=false",
        ),
        (
            64,
            10,
            "facing=north,half=upper,hinge=left,open=false,powered=true",
        ),
        (50, 0, "facing=up"),
        (50, 1, "facing=east"),
        (50, 5, "facing=up"),
        (86, 0, "facing=south"),
        (61, 2, "facing=north"),
        (61, 3, "facing=south"),
        (61, 15, "facing=south"),
        (162, 1, "axis=y,variant=dark_oak"),
        (161, 0, "check_decay=false,decayable=true,variant=acacia"),
        (59, 9, "age=1"),
        (60, 9, "moisture=1"),
        (81, 15, "age=15"),
        (85, 0, "east=false,north=false,south=false,west=false"),
        (102, 7, "east=false,north=false,south=false,west=false"),
        (99, 14, "variant=all_outside"),
        (100, 1, "variant=north_west"),
        (155, 2, "variant=lines_y"),
        (175, 0, "facing=north,half=lower,variant=sunflower"),
        (175, 8, "facing=north,half=upper,variant=sunflower"),
        (31, 1, "type=tall_grass"),
        (38, 1, "type=blue_orchid"),
        (38, 9, "type=poppy"),
        (39, 0, ""),
        (52, 0, ""),
        (46, 1, "explode=true"),
        (83, 1, "age=1"),
        (72, 1, "powered=true"),
        (2, 8, "snowy=false"),
        (110, 15, "snowy=false"),
    ];
    for (id, meta, expected) in cases {
        assert_eq!(
            variant_key(block(*id), *meta),
            *expected,
            "id {id} meta {meta}: the source's own property string"
        );
    }
}

#[test]
fn liquid_height_percent_pins_the_source_rule() {
    // BlockLiquid.getLiquidHeightPercent(meta) is (meta + 1) / 9 for meta 0..=7 and
    // treats 8..=15 (a falling column) as a source; the surface height is
    // 1 - that (BlockFluidRenderer.getFluidHeight, EntityRainFX use the complement).
    assert_eq!(liquid_height_percent(0), 0.888_888_9);
    assert_eq!(liquid_height_percent(0), 1.0 - 1.0 / 9.0);
    assert_eq!(liquid_height_percent(7), 0.111_111_104);
    assert_eq!(liquid_height_percent(7), 1.0 - 8.0 / 9.0);
    // Levels 8..=15 are falling columns and take the source's height.
    assert_eq!(liquid_height_percent(8), liquid_height_percent(0));
    assert_eq!(liquid_height_percent(15), liquid_height_percent(0));
    // The rule is monotone over the flow levels.
    for level in 1..8u8 {
        assert!(
            liquid_height_percent(level) < liquid_height_percent(level - 1),
            "level {level} sits below level {}",
            level - 1
        );
    }
}

#[test]
fn liquid_kind_names_the_two_liquids() {
    assert_eq!(liquid_kind(8), Some(LiquidKind::Water));
    assert_eq!(liquid_kind(9), Some(LiquidKind::Water));
    assert_eq!(liquid_kind(10), Some(LiquidKind::Lava));
    assert_eq!(liquid_kind(11), Some(LiquidKind::Lava));
    for id in [0, 1, 2, 20, 54, 79, 89] {
        assert_eq!(liquid_kind(id), None, "id {id} is not a liquid");
    }
}

#[test]
fn the_light_and_visual_columns_hold_their_source_values() {
    // Leaves: lightOpacity 1 (BlockLeaves' constructor), Fast semantics render them
    // solid (BlockLeaves.getBlockLayer: `isTransparent ? CUTOUT_MIPPED : SOLID`).
    let leaves = block(18);
    assert_eq!(leaves.light_opacity, 1);
    assert_eq!(leaves.render_layer, RenderLayer::Solid);
    assert_eq!(leaves.tint, TintKind::Foliage);
    // Water: lightOpacity 3 (the registration in Block.java's static block).
    let water = block(9);
    assert_eq!(water.light_opacity, 3);
    assert_eq!(water.render_layer, RenderLayer::Translucent);
    assert_eq!(water.tint, TintKind::Water);
    assert_eq!(water.render, RenderKind::Liquid);
    assert_eq!(water.liquid, Some(LiquidKind::Water));
    // Lava is the opaque liquid.
    assert_eq!(block(11).render_layer, RenderLayer::Solid);
    assert_eq!(block(11).tint, TintKind::None);
    // Glass: lightOpacity 0, cutout.
    let glass = block(20);
    assert_eq!(glass.light_opacity, 0);
    assert_eq!(glass.render_layer, RenderLayer::Cutout);
    // Slabs and stairs are light-opaque even though they are not full cubes.
    assert_eq!(block(43).light_opacity, 255);
    assert_eq!(block(53).light_opacity, 255);
    assert!(!block(53).full_cube);
    // Ice is translucent with the registration's opacity 3.
    assert_eq!(block(79).light_opacity, 3);
    assert_eq!(block(79).render_layer, RenderLayer::Translucent);
    // Emissions: torch 14, lit furnace 13, lava 15, glowstone 15, brown mushroom 1.
    assert_eq!(block(50).light_emission, 14);
    assert_eq!(block(62).light_emission, 13);
    assert_eq!(block(11).light_emission, 15);
    assert_eq!(block(89).light_emission, 15);
    assert_eq!(block(39).light_emission, 1);
    assert_eq!(block(73).light_emission, 0);
    // The grass block's tinted faces are the top and the side overlay.
    assert_eq!(block(2).tint, TintKind::GrassSideOverlay);
    // The cross-quad plants carry the cross render kind.
    for id in [31, 32, 37, 38, 39, 40, 59, 83, 141, 142, 175] {
        assert_eq!(
            block(id).render,
            RenderKind::Cross,
            "id {id} is a cross plant"
        );
    }
    assert_eq!(
        block(50).render,
        RenderKind::Model,
        "a torch is not a cross plant"
    );
    assert_eq!(
        block(65).render,
        RenderKind::Model,
        "a ladder is not a cross plant"
    );
    // The materials the source's constructors name.
    assert_eq!(block(1).material, Material::Rock);
    assert_eq!(block(2).material, Material::Grass);
    assert_eq!(block(5).material, Material::Wood);
    assert_eq!(block(35).material, Material::Cloth);
    assert_eq!(block(46).material, Material::Tnt);
    assert_eq!(block(50).material, Material::Circuit);
    assert_eq!(block(80).material, Material::CraftedSnow);
    assert_eq!(block(82).material, Material::Clay);
    assert_eq!(block(86).material, Material::Gourd);
    assert_eq!(block(31).material, Material::Vine);
}
