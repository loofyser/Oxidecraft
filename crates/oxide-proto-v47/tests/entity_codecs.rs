//! The entity codec fixture corpus: one hand-built byte vector per spawn
//! packet and the spawn-id table suite.
//!
//! Every field value is hand-picked to catch unit errors — a fixed-point
//! coordinate with a fraction, a negative angle byte, a mob with two metadata
//! entries and its terminator, an object with velocity on all axes, a painting
//! facing 3, an orb count of 32767 — and every id in the tables is pinned as a
//! literal transcribed from `docs/research/protocol-47-reference.md` §2.1 and
//! §6.3.

use oxide_proto_v47::PacketError;
use oxide_proto_v47::entity::{
    EntityMetadata, GlobalType, MetadataItem, MetadataValue, MobType, ObjectType, SpawnGlobal,
    SpawnMob, SpawnObject, SpawnPainting, SpawnPlayer, SpawnXpOrb, decode_entity_metadata,
    decode_spawn_global, decode_spawn_mob, decode_spawn_object, decode_spawn_painting,
    decode_spawn_player, decode_spawn_xp_orb,
};

/// Spawn Player (0x0C): EID 20; UUID; a fractional fixed-point triple; a
/// negative angle byte; a held item; two metadata entries.
const SPAWN_PLAYER: &[u8] = &[
    0x14, // EID 20
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, // UUID 01020304-0506-0708-…
    0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, // …-090a-0b0c0d0e0f10
    0x00, 0x00, 0x01, 0x90, // x = 400 / 32 = 12.5
    0x00, 0x00, 0x08, 0x20, // y = 2080 / 32 = 65.0
    0xff, 0xff, 0xff, 0xd0, // z = -48 / 32 = -1.5
    0xd0, // yaw: byte 208, 292.5 degrees (or -67.5, one turn down)
    0x40, // pitch: byte 64, 90.0 degrees
    0x01, 0x14, // current item 276
    0x00, 0x01, // metadata: byte index 0 = 1
    0x21, 0x01, 0x2c, // metadata: short index 1 = 300
    0x7f, // terminator
];

/// Spawn Object (0x0E), type 60 Arrow: a positive Data and velocity shorts on
/// all three axes.
const SPAWN_OBJECT: &[u8] = &[
    0x64, // EID 100
    0x3c, // type 60 Arrow
    0x00, 0x00, 0x01, 0x00, // x = 256 / 32 = 8.0
    0x00, 0x00, 0x08, 0x00, // y = 2048 / 32 = 64.0
    0xff, 0xff, 0xff, 0xf0, // z = -16 / 32 = -0.5
    0x40, // pitch: 90.0 degrees
    0xc0, // yaw: 270.0 degrees
    0x00, 0x00, 0x00, 0x0a, // data 10
    0xe0, 0xc0, // velocity x = -8000 / 8000 = -1.0
    0x1f, 0x40, // velocity y = 8000 / 8000 = 1.0
    0x0f, 0xa0, // velocity z = 4000 / 8000 = 0.5
];

/// Spawn Object (0x0E), type 1 Boat: `Data` zero, so no velocity follows.
const SPAWN_OBJECT_ZERO_DATA: &[u8] = &[
    0x64, // EID 100
    0x01, // type 1 Boat
    0x00, 0x00, 0x00, 0x00, // x = 0.0
    0x00, 0x00, 0x00, 0x00, // y = 0.0
    0x00, 0x00, 0x00, 0x00, // z = 0.0
    0x00, // pitch: 0.0 degrees
    0x00, // yaw: 0.0 degrees
    0x00, 0x00, 0x00, 0x00, // data 0
];

/// Spawn Object (0x0E), type 65 ThrownEnderpearl: `Data` is the thrower's EID
/// and nothing else — a negative `Data` still carries no velocity, as the
/// reader only reads speeds when `Data > 0` (`S0EPacketSpawnObject.java:101`).
const SPAWN_OBJECT_NEGATIVE_DATA: &[u8] = &[
    0x64, // EID 100
    0x41, // type 65 ThrownEnderpearl
    0x00, 0x00, 0x00, 0x00, // x = 0.0
    0x00, 0x00, 0x00, 0x00, // y = 0.0
    0x00, 0x00, 0x00, 0x00, // z = 0.0
    0x00, 0x00, // pitch, yaw: 0.0
    0xff, 0xff, 0xff, 0xff, // data -1
];

