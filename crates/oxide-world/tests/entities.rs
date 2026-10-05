//! The scripted scenario: a player, a mob and an item spawn, move for forty
//! ticks with one teleport each way through the middle, take a metadata
//! update and a status, and one of them despawns. The end state and the pose
//! pairs are asserted against hand arithmetic.

use oxide_proto_v47::entity::{Metadata, MetadataValue};
use oxide_world::entity::{Entities, Entity, EntityKind, KindData};

/// A metadata block from literal entries.
fn block(entries: Vec<(u8, MetadataValue)>) -> Metadata {
    Metadata { entries }
}

#[test]
fn a_player_a_mob_and_an_item_move_for_forty_ticks_and_one_despawns() {
    let mut entities = Entities::new();

    // Inserted out of id order so the iteration order is its own assertion.
    let mut mob = Entity::new(101, EntityKind::Zombie);
    mob.position = [0.0, 64.0, 0.0];
    mob.last_tick_position = [0.0, 64.0, 0.0];
    mob.velocity = [0.0, 0.0, 0.25];
    entities.insert(mob);

    let mut player = Entity::new(100, EntityKind::Player);
    player.uuid = Some("069a79f4-44e9-4726-a5be-fca90e38aaf5".to_owned());
    entities.insert(player);

    let mut item = Entity::new(102, EntityKind::Item);
    item.data = KindData::Item {
        id: 276,
        count: 1,
        damage: 0,
    };
    entities.insert(item);

    assert_eq!(
        entities.iter().map(|entity| entity.id).collect::<Vec<_>>(),
        vec![100, 101, 102]
    );

    for tick in 1..=40 {
        entities.apply_relative_move(100, [0.125, 0.0, 0.0]);
        entities.apply_relative_move(101, [0.0, 0.0, 0.25]);
        entities.apply_relative_move(102, [0.5, 0.0, 0.0]);

        if tick == 10 {
            entities.apply_look(101, 180.0, 0.0);
        }
        if tick == 20 {
            // The player is teleported across the room and the item far
            // away; both keep moving their axes afterwards.
            entities.apply_teleport(100, [5.0, 64.0, 1.0], 90.0, 10.0, true);
            entities.apply_head_look(100, 90.0);
            entities.apply_teleport(102, [10.5, 64.0, -3.25], 0.0, 0.0, false);
        }
        if tick == 25 {
            entities.apply_metadata(
                101,
                block(vec![
                    (0, MetadataValue::Byte(1)),
                    (16, MetadataValue::Byte(0)),
                ]),
            );
        }
        if tick == 30 {
            entities.apply_metadata(101, block(vec![(16, MetadataValue::Byte(1))]));
        }
        if tick == 36 {
            entities.apply_status(101, 2);
        }

        entities.tick();
    }

    // The player: 19 steps to 2.375, the teleport to 5.0 at tick 20, then
    // twenty more steps to 7.5.
    let player = entities.get(100).expect("the player is live");
    assert_eq!(player.position, [7.5, 64.0, 1.0]);
    assert_eq!(player.last_tick_position, player.position);
    assert_eq!(player.last_tick_yaw, player.yaw);
    assert_eq!(player.last_tick_pitch, player.pitch);
    assert_eq!(player.last_tick_head_yaw, player.head_yaw);
    assert_eq!(player.yaw, 90.0);
    assert_eq!(player.pitch, 10.0);
    assert_eq!(player.head_yaw, 90.0);
    assert!(player.on_ground);
    assert_eq!(player.age, 40);
    assert_eq!(
        player.uuid.as_deref(),
        Some("069a79f4-44e9-4726-a5be-fca90e38aaf5")
    );
    // After the teleport the player walks +x, a movement target of -90,
    // while its body yaw is 90: the ease pulls the render yaw toward -90
    // but the body bound holds it inside the body's reach, so the tick's
    // arithmetic seats it on 90 - 75 + 75 x 0.2 = 30 and leaves it there.
    assert!(
        (player.render_yaw_offset - 30.0).abs() < 0.01,
        "the render yaw sits in the body's reach, saw {}",
        player.render_yaw_offset
    );

    // The mob: a straight ten-block walk along z, a look at tick 10, the
    // merged metadata and five ticks of hurt left.
    let mob = entities.get(101).expect("the mob is live");
    assert_eq!(mob.position, [0.0, 64.0, 10.0]);
    assert_eq!(mob.last_tick_position, mob.position);
    assert_eq!(mob.yaw, 180.0);
    assert_eq!(mob.last_tick_yaw, 180.0);
    // The moving mob's head bounds to its body: it faced 0 while walking
    // +z, then the look at tick 10 turned the body to 180 and the head was
    // pulled to 75 short of it the short way, 180 + 75 = 255.
    assert_eq!(mob.head_yaw, 255.0);
    assert_eq!(
        mob.velocity,
        [0.0, 0.0, 0.25],
        "the store does not integrate"
    );
    assert_eq!(mob.age, 40);
    assert_eq!(mob.death_ticks, 0);
    assert_eq!(
        mob.hurt_ticks, 5,
        "ten minus the five ticks since the status"
    );
    assert_eq!(
        mob.metadata.entries,
        vec![(0, MetadataValue::Byte(1)), (16, MetadataValue::Byte(1)),],
        "index 0 stayed and index 16 was replaced"
    );

    // The item: nineteen half-block steps to 9.5, the teleport to 10.5 at
    // tick 20, then twenty more steps to 20.5.
    let item = entities.get(102).expect("the item is live");
    assert_eq!(item.position, [20.5, 64.0, -3.25]);
    assert_eq!(item.last_tick_position, item.position);
    assert!(!item.on_ground);
    assert_eq!(item.age, 40);
    assert_eq!(
        item.data,
        KindData::Item {
            id: 276,
            count: 1,
            damage: 0
        }
    );

    // One despawn.
    assert_eq!(entities.remove(&[102]), 1);
    assert_eq!(entities.len(), 2);
    assert!(entities.get(102).is_none());
    assert_eq!(
        entities.iter().map(|entity| entity.id).collect::<Vec<_>>(),
        vec![100, 101]
    );
}