/// Spawn Mob (0x0F): EID 42, a creeper, three angle bytes with distinct
/// values, velocity on all axes, two metadata entries with the terminator.
const SPAWN_MOB: &[u8] = &[
    0x2a, // EID 42
    0x32, // type 50 Creeper
    0x00, 0x00, 0x01, 0x00, // x = 8.0
    0x00, 0x00, 0x08, 0x00, // y = 64.0
    0xff, 0xff, 0xff, 0xf0, // z = -0.5
    0x00, // yaw: 0.0 degrees
    0x40, // pitch: 90.0 degrees
    0x80, // head yaw: 180.0 degrees
    0xf0, 0x60, // velocity x = -4000 / 8000 = -0.5
    0x1f, 0x40, // velocity y = 1.0
    0x00, 0x64, // velocity z = 100 / 8000 = 0.0125
    0x00, 0x01, // metadata: byte index 0 = 1
    0x21, 0x01, 0x2c, // metadata: short index 1 = 300
    0x7f, // terminator
];

/// Spawn Painting (0x10): a 13-byte title, a packed position, facing 3.
const SPAWN_PAINTING: &[u8] = &[
    0x07, // EID 7
    0x0d, b'S', b'k', b'u', b'l', b'l', b'A', b'n', b'd', b'R', b'o', b's', b'e',
    b's', // title "SkullAndRoses", 13 bytes
    0x00, 0x00, 0x19, 0x01, 0x1b, 0xff, 0xff, 0x38, // (100, 70, -200) packed
    0x03, // facing 3
];

/// Spawn Experience Orb (0x11): whole-block fixed points and the largest
/// count a short can carry.
const SPAWN_XP_ORB: &[u8] = &[
    0x05, // EID 5
    0x00, 0x00, 0x00, 0x20, // x = 32 / 32 = 1.0
    0x00, 0x00, 0x00, 0x40, // y = 64 / 32 = 2.0
    0x00, 0x00, 0x00, 0x60, // z = 96 / 32 = 3.0
    0x7f, 0xff, // count 32767
];

/// Spawn Global Entity (0x2C): type 1 lightning, negative z.
const SPAWN_GLOBAL: &[u8] = &[
    0x09, // EID 9
    0x01, // type 1 lightning
    0x00, 0x00, 0x06, 0x00, // x = 1536 / 32 = 48.0
    0x00, 0x00, 0x01, 0xa0, // y = 416 / 32 = 13.0
    0xff, 0xff, 0xff, 0x00, // z = -256 / 32 = -8.0
];

/// Entity Metadata (0x1C): EID 20, a byte entry and an int entry.
const ENTITY_METADATA: &[u8] = &[
    0x14, // EID 20
    0x00, 0x01, // byte index 0 = 1
    0x42, 0x00, 0x00, 0x00, 0x40, // int index 2 = 64
    0x7f, // terminator
];

#[test]
fn spawn_player_decodes_every_field() {
    assert_eq!(SpawnPlayer::ID, 0x0c);
    let player = decode_spawn_player(SPAWN_PLAYER).expect("a spawning player decodes");
    assert_eq!(player.entity_id, 20);
    assert_eq!(player.uuid, "01020304-0506-0708-090a-0b0c0d0e0f10");
    assert_eq!(player.x, 12.5);
    assert_eq!(player.y, 65.0);
    assert_eq!(player.z, -1.5);
    assert_eq!(player.yaw, 292.5);
    assert_eq!(player.pitch, 90.0);
    assert_eq!(player.current_item, 276);
    assert_eq!(
        player.metadata.entries,
        vec![(0, MetadataValue::Byte(1)), (1, MetadataValue::Short(300))]
    );
}

#[test]
fn spawn_object_decodes_every_field_with_velocity() {
    assert_eq!(SpawnObject::ID, 0x0e);
    let object = decode_spawn_object(SPAWN_OBJECT).expect("a spawning object decodes");
    assert_eq!(object.entity_id, 100);
    assert_eq!(object.kind, ObjectType::Arrow);
    assert_eq!(object.x, 8.0);
    assert_eq!(object.y, 64.0);
    assert_eq!(object.z, -0.5);
    assert_eq!(object.pitch, 90.0);
    assert_eq!(object.yaw, 270.0);
    assert_eq!(object.data, 10);
    assert_eq!(object.velocity, [-1.0, 1.0, 0.5]);
}

#[test]
fn spawn_object_without_velocity_leaves_it_zeroed() {
    let boat = decode_spawn_object(SPAWN_OBJECT_ZERO_DATA).expect("data 0 decodes");
    assert_eq!(boat.kind, ObjectType::Boat);
    assert_eq!(boat.data, 0);
    assert_eq!(boat.velocity, [0.0, 0.0, 0.0]);

    let pearl = decode_spawn_object(SPAWN_OBJECT_NEGATIVE_DATA).expect("negative data decodes");
    assert_eq!(pearl.kind, ObjectType::ThrownEnderpearl);
    assert_eq!(pearl.data, -1);
    assert_eq!(pearl.velocity, [0.0, 0.0, 0.0]);
}

#[test]
fn spawn_mob_decodes_every_field() {
    assert_eq!(SpawnMob::ID, 0x0f);
    let mob = decode_spawn_mob(SPAWN_MOB).expect("a spawning mob decodes");
    assert_eq!(mob.entity_id, 42);
    assert_eq!(mob.kind, MobType::Creeper);
    assert_eq!(mob.x, 8.0);
    assert_eq!(mob.y, 64.0);
    assert_eq!(mob.z, -0.5);
    assert_eq!(mob.yaw, 0.0);
    assert_eq!(mob.pitch, 90.0);
    assert_eq!(mob.head_yaw, 180.0);
    assert_eq!(mob.velocity, [-0.5, 1.0, 0.0125]);
    assert_eq!(
        mob.metadata.entries,
        vec![(0, MetadataValue::Byte(1)), (1, MetadataValue::Short(300))]
    );
}

#[test]
fn spawn_painting_decodes_every_field() {
    assert_eq!(SpawnPainting::ID, 0x10);
    let painting = decode_spawn_painting(SPAWN_PAINTING).expect("a spawning painting decodes");
    assert_eq!(painting.entity_id, 7);
    assert_eq!(painting.title, "SkullAndRoses");
    assert_eq!((painting.x, painting.y, painting.z), (100, 70, -200));
    assert_eq!(painting.facing, 3);
}

#[test]
fn spawn_xp_orb_decodes_every_field() {
    assert_eq!(SpawnXpOrb::ID, 0x11);
    let orb = decode_spawn_xp_orb(SPAWN_XP_ORB).expect("a spawning orb decodes");
    assert_eq!(orb.entity_id, 5);
    assert_eq!(orb.x, 1.0);
    assert_eq!(orb.y, 2.0);
    assert_eq!(orb.z, 3.0);
    assert_eq!(orb.count, 32767);
}

#[test]
fn spawn_global_decodes_every_field() {
    assert_eq!(SpawnGlobal::ID, 0x2c);
    let global = decode_spawn_global(SPAWN_GLOBAL).expect("a spawning global decodes");
    assert_eq!(global.entity_id, 9);
    assert_eq!(global.kind, GlobalType::Lightning);
    assert_eq!(global.x, 48.0);
    assert_eq!(global.y, 13.0);
    assert_eq!(global.z, -8.0);
}

#[test]
fn entity_metadata_decodes_the_pair() {
    assert_eq!(EntityMetadata::ID, 0x1c);
    let update = decode_entity_metadata(ENTITY_METADATA).expect("a metadata packet decodes");
    assert_eq!(update.entity_id, 20);
    assert_eq!(
        update.metadata.entries,
        vec![(0, MetadataValue::Byte(1)), (2, MetadataValue::Int(64))]
    );
}

#[test]
fn spawn_decodes_refuse_a_truncated_body() {
    // One byte short of each fixture, the last field cannot be read.
    assert!(decode_spawn_player(&SPAWN_PLAYER[..SPAWN_PLAYER.len() - 1]).is_err());
    assert!(decode_spawn_object(&SPAWN_OBJECT[..SPAWN_OBJECT.len() - 1]).is_err());
    assert!(decode_spawn_mob(&SPAWN_MOB[..SPAWN_MOB.len() - 1]).is_err());
    assert!(decode_spawn_painting(&SPAWN_PAINTING[..SPAWN_PAINTING.len() - 1]).is_err());
    assert!(decode_spawn_xp_orb(&SPAWN_XP_ORB[..SPAWN_XP_ORB.len() - 1]).is_err());
    assert!(decode_spawn_global(&SPAWN_GLOBAL[..SPAWN_GLOBAL.len() - 1]).is_err());
    assert!(decode_entity_metadata(&ENTITY_METADATA[..ENTITY_METADATA.len() - 1]).is_err());
}

#[test]
fn spawn_decodes_refuse_a_trailing_byte() {
    for (name, result) in [
        (
            "player",
            with_trailing_byte(SPAWN_PLAYER, decode_spawn_player),
        ),
        (
            "object",
            with_trailing_byte(SPAWN_OBJECT, decode_spawn_object),
        ),
        ("mob", with_trailing_byte(SPAWN_MOB, decode_spawn_mob)),
        (
            "painting",
            with_trailing_byte(SPAWN_PAINTING, decode_spawn_painting),
        ),
        ("orb", with_trailing_byte(SPAWN_XP_ORB, decode_spawn_xp_orb)),
        (
            "global",
            with_trailing_byte(SPAWN_GLOBAL, decode_spawn_global),
        ),
        (
            "metadata",
            with_trailing_byte(ENTITY_METADATA, decode_entity_metadata),
        ),
    ] {
        assert!(
            matches!(result, Err(PacketError::Trailing(1))),
            "{name}: expected a trailing refusal, got {result:?}"
        );
    }
}

/// Decodes `body` with one refused byte appended.
fn with_trailing_byte<T>(
    body: &[u8],
    decode: fn(&[u8]) -> Result<T, PacketError>,
) -> Result<(), PacketError> {
    let mut padded = body.to_vec();
    padded.push(0x00);
    decode(&padded).map(drop)
}

#[test]
fn spawn_decodes_refuse_an_id_outside_the_roster() {
    // 48 and 49 are the abstract Mob and Monster markers, not spawnable
    // entities; 69 is a later version's mob.
    let mut body = SPAWN_MOB.to_vec();
    for id in [48u8, 49, 69] {
        body[1] = id;
        let error = decode_spawn_mob(&body).expect_err("an unlisted mob id is refused");
        assert!(
            error
                .to_string()
                .contains(&format!("unknown Spawn Mob type id {id}")),
            "named refusal, saw: {error}"
        );
    }

    // 13 would be a minecart variant in later tables; §6.3's roster lists
    // 10/11/12 and the sub-type rides in Data.
    let mut body = SPAWN_OBJECT.to_vec();
    body[1] = 13;
    let error = decode_spawn_object(&body).expect_err("an unlisted object id is refused");
    assert!(
        error
            .to_string()
            .contains("unknown Spawn Object type id 13"),
        "named refusal, saw: {error}"
    );

    let mut body = SPAWN_GLOBAL.to_vec();
    body[1] = 2;
    let error = decode_spawn_global(&body).expect_err("an unlisted global id is refused");
    assert!(
        error
            .to_string()
            .contains("unknown global entity type id 2"),
        "named refusal, saw: {error}"
    );
}

/// The spawn-mob roster of §6.3: 50..=68, 90..=101 and 120, one entry per id.
const MOBS: &[(u8, MobType, &str)] = &[
    (50, MobType::Creeper, "Creeper"),
    (51, MobType::Skeleton, "Skeleton"),
    (52, MobType::Spider, "Spider"),
    (53, MobType::Giant, "Giant"),
    (54, MobType::Zombie, "Zombie"),
    (55, MobType::Slime, "Slime"),
    (56, MobType::Ghast, "Ghast"),
    (57, MobType::PigZombie, "PigZombie"),
    (58, MobType::Enderman, "Enderman"),
    (59, MobType::CaveSpider, "CaveSpider"),
    (60, MobType::Silverfish, "Silverfish"),
    (61, MobType::Blaze, "Blaze"),
    (62, MobType::LavaSlime, "LavaSlime"),
    (63, MobType::EnderDragon, "EnderDragon"),
    (64, MobType::WitherBoss, "WitherBoss"),
    (65, MobType::Bat, "Bat"),
    (66, MobType::Witch, "Witch"),
    (67, MobType::Endermite, "Endermite"),
    (68, MobType::Guardian, "Guardian"),
    (90, MobType::Pig, "Pig"),
    (91, MobType::Sheep, "Sheep"),
    (92, MobType::Cow, "Cow"),
    (93, MobType::Chicken, "Chicken"),
    (94, MobType::Squid, "Squid"),
    (95, MobType::Wolf, "Wolf"),
    (96, MobType::MushroomCow, "MushroomCow"),
    (97, MobType::SnowMan, "SnowMan"),
    (98, MobType::Ozelot, "Ozelot"),
    (99, MobType::VillagerGolem, "VillagerGolem"),
    (100, MobType::EntityHorse, "EntityHorse"),
    (101, MobType::Rabbit, "Rabbit"),
    (120, MobType::Villager, "Villager"),
];

/// The spawn-object roster of §6.3: 1, 2, 10/11/12, 50, 51, 60..=66,
/// 70..=78 and 90.
const OBJECTS: &[(u8, ObjectType, &str)] = &[
    (1, ObjectType::Boat, "Boat"),
    (2, ObjectType::Item, "Item"),
    (10, ObjectType::Minecart, "Minecart"),
    (11, ObjectType::MinecartStorage, "MinecartStorage"),
    (12, ObjectType::MinecartPowered, "MinecartPowered"),
    (50, ObjectType::PrimedTnt, "PrimedTnt"),
    (51, ObjectType::EnderCrystal, "EnderCrystal"),
    (60, ObjectType::Arrow, "Arrow"),
    (61, ObjectType::Snowball, "Snowball"),
    (62, ObjectType::ThrownEgg, "ThrownEgg"),
    (63, ObjectType::Fireball, "Fireball"),
    (64, ObjectType::SmallFireball, "SmallFireball"),
    (65, ObjectType::ThrownEnderpearl, "ThrownEnderpearl"),
    (66, ObjectType::WitherSkull, "WitherSkull"),
    (70, ObjectType::FallingSand, "FallingSand"),
    (71, ObjectType::ItemFrame, "ItemFrame"),
    (72, ObjectType::EyeOfEnderSignal, "EyeOfEnderSignal"),
    (73, ObjectType::ThrownPotion, "ThrownPotion"),
    (74, ObjectType::FallingDragonEgg, "FallingDragonEgg"),
    (75, ObjectType::ThrownExpBottle, "ThrownExpBottle"),
    (
        76,
        ObjectType::FireworksRocketEntity,
        "FireworksRocketEntity",
    ),
    (77, ObjectType::LeashKnot, "LeashKnot"),
    (78, ObjectType::ArmorStand, "ArmorStand"),
    (90, ObjectType::FishHook, "FishHook"),
];

/// The global-entity table: §6.3 has no further roster, so lightning is the
/// one entry.
const GLOBALS: &[(u8, GlobalType, &str)] = &[(1, GlobalType::Lightning, "Lightning")];

#[test]
fn mob_table_covers_the_roster_and_back() {
    assert_eq!(MOBS.len(), 32, "§6.3's roster is 32 ids");
    for &(id, kind, name) in MOBS {
        assert_eq!(MobType::from_id(id), Some(kind), "id {id} is {name}");
        assert_eq!(kind.id(), id, "{name} maps back to {id}");
        assert_eq!(format!("{kind:?}"), name, "the variant is named {name}");
    }
    for id in [0u8, 1, 47, 48, 49, 69, 89, 102, 119, 121, 255] {
        assert_eq!(MobType::from_id(id), None, "id {id} is not a spawn-mob id");
    }
}

#[test]
fn object_table_covers_the_roster_and_back() {
    assert_eq!(OBJECTS.len(), 24, "§6.3's object roster is 24 ids");
    for &(id, kind, name) in OBJECTS {
        assert_eq!(ObjectType::from_id(id), Some(kind), "id {id} is {name}");
        assert_eq!(kind.id(), id, "{name} maps back to {id}");
        assert_eq!(format!("{kind:?}"), name, "the variant is named {name}");
    }
    for id in [0u8, 3, 9, 13, 49, 52, 59, 69, 79, 89, 91, 255] {
        assert_eq!(
            ObjectType::from_id(id),
            None,
            "id {id} is not a spawn-object id"
        );
    }
}

#[test]
fn global_table_covers_the_roster_and_back() {
    assert_eq!(GLOBALS.len(), 1, "§6.3 lists lightning alone");
    for &(id, kind, name) in GLOBALS {
        assert_eq!(GlobalType::from_id(id), Some(kind), "id {id} is {name}");
        assert_eq!(kind.id(), id, "{name} maps back to {id}");
        assert_eq!(format!("{kind:?}"), name, "the variant is named {name}");
    }
    for id in [0u8, 2, 255] {
        assert_eq!(
            GlobalType::from_id(id),
            None,
            "id {id} is not a global-entity id"
        );
    }
}

#[test]
fn spot_ids_from_the_roster() {
    // Literal pairs straight from §6.3, so a shifted table fails loudly
    // without waiting for the roster loop: creepers 50, zombies 54, pigs 90,
    // horses 100, villagers 120; boats 1, minecarts 10, fishhooks 90,
    // armor stands 78.
    assert_eq!(MobType::from_id(50), Some(MobType::Creeper));
    assert_eq!(MobType::from_id(54), Some(MobType::Zombie));
    assert_eq!(MobType::from_id(67), Some(MobType::Endermite));
    assert_eq!(MobType::from_id(90), Some(MobType::Pig));
    assert_eq!(MobType::from_id(95), Some(MobType::Wolf));
    assert_eq!(MobType::from_id(100), Some(MobType::EntityHorse));
    assert_eq!(MobType::from_id(120), Some(MobType::Villager));
    assert_eq!(MobType::from_id(62), Some(MobType::LavaSlime));

    assert_eq!(ObjectType::from_id(1), Some(ObjectType::Boat));
    assert_eq!(ObjectType::from_id(10), Some(ObjectType::Minecart));
    assert_eq!(ObjectType::from_id(50), Some(ObjectType::PrimedTnt));
    assert_eq!(ObjectType::from_id(60), Some(ObjectType::Arrow));
    assert_eq!(ObjectType::from_id(63), Some(ObjectType::Fireball));
    assert_eq!(ObjectType::from_id(73), Some(ObjectType::ThrownPotion));
    assert_eq!(ObjectType::from_id(78), Some(ObjectType::ArmorStand));
    assert_eq!(ObjectType::from_id(90), Some(ObjectType::FishHook));

    assert_eq!(GlobalType::from_id(1), Some(GlobalType::Lightning));

    // And the id() arrows back, as literals.
    assert_eq!(MobType::Creeper.id(), 50);
    assert_eq!(MobType::Endermite.id(), 67);
    assert_eq!(MobType::Villager.id(), 120);
    assert_eq!(ObjectType::Boat.id(), 1);
    assert_eq!(ObjectType::Minecart.id(), 10);
    assert_eq!(ObjectType::FishHook.id(), 90);
    assert_eq!(GlobalType::Lightning.id(), 1);
}

#[test]
fn metadata_item_carries_the_slot_shape() {
    // §6.1's slot payload as a plain value; the fixture above already proves
    // the decoder hands one over.
    let item = MetadataItem {
        id: 276,
        count: 2,
        damage: 42,
    };
    assert_eq!(item.id, 276);
    assert_eq!(item.count, 2);
    assert_eq!(item.damage, 42);
}
