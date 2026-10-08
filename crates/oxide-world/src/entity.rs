//! The entity table: the closed kind enum, the per-entity state the session
//! mutates from spawn and movement packets, and the once-per-tick update the
//! renderer's interpolation pairs read.
//!
//! The kinds are closed: the session maps wire spawn types to [`EntityKind`]
//! at its own edge, so this store never sees a wire type. The tick arithmetic
//! is the source's own, taken from `EntityLivingBase.onEntityUpdate`/
//! `updateArmSwingProgress`/`handleStatusUpdate`/`onDeathUpdate`, the
//! `updateDistance` pair — the base chase and the `EntityLiving` override
//! that hands mobs to `EntityBodyHelper` — the `rangeChecks` folds and
//! `EntityOtherPlayerMP.onUpdate`
//! (`refs/_src/MCP-919/src/minecraft/net/minecraft/`), and every step of it
//! is asserted against hand arithmetic in the tests below.
//!
//! The store carries raw state: metadata arrives as the decoded block and
//! stays a wire-order list ([`Metadata`]) that later layers extract from; this
//! table does not grow an accessor per mob field.

use std::collections::BTreeMap;
use std::sync::Arc;

use oxide_proto_v47::entity::{Metadata, MetadataItem};

/// What an entity is, as the session's wire-type mapping resolved it.
///
/// The roster mirrors the spawn codecs' own type tables: the object variants
/// come from [`oxide_proto_v47::entity::ObjectType`]'s client-relevant set,
/// the mob variants are one per [`oxide_proto_v47::entity::MobType`] member
/// (names kept verbatim), and `Global` covers the global entity table
/// (lightning and friends), which is tracked but not drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    /// Another player.
    Player,
    /// A dropped item.
    Item,
    /// An experience orb.
    XpOrb,
    /// An arrow in flight.
    Arrow,
    /// A thrown snowball.
    Snowball,
    /// A thrown egg.
    Egg,
    /// A thrown ender pearl.
    EnderPearl,
    /// An eye of ender in flight.
    EyeOfEnder,
    /// A thrown potion.
    Potion,
    /// A thrown bottle o' enchanting.
    XpBottle,
    /// A flying firework rocket.
    Firework,
    /// A ghast's fireball.
    Fireball,
    /// A blaze's small fireball.
    SmallFireball,
    /// A wither skull in flight.
    WitherSkull,
    /// A hanging painting.
    Painting,
    /// An item frame.
    ItemFrame,
    /// A boat.
    Boat,
    /// A minecart; the wire's cargo sub-types are distinct spawn ids that a
    /// session may split into kinds of their own without touching this table.
    Minecart,
    /// A tracked global entity — lightning and friends. Carried in the table
    /// but not drawn.
    Global,
    /// A creeper.
    Creeper,
    /// A skeleton.
    Skeleton,
    /// A spider.
    Spider,
    /// A giant.
    Giant,
    /// A zombie.
    Zombie,
    /// A slime.
    Slime,
    /// A ghast.
    Ghast,
    /// A zombie pigman.
    PigZombie,
    /// An enderman.
    Enderman,
    /// A cave spider.
    CaveSpider,
    /// A silverfish.
    Silverfish,
    /// A blaze.
    Blaze,
    /// A magma cube.
    LavaSlime,
    /// An ender dragon.
    EnderDragon,
    /// A wither.
    WitherBoss,
    /// A bat.
    Bat,
    /// A witch.
    Witch,
    /// An endermite.
    Endermite,
    /// A guardian.
    Guardian,
    /// A pig.
    Pig,
    /// A sheep.
    Sheep,
    /// A cow.
    Cow,
    /// A chicken.
    Chicken,
    /// A squid.
    Squid,
    /// A wolf.
    Wolf,
    /// A mooshroom.
    MushroomCow,
    /// A snow golem.
    SnowMan,
    /// An ocelot.
    Ozelot,
    /// An iron golem.
    VillagerGolem,
    /// A horse.
    EntityHorse,
    /// A rabbit.
    Rabbit,
    /// A villager.
    Villager,
    /// An entity whose wire type the session did not map to a kind.
    Unknown,
}

/// The spawn-given extras an entity's kind carries.
///
/// Everything else a renderer needs that arrives in metadata stays in the
/// entity's [`Metadata`] and is extracted by the session; this table does not
/// grow a payload per mob field.
#[derive(Debug, Clone, PartialEq)]
pub enum KindData {
    /// The spawn carried no kind-specific extras.
    None,
    /// A dropped item's stack.
    Item {
        /// The item's id.
        id: i16,
        /// The stack's count.
        count: u8,
        /// The stack's damage or metadata value.
        damage: i16,
    },
    /// A painting's art and hanging.
    Painting {
        /// The art's title (a name from the art table).
        title: Arc<str>,
        /// The source's direction byte (`0` −Z, `1` −X, `2` +Z, `3` +X).
        facing: u8,
    },
    /// An experience orb's value.
    XpOrb {
        /// The experience the orb carries.
        count: i16,
    },
    /// A minecart (all of the wire's cart sub-types today).
    Minecart,
    /// A boat.
    Boat,
}

/// One entity's attachment: what it rides or is leashed to
/// (`S1BPacketEntityAttach`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attachment {
    /// The holder: the vehicle or the leash's anchor.
    pub holder: i32,
    /// Whether the attach was the leash form rather than a mount.
    pub leash: bool,
}

/// One entity's state: the pose pairs the renderer interpolates, the raw
/// metadata block, the equipment, the animation counters and the spawn's
/// kind data.
///
/// Every pair's partner holds the value from before the latest [`Entities::tick`]:
/// a live entity's fields move as packets land, and the `last_tick_*` /
/// `prev_*` / `last_*` partners are refreshed once per tick so the renderer
/// can lerp between the last tick's pose and the current one.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    /// The entity id the server uses.
    pub id: i32,
    /// What the entity is.
    pub kind: EntityKind,
    /// The profile UUID; players only.
    pub uuid: Option<String>,
    /// The position in blocks.
    pub position: [f64; 3],
    /// The position at the last tick.
    pub last_tick_position: [f64; 3],
    /// The body yaw in degrees.
    pub yaw: f32,
    /// The body yaw at the last tick.
    pub last_tick_yaw: f32,
    /// The pitch in degrees.
    pub pitch: f32,
    /// The pitch at the last tick.
    pub last_tick_pitch: f32,
    /// The head's yaw in degrees.
    pub head_yaw: f32,
    /// The head's yaw at the last tick.
    pub last_tick_head_yaw: f32,
    /// The body's render yaw in degrees; chased every tick — players toward
    /// the movement direction (or the body yaw while swinging, else held),
    /// mobs toward the head through the body helper.
    pub render_yaw_offset: f32,
    /// The render yaw from before the last tick's chase.
    pub prev_render_yaw_offset: f32,
    /// The velocity in blocks per tick.
    pub velocity: [f64; 3],
    /// Whether the wire last reported the entity on the ground.
    pub on_ground: bool,
    /// The raw metadata block, in wire order.
    pub metadata: Metadata,
    /// The equipment slots: `0` held, `1` boots, `2` leggings, `3` chestplate,
    /// `4` helmet (`S04PacketEntityEquipment`).
    pub equipment: [Option<MetadataItem>; 5],
    /// The entity's attachment, when it rides or is leashed.
    pub attachment: Option<Attachment>,
    /// The entity's age in ticks (the source's `ticksExisted`).
    pub age: u32,
    /// The limb swing's accumulated distance.
    pub limb_swing: f32,
    /// The limb swing's eased amount.
    pub limb_swing_amount: f32,
    /// The limb swing amount at the last tick.
    pub last_limb_swing_amount: f32,
    /// The arm swing's progress within its cycle, `0.0..1.0`.
    pub swing_progress: f32,
    /// The swing progress at the last tick.
    pub last_swing_progress: f32,
    /// The hurt window's remaining ticks (`EntityLivingBase.hurtTime`).
    pub hurt_ticks: u16,
    /// The death animation's ticks since death started
    /// (`EntityLivingBase.deathTime`): zero until status 3 starts it, then
    /// counting up once per tick from the following tick.
    pub death_ticks: u16,
    /// The spawn's kind-specific extras.
    pub data: KindData,
    /// The swing state machine's tick counter
    /// (`EntityLivingBase.swingProgressInt`).
    swing_progress_ticks: i32,
    /// Whether a swing is in progress
    /// (`EntityLivingBase.isSwingInProgress`).
    swing_in_progress: bool,
    /// The body helper's held-head tick counter
    /// (`EntityBodyHelper.rotationTickCounter`).
    rotation_tick_counter: i32,
    /// The body helper's own previous head reading
    /// (`EntityBodyHelper.prevRenderYawHead`) — distinct from the
    /// [`Entity::last_tick_head_yaw`] pose pair the renderer reads.
    prev_render_yaw_head: f32,
    /// Whether status 3 has started the death counter; while it has not,
    /// `death_ticks` stays at zero.
    death_started: bool,
}

/// The entity table: every live entity by id, iterated in ascending id order
/// so tests and the per-tick feed are deterministic.
#[derive(Debug, Default)]
pub struct Entities {
    /// The entities, keyed by id.
    map: BTreeMap<i32, Entity>,
}

/// The arm swing's period in ticks: the base six the source's
/// `getArmSwingAnimationEnd` returns (`EntityLivingBase.java:1334-1337`; the
/// dig-speed potions it adjusts for are not tracked here).
const ARM_SWING_PERIOD_TICKS: i32 = 6;

/// The hurt window status 2 starts: the source sets `hurtTime` to
/// `maxHurtTime` there, ten ticks (`EntityLivingBase.java:1358-1362`).
const HURT_WINDOW_TICKS: u16 = 10;

/// The base chase's ease of the wrapped difference into the render yaw
/// (`EntityLivingBase.java:1915`).
const CHASE_EASE: f32 = 0.3;

/// The base chase's movement gate on the squared horizontal displacement
/// (`EntityLivingBase.java:1845`): below it the chase holds instead of
/// steering by the tick's movement.
const MOVEMENT_GATE_SQUARED: f32 = 0.0025000002;

/// The body helper's own, smaller movement gate
/// (`EntityBodyHelper.java:29`).
const MOB_MOVEMENT_GATE_SQUARED: f64 = 2.500000277905201e-7;

/// The body reach in degrees: how far the render yaw may sit from the body
/// yaw before the bound pulls it back (`EntityLivingBase.java:1919-1927`,
/// `EntityBodyHelper.java:73-76`).
const BODY_REACH_DEGREES: f32 = 75.0;

/// The squared reach past which the base chase's release fires
/// (`EntityLivingBase.java:1931`).
const RELEASE_THRESHOLD_SQUARED: f32 = 2500.0;

/// The release's ease (`EntityLivingBase.java:1933`).
const RELEASE_EASE: f32 = 0.2;

/// How far the held head may drift before the body helper resets its
/// held-tick counter (`EntityBodyHelper.java:40`).
const HELD_HEAD_RESET_DEGREES: f32 = 15.0;

/// The held ticks before the body helper's reach starts to decay, a tenth
/// of the reach per further tick (`EntityBodyHelper.java:48-52`).
const HELD_DECAY_START_TICKS: i32 = 10;

/// The source's `MathHelper.wrapAngleTo180_float`
/// (`util/MathHelper.java:212-225`): an angle folded into `[-180, 180)`.
fn wrap_angle_to_180(value: f32) -> f32 {
    let mut value = value % 360.0;
    if value >= 180.0 {
        value -= 360.0;
    }
    if value < -180.0 {
        value += 360.0;
    }
    value
}

/// The body helper's own bound (`EntityBodyHelper.java:60-79`): the new
/// `angle2` such that its wrapped difference from `angle1` stays within
/// `±reach`. The two comparisons are the source's — a difference of exactly
/// `-reach` passes, one of exactly `+reach` clamps (to itself).
fn compute_angle_with_bound(angle1: f32, angle2: f32, reach: f32) -> f32 {
    let mut difference = wrap_angle_to_180(angle1 - angle2);
    if difference < -reach {
        difference = -reach;
    }
    if difference >= reach {
        difference = reach;
    }
    angle1 - difference
}

/// Folds one pose pair's partner within `±180` of its current value — the
/// source's `rangeChecks` (`EntityLivingBase.java:1867-1906`).
///
/// The two while-loops are the source's own in order and condition. The
/// `next == partner` checks only fire where an f32 step can no longer change
/// the partner at extreme magnitudes — there the source's loops would never
/// terminate, and hostile values must stay total here.
fn fold_pose_pair(current: f32, partner: &mut f32) {
    while current - *partner < -180.0 {
        let next = *partner - 360.0;
        if next == *partner {
            break;
        }
        *partner = next;
    }
    while current - *partner >= 180.0 {
        let next = *partner + 360.0;
        if next == *partner {
            break;
        }
        *partner = next;
    }
}

impl EntityKind {
    /// Whether this kind is one of the mobs: the `EntityLiving` subclasses
    /// whose `updateDistance` hands the body to the per-entity
    /// `EntityBodyHelper` (`EntityLiving.java:288-292`) — every variant that
    /// mirrors a `MobType` member.
    fn is_mob(self) -> bool {
        matches!(
            self,
            EntityKind::Creeper
                | EntityKind::Skeleton
                | EntityKind::Spider
                | EntityKind::Giant
                | EntityKind::Zombie
                | EntityKind::Slime
                | EntityKind::Ghast
                | EntityKind::PigZombie
                | EntityKind::Enderman
                | EntityKind::CaveSpider
                | EntityKind::Silverfish
                | EntityKind::Blaze
                | EntityKind::LavaSlime
                | EntityKind::EnderDragon
                | EntityKind::WitherBoss
                | EntityKind::Bat
                | EntityKind::Witch
                | EntityKind::Endermite
                | EntityKind::Guardian
                | EntityKind::Pig
                | EntityKind::Sheep
                | EntityKind::Cow
                | EntityKind::Chicken
                | EntityKind::Squid
                | EntityKind::Wolf
                | EntityKind::MushroomCow
                | EntityKind::SnowMan
                | EntityKind::Ozelot
                | EntityKind::VillagerGolem
                | EntityKind::EntityHorse
                | EntityKind::Rabbit
                | EntityKind::Villager
        )
    }

    /// Whether this kind is a living entity (`EntityLivingBase`): players
    /// and mobs — the kinds whose tick folds the copied pose pairs
    /// (`EntityLivingBase.java:1867-1906`).
    fn is_living(self) -> bool {
        self == EntityKind::Player || self.is_mob()
    }
}

impl Entity {
    /// Creates the entity a spawn names, with every pose, counter and extra
    /// blank; the session applies the spawn's values on top.
    pub fn new(id: i32, kind: EntityKind) -> Self {
        Self {
            id,
            kind,
            uuid: None,
            position: [0.0; 3],
            last_tick_position: [0.0; 3],
            yaw: 0.0,
            last_tick_yaw: 0.0,
            pitch: 0.0,
            last_tick_pitch: 0.0,
            head_yaw: 0.0,
            last_tick_head_yaw: 0.0,
            render_yaw_offset: 0.0,
            prev_render_yaw_offset: 0.0,
            velocity: [0.0; 3],
            on_ground: false,
            metadata: Metadata {
                entries: Vec::new(),
            },
            equipment: [const { None }; 5],
            attachment: None,
            age: 0,
            limb_swing: 0.0,
            limb_swing_amount: 0.0,
            last_limb_swing_amount: 0.0,
            swing_progress: 0.0,
            last_swing_progress: 0.0,
            hurt_ticks: 0,
            death_ticks: 0,
            data: KindData::None,
            swing_progress_ticks: 0,
            swing_in_progress: false,
            rotation_tick_counter: 0,
            prev_render_yaw_head: 0.0,
            death_started: false,
        }
    }

    /// Starts the swing the source's `swingItem` starts
    /// (`EntityLivingBase.java:1342-1354`): the counter restarts at `-1`
    /// unless a swing is already running before its halfway point.
    pub fn swing(&mut self) {
        if !self.swing_in_progress
            || self.swing_progress_ticks >= ARM_SWING_PERIOD_TICKS / 2
            || self.swing_progress_ticks < 0
        {
            self.swing_progress_ticks = -1;
            self.swing_in_progress = true;
        }
    }

    /// The base chase, for players — the call site at
    /// `EntityLivingBase.java:1836-1864` running `updateDistance:1912-1942`.
    ///
    /// The target is this tick's movement direction, `atan2(dz, dx)` in
    /// degrees minus a quarter turn, and it only counts past the squared
    /// displacement `MOVEMENT_GATE_SQUARED`; a running swing overrides it
    /// with the body yaw, and otherwise the current render yaw holds. The
    /// body then eases `CHASE_EASE` of the wrapped way toward the target,
    /// sits within `BODY_REACH_DEGREES` of the body yaw, and past a squared
    /// reach of `RELEASE_THRESHOLD_SQUARED` the release eases `RELEASE_EASE`
    /// of the clamped difference back out.
    ///
    /// (The source's negated return value feeds its `movedDistance`, which
    /// this table does not carry.)
    fn chase_body_base(&mut self, d0: f64, d1: f64) {
        let mut target = self.render_yaw_offset;
        let displacement = (d0 * d0 + d1 * d1) as f32;
        if displacement > MOVEMENT_GATE_SQUARED {
            // `(float)MathHelper.atan2(d1, d0) * 180.0F / (float)Math.PI
            // - 90.0F`, left to right. The source's own `atan2` is a
            // lookup-table approximation this port does not copy; the
            // platform's `atan2` stands in at the same place.
            target = d1.atan2(d0) as f32 * 180.0 / std::f32::consts::PI - 90.0;
        }
        if self.swing_progress > 0.0 {
            target = self.yaw;
        }

        let step = wrap_angle_to_180(target - self.render_yaw_offset);
        self.render_yaw_offset += step * CHASE_EASE;
        let mut difference = wrap_angle_to_180(self.yaw - self.render_yaw_offset);
        // The source's `< -75` / `>= 75` pair; `clamp` agrees with it for
        // every value, NaN included.
        difference = difference.clamp(-BODY_REACH_DEGREES, BODY_REACH_DEGREES);
        self.render_yaw_offset = self.yaw - difference;
        if difference * difference > RELEASE_THRESHOLD_SQUARED {
            self.render_yaw_offset += difference * RELEASE_EASE;
        }
    }

    /// The mob chase — `EntityLiving.updateDistance:288-292` handing the
    /// body to `EntityBodyHelper.updateRenderAngles:24-58`.
    ///
    /// While the mob has moved this tick past the helper's own squared gate,
    /// the body snaps to the body yaw and the head bounds to
    /// `BODY_REACH_DEGREES` of it. While it has not, the body bounds toward
    /// the head at the full reach until the head has been held within
    /// `HELD_HEAD_RESET_DEGREES` for `HELD_DECAY_START_TICKS` ticks, then
    /// the reach decays a tenth per further held tick; a head that moves
    /// past the reset threshold restarts the count.
    fn chase_body_helper(&mut self, d0: f64, d1: f64) {
        if d0 * d0 + d1 * d1 > MOB_MOVEMENT_GATE_SQUARED {
            self.render_yaw_offset = self.yaw;
            self.head_yaw =
                compute_angle_with_bound(self.render_yaw_offset, self.head_yaw, BODY_REACH_DEGREES);
            self.prev_render_yaw_head = self.head_yaw;
            self.rotation_tick_counter = 0;
        } else {
            let mut reach = BODY_REACH_DEGREES;
            if (self.head_yaw - self.prev_render_yaw_head).abs() > HELD_HEAD_RESET_DEGREES {
                self.rotation_tick_counter = 0;
                self.prev_render_yaw_head = self.head_yaw;
            } else {
                self.rotation_tick_counter = self.rotation_tick_counter.saturating_add(1);
                if self.rotation_tick_counter > HELD_DECAY_START_TICKS {
                    let held = (self.rotation_tick_counter - HELD_DECAY_START_TICKS) as f32 / 10.0;
                    reach = f32::max(1.0 - held, 0.0) * BODY_REACH_DEGREES;
                }
            }
            self.render_yaw_offset =
                compute_angle_with_bound(self.head_yaw, self.render_yaw_offset, reach);
        }
    }
}

impl Entities {
    /// An empty table.
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    /// Removes every entity.
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// How many entities are live.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the table holds no entities.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The entity with this id, if it is live.
    pub fn get(&self, id: i32) -> Option<&Entity> {
        self.map.get(&id)
    }

    /// The entity with this id, if it is live, for direct mutation.
    pub fn get_mut(&mut self, id: i32) -> Option<&mut Entity> {
        self.map.get_mut(&id)
    }

    /// Every entity in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &Entity> + '_ {
        self.map.values()
    }

    /// Adds the entity, replacing any live entity with the same id — a spawn
    /// for an id that is already occupied is the newer state.
    pub fn insert(&mut self, entity: Entity) {
        self.map.insert(entity.id, entity);
    }

    /// Removes every named entity that is live; returns how many were
    /// removed.
    pub fn remove(&mut self, ids: &[i32]) -> usize {
        let mut removed = 0;
        for id in ids {
            if self.map.remove(id).is_some() {
                removed += 1;
            }
        }
        removed
    }

    /// Adds the delta to the entity's position.
    ///
    /// The delta is in blocks, already converted from the wire's 1/32 units —
    /// the session applies the same single division the source's movement
    /// handler does (`NetHandlerPlayClient.handleEntityMovement:620-625`).
    pub fn apply_relative_move(&mut self, id: i32, delta: [f64; 3]) {
        if let Some(entity) = self.map.get_mut(&id) {
            for (axis, step) in entity.position.iter_mut().zip(delta) {
                *axis += step;
            }
        }
    }

    /// Sets the entity's body yaw and pitch
    /// (`NetHandlerPlayClient.handleEntityMovement:626-627`).
    pub fn apply_look(&mut self, id: i32, yaw: f32, pitch: f32) {
        if let Some(entity) = self.map.get_mut(&id) {
            entity.yaw = yaw;
            entity.pitch = pitch;
        }
    }

    /// Sets the entity's head yaw
    /// (`NetHandlerPlayClient.handleEntityHeadLook`).
    pub fn apply_head_look(&mut self, id: i32, head_yaw: f32) {
        if let Some(entity) = self.map.get_mut(&id) {
            entity.head_yaw = head_yaw;
        }
    }

    /// Overwrites the entity's position, body yaw, pitch and ground flag
    /// absolutely (`NetHandlerPlayClient.handleEntityTeleport:576-580`).
    ///
    /// The pose pairs are untouched: they copy at the next tick, like every
    /// pose, so the renderer sees the teleport as one interpolated step.
    pub fn apply_teleport(
        &mut self,
        id: i32,
        position: [f64; 3],
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    ) {
        if let Some(entity) = self.map.get_mut(&id) {
            entity.position = position;
            entity.yaw = yaw;
            entity.pitch = pitch;
            entity.on_ground = on_ground;
        }
    }

    /// Sets the entity's velocity in blocks per tick
    /// (`NetHandlerPlayClient.handleEntityVelocity:501-510`).
    pub fn apply_velocity(&mut self, id: i32, velocity: [f64; 3]) {
        if let Some(entity) = self.map.get_mut(&id) {
            entity.velocity = velocity;
        }
    }

    /// Merges a metadata block by index: every entry the update carries
    /// replaces the stored entries with that index, and every other stored
    /// entry stays, in its own order.
    pub fn apply_metadata(&mut self, id: i32, metadata: Metadata) {
        if let Some(entity) = self.map.get_mut(&id) {
            entity
                .metadata
                .entries
                .retain(|(index, _)| !metadata.entries.iter().any(|(updated, _)| updated == index));
            entity.metadata.entries.extend(metadata.entries);
        }
    }

    /// Applies one entity status byte (`EntityLivingBase.handleStatusUpdate`).
    ///
    /// The source's map, for the states this store carries:
    ///
    /// - `2` (hurt): the limb-swing amount jumps to `1.5` and `hurt_ticks`
    ///   takes the ten-tick hurt window;
    /// - `3` (dead): the death counter starts — seeded at zero like the
    ///   source's `deathTime` (this status only zeroes the health,
    ///   `EntityLivingBase.java:1382`), counting up from the following
    ///   tick.
    ///
    /// Every other status changes nothing here. The statuses with render
    /// meaning outside this table — `6`/`7` (taming), `9` (eat accepted),
    /// `10` (grass) and `14` (zombie villager) — are noted for the layers
    /// that will carry them; the store has no logging convention of its own,
    /// so an unmapped status is a quiet no-op.
    pub fn apply_status(&mut self, id: i32, status: i8) {
        let Some(entity) = self.map.get_mut(&id) else {
            return;
        };
        match status {
            2 => {
                entity.limb_swing_amount = 1.5;
                entity.hurt_ticks = HURT_WINDOW_TICKS;
            }
            3 => {
                entity.death_started = true;
            }
            _ => {}
        }
    }

    /// Sets one equipment slot.
    ///
    /// The slots are the source's `0` held, `1` boots, `2` leggings,
    /// `3` chestplate and `4` helmet (`S04PacketEntityEquipment`). The wire
    /// decoder refuses anything outside that range; a slot that lands here
    /// anyway is ignored rather than panicked on.
    pub fn set_equipment(&mut self, id: i32, slot: i16, item: Option<MetadataItem>) {
        if let Some(entity) = self.map.get_mut(&id) {
            if let Ok(slot) = usize::try_from(slot) {
                if let Some(slot) = entity.equipment.get_mut(slot) {
                    *slot = item;
                }
            }
        }
    }

    /// Records an attach (`NetHandlerPlayClient.handleEntityAttach:966-1016`).
    ///
    /// A negative holder is the detach the source treats as "no entity there"
    /// (the wire's own sentinel is `-1`); anything else stores the holder and
    /// the leash flag.
    pub fn set_attachment(&mut self, id: i32, holder: i32, leash: bool) {
        if let Some(entity) = self.map.get_mut(&id) {
            entity.attachment = if holder < 0 {
                None
            } else {
                Some(Attachment { holder, leash })
            };
        }
    }

    /// Advances every entity by one session tick, in ascending id order.
    ///
    /// The order and values are the source's own, mapped onto the store:
    ///
    /// 1. the swing pair's partner takes the pre-tick reading
    ///    (`prevSwingProgress = swingProgress` at the top of
    ///    `EntityLivingBase.onEntityUpdate:266`);
    /// 2. the limb pair (`EntityOtherPlayerMP.onUpdate:56-67`): the partner
    ///    takes the pre-update amount, the update measures the horizontal
    ///    distance travelled since the previous tick — `sqrt(dx² + dz²) × 4`,
    ///    clamped to `1.0` — eases it into `limb_swing_amount` by `0.4`, and
    ///    adds the amount onto `limb_swing`;
    /// 3. the swing state machine (`updateArmSwingProgress:1402-1422`): an
    ///    active swing advances its counter and resets from the six-tick
    ///    period, and `swing_progress` is the counter over that period;
    /// 4. the counters: `age` up by one (the world's `++ticksExisted`,
    ///    `World.updateEntityWithOptionalForce:1871`), `hurt_ticks` down
    ///    (`onEntityUpdate:337-340`), and `death_ticks` up once status 3 has
    ///    started it (`onDeathUpdate:398-402`);
    /// 5. the pose pairs copy for the tick — `last_tick_position`,
    ///    `last_tick_yaw`, `last_tick_pitch` and `last_tick_head_yaw` take
    ///    the current values and `prev_render_yaw_offset` the pre-chase
    ///    render yaw — so every partner holds the state from before this
    ///    tick's chase, where the source copies them
    ///    (`EntityLivingBase.onEntityUpdate:379-383`);
    /// 6. the chase, per kind. Players run the base path: the target is the
    ///    tick's movement direction (gated on the squared displacement past
    ///    `0.0025000002`) or the body yaw while a swing runs, else the
    ///    render yaw holds; the body then eases `0.3` of the wrapped way
    ///    toward it, sits within `±75` of the body yaw and takes the
    ///    past-`50` release (`updateDistance:1912-1942`). Mobs hand the body
    ///    to the body helper (`EntityLiving.java:288-292`): moving, the body
    ///    snaps to the body yaw and the head bounds to `±75` of it; held,
    ///    the body bounds toward the head under the helper's decaying reach.
    ///    Every other kind keeps its render yaw;
    /// 7. the folds (`rangeChecks:1867-1906`): for the living kinds, every
    ///    copied partner is folded within `±180` of its current value, so a
    ///    pair that crossed the angle seam reads the short way.
    pub fn tick(&mut self) {
        for entity in self.map.values_mut() {
            // The swing pair's partner, from before this tick's update.
            entity.last_swing_progress = entity.swing_progress;

            // The horizontal movement since the previous tick (the source's
            // posX - prevPosX): the basis the limb pair and both chase paths
            // measure movement with. It is read before the pair copies
            // below overwrite it.
            let d0 = entity.position[0] - entity.last_tick_position[0];
            let d1 = entity.position[2] - entity.last_tick_position[2];

            // The limb pair: the distance walked since the previous tick,
            // scaled and clamped.
            let mut target = (d0 * d0 + d1 * d1).sqrt() as f32 * 4.0;
            if target > 1.0 {
                target = 1.0;
            }
            entity.last_limb_swing_amount = entity.limb_swing_amount;
            entity.limb_swing_amount += (target - entity.limb_swing_amount) * 0.4;
            entity.limb_swing += entity.limb_swing_amount;

            // The swing state machine.
            if entity.swing_in_progress {
                entity.swing_progress_ticks += 1;
                if entity.swing_progress_ticks >= ARM_SWING_PERIOD_TICKS {
                    entity.swing_progress_ticks = 0;
                    entity.swing_in_progress = false;
                }
            } else {
                entity.swing_progress_ticks = 0;
            }
            entity.swing_progress =
                entity.swing_progress_ticks as f32 / ARM_SWING_PERIOD_TICKS as f32;

            // The counters.
            entity.age = entity.age.saturating_add(1);
            entity.hurt_ticks = entity.hurt_ticks.saturating_sub(1);
            if entity.death_started {
                entity.death_ticks = entity.death_ticks.saturating_add(1);
            }

            // The pose pairs copy for the tick, before the chase changes the
            // render yaw (and, for a moving mob, the head): the partners
            // hold the state from before the chase, as the source's copies
            // do.
            entity.last_tick_position = entity.position;
            entity.last_tick_yaw = entity.yaw;
            entity.last_tick_pitch = entity.pitch;
            entity.last_tick_head_yaw = entity.head_yaw;
            entity.prev_render_yaw_offset = entity.render_yaw_offset;

            // The chase, per kind.
            match entity.kind {
                EntityKind::Player => entity.chase_body_base(d0, d1),
                kind if kind.is_mob() => entity.chase_body_helper(d0, d1),
                _ => {}
            }

            // The folds, for the living kinds.
            if entity.kind.is_living() {
                fold_pose_pair(entity.yaw, &mut entity.last_tick_yaw);
                fold_pose_pair(entity.render_yaw_offset, &mut entity.prev_render_yaw_offset);
                fold_pose_pair(entity.pitch, &mut entity.last_tick_pitch);
                fold_pose_pair(entity.head_yaw, &mut entity.last_tick_head_yaw);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Store-operation tests: insert/get/iteration order, upsert, removal
    //! counts, the movement operations' exact arithmetic, metadata merging,
    //! the status map, equipment and attachment. Every expected value is hand
    //! arithmetic on a hand-picked literal, never rebuilt with the code under
    //! test.

    use super::{Attachment, Entities, Entity, EntityKind, KindData};
    use std::sync::Arc;

    use oxide_proto_v47::entity::{Metadata, MetadataItem, MetadataValue};

    /// A fresh entity with no state beyond its identity.
    fn spawned(id: i32, kind: EntityKind) -> Entity {
        Entity::new(id, kind)
    }

    /// A metadata block from literal entries.
    fn block(entries: Vec<(u8, MetadataValue)>) -> Metadata {
        Metadata { entries }
    }

    /// One equipment item from a literal id.
    fn item(id: i16) -> MetadataItem {
        MetadataItem {
            id,
            count: 1,
            damage: 0,
            nbt: None,
        }
    }

    #[test]
    fn insert_get_and_iteration_stay_ordered_by_id() {
        let mut entities = Entities::new();
        assert!(entities.is_empty());
        entities.insert(spawned(3, EntityKind::Cow));
        entities.insert(spawned(1, EntityKind::Player));
        entities.insert(spawned(2, EntityKind::Creeper));

        assert_eq!(entities.len(), 3);
        assert!(!entities.is_empty());
        assert_eq!(
            entities.iter().map(|entity| entity.id).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "iteration ascends by id regardless of insert order"
        );
        assert_eq!(
            entities.get(2).map(|entity| entity.kind),
            Some(EntityKind::Creeper)
        );
        assert!(entities.get(9).is_none());

        entities.get_mut(2).expect("id 2 is live").yaw = 90.0;
        assert_eq!(entities.get(2).map(|entity| entity.yaw), Some(90.0));
    }

    #[test]
    fn a_second_spawn_for_a_live_id_replaces_the_entity() {
        let mut entities = Entities::new();
        let mut first = spawned(5, EntityKind::Creeper);
        first.position = [1.0, 64.0, 1.0];
        entities.insert(first);

        let mut second = spawned(5, EntityKind::Zombie);
        second.position = [2.0, 65.0, 2.0];
        entities.insert(second);

        assert_eq!(entities.len(), 1, "the id stays occupied once");
        let entity = entities.get(5).expect("id 5 is live");
        assert_eq!(entity.kind, EntityKind::Zombie);
        assert_eq!(entity.position, [2.0, 65.0, 2.0]);
    }

    #[test]
    fn remove_reports_the_ids_it_removed_and_clear_empties_the_table() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Player));
        entities.insert(spawned(2, EntityKind::Item));
        entities.insert(spawned(3, EntityKind::Arrow));

        assert_eq!(entities.remove(&[]), 0);
        assert_eq!(entities.remove(&[3, 9]), 1, "only the live id counts");
        assert_eq!(entities.remove(&[1, 2]), 2);
        assert!(entities.is_empty());
        assert!(entities.get(3).is_none());

        entities.insert(spawned(7, EntityKind::Pig));
        entities.clear();
        assert!(entities.is_empty());
        assert_eq!(entities.len(), 0);
    }

    #[test]
    fn relative_move_adds_the_converted_delta() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Player);
        entity.position = [0.5, 64.0, -1.0];
        entity.last_tick_position = [0.5, 64.0, -1.0];
        entities.insert(entity);

        // 127/32 is the wire byte 127 already converted: 0.5 + 3.96875.
        entities.apply_relative_move(1, [3.96875, 0.0, 0.0]);
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(entity.position, [4.46875, 64.0, -1.0]);
        assert_eq!(
            entity.last_tick_position,
            [0.5, 64.0, -1.0],
            "the pair only moves at tick"
        );

        // -1/32 in each of the other axes.
        entities.apply_relative_move(1, [0.0, 0.0, -0.03125]);
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(entity.position, [4.46875, 64.0, -1.03125]);

        // An id that is not live is left alone rather than refused.
        entities.apply_relative_move(9, [1.0, 1.0, 1.0]);
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn teleport_overwrites_and_leaves_the_pairs_alone() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.position = [1.0, 2.0, 3.0];
        entity.last_tick_position = [9.0, 8.0, 7.0];
        entity.yaw = 45.0;
        entity.last_tick_yaw = 30.0;
        entities.insert(entity);

        entities.apply_teleport(1, [10.0, 64.0, -2.5], 90.0, 12.0, true);
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(entity.position, [10.0, 64.0, -2.5], "absolute, not a delta");
        assert_eq!(entity.yaw, 90.0);
        assert_eq!(entity.pitch, 12.0);
        assert!(entity.on_ground);
        assert_eq!(
            entity.last_tick_position,
            [9.0, 8.0, 7.0],
            "the pair copies at tick"
        );
        assert_eq!(entity.last_tick_yaw, 30.0);
    }

    #[test]
    fn look_head_look_and_velocity_set_the_state() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Creeper));

        entities.apply_look(1, 90.0, 45.0);
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(entity.yaw, 90.0);
        assert_eq!(entity.pitch, 45.0);
        assert_eq!(entity.last_tick_yaw, 0.0, "the pair only moves at tick");

        entities.apply_head_look(1, 180.0);
        assert_eq!(entities.get(1).map(|entity| entity.head_yaw), Some(180.0));
        assert_eq!(
            entities.get(1).map(|entity| entity.last_tick_head_yaw),
            Some(0.0)
        );

        entities.apply_velocity(1, [0.25, -0.125, 0.0]);
        assert_eq!(
            entities.get(1).map(|entity| entity.velocity),
            Some([0.25, -0.125, 0.0])
        );
    }

    #[test]
    fn metadata_merges_by_index() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.metadata = block(vec![
            (0, MetadataValue::Byte(1)),
            (2, MetadataValue::Int(5)),
        ]);
        entities.insert(entity);

        // The update carries index 2: it replaces; index 0 stays.
        entities.apply_metadata(1, block(vec![(2, MetadataValue::Int(9))]));
        assert_eq!(
            entities.get(1).expect("id 1 is live").metadata.entries,
            vec![(0, MetadataValue::Byte(1)), (2, MetadataValue::Int(9))]
        );

        // A new index appends in the update's own order.
        entities.apply_metadata(
            1,
            block(vec![
                (1, MetadataValue::Short(3)),
                (30, MetadataValue::Float(0.5)),
            ]),
        );
        assert_eq!(
            entities.get(1).expect("id 1 is live").metadata.entries,
            vec![
                (0, MetadataValue::Byte(1)),
                (2, MetadataValue::Int(9)),
                (1, MetadataValue::Short(3)),
                (30, MetadataValue::Float(0.5)),
            ]
        );

        // An id that is not live changes nothing.
        entities.apply_metadata(9, block(vec![(0, MetadataValue::Byte(0))]));
        assert_eq!(entities.len(), 1);
    }

    #[test]
    fn status_two_starts_the_hurt_window() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Zombie));

        entities.apply_status(1, 2);
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(entity.hurt_ticks, 10, "the source's hurt window");
        assert_eq!(entity.limb_swing_amount, 1.5, "the source's flinch");
        assert_eq!(entity.death_ticks, 0, "hurt is not death");
    }

    #[test]
    fn status_three_starts_the_death_counter() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Zombie));

        entities.apply_status(1, 3);
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(
            entity.death_ticks, 0,
            "the source seeds deathTime at zero; the ticks count it up"
        );
        assert_eq!(entity.hurt_ticks, 0, "death is not hurt");
    }

    #[test]
    fn an_unmapped_status_changes_nothing() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Zombie));
        let before = entities.get(1).expect("id 1 is live").clone();

        // 6, 7 (taming), 9 (eat accepted), 10 (grass) and 14 (zombie
        // villager) mean nothing to this store yet; neither does a value the
        // wire's table never names.
        for status in [6_i8, 7, 9, 10, 14, 77] {
            entities.apply_status(1, status);
        }
        assert_eq!(entities.get(1).expect("id 1 is live"), &before);
    }

    #[test]
    fn equipment_slots_land() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Player));

        entities.set_equipment(1, 0, Some(item(276)));
        entities.set_equipment(1, 4, Some(item(310)));
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(entity.equipment[0], Some(item(276)), "the held slot");
        assert_eq!(entity.equipment[4], Some(item(310)), "the helmet slot");
        assert_eq!(entity.equipment[1..4], [None, None, None]);

        entities.set_equipment(1, 0, None);
        assert_eq!(
            entities.get(1).map(|entity| entity.equipment[0].clone()),
            Some(None)
        );

        // A slot outside 0..=4 is refused at the wire edge; one that lands
        // here anyway changes nothing.
        entities.set_equipment(1, 5, Some(item(1)));
        entities.set_equipment(1, -1, Some(item(1)));
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(entity.equipment[4], Some(item(310)));
        assert_eq!(entity.equipment[1..4], [None, None, None]);
    }

    #[test]
    fn attachment_records_the_holder_and_detaches_on_a_negative_one() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Cow));

        entities.set_attachment(1, 7, true);
        assert_eq!(
            entities.get(1).expect("id 1 is live").attachment,
            Some(Attachment {
                holder: 7,
                leash: true
            })
        );

        entities.set_attachment(1, 9, false);
        assert_eq!(
            entities.get(1).expect("id 1 is live").attachment,
            Some(Attachment {
                holder: 9,
                leash: false
            })
        );

        // A negative holder is the source's detach.
        entities.set_attachment(1, -1, true);
        assert_eq!(entities.get(1).expect("id 1 is live").attachment, None);
    }

    #[test]
    fn kind_data_keeps_the_spawn_extras() {
        let mut entities = Entities::new();
        let mut item = spawned(1, EntityKind::Item);
        item.data = KindData::Item {
            id: 276,
            count: 3,
            damage: 12,
        };
        entities.insert(item);

        let mut painting = spawned(2, EntityKind::Painting);
        painting.data = KindData::Painting {
            title: Arc::from("Kebab"),
            facing: 2,
        };
        entities.insert(painting);

        assert_eq!(
            entities.get(1).map(|entity| entity.data.clone()),
            Some(KindData::Item {
                id: 276,
                count: 3,
                damage: 12
            })
        );
        assert_eq!(
            entities.get(2).map(|entity| entity.data.clone()),
            Some(KindData::Painting {
                title: Arc::from("Kebab"),
                facing: 2
            })
        );
        assert_eq!(
            entities.get(1).map(|entity| entity.data == KindData::None),
            Some(false)
        );
    }

    #[test]
    fn the_limb_pair_eases_from_rest_over_a_steady_walk() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.position = [0.0, 64.0, 0.0];
        entity.last_tick_position = [0.0, 64.0, 0.0];
        entities.insert(entity);

        // A steady walk of 0.2 blocks a tick gives sqrt(0.04) x 4 = 0.8, and
        // the first eased amount is (0.8 - 0) x 0.4 = 0.32; each later tick
        // closes 0.4 of what is left of 0.8, and the swing accumulates the
        // amounts.
        let amounts = [0.32_f32, 0.512, 0.6272, 0.69632];
        let swings = [0.32_f32, 0.832, 1.4592, 2.15552];
        for (step, (&amount, &swing)) in amounts.iter().zip(swings.iter()).enumerate() {
            entities.apply_relative_move(1, [0.2, 0.0, 0.0]);
            entities.tick();
            let entity = entities.get(1).expect("id 1 is live");
            assert!(
                (entity.limb_swing_amount - amount).abs() < 1e-5,
                "tick {}: the eased amount, saw {}",
                step + 1,
                entity.limb_swing_amount
            );
            assert!(
                (entity.limb_swing - swing).abs() < 1e-5,
                "tick {}: the accumulated swing, saw {}",
                step + 1,
                entity.limb_swing
            );
        }

        // The pair holds the value from before the tick's own update.
        assert!(
            (entities
                .get(1)
                .expect("id 1 is live")
                .last_limb_swing_amount
                - 0.6272)
                .abs()
                < 1e-5
        );

        // Kept walking, the eased amount approaches the 0.8 input.
        for _ in 0..40 {
            entities.apply_relative_move(1, [0.2, 0.0, 0.0]);
            entities.tick();
        }
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.limb_swing_amount - 0.8).abs() < 1e-4,
            "the fixed point, saw {}",
            entity.limb_swing_amount
        );
    }

    #[test]
    fn a_long_step_clamps_the_limb_factor_at_one_and_a_stop_eases_it_down() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.position = [0.0, 64.0, 0.0];
        entity.last_tick_position = [0.0, 64.0, 0.0];
        entities.insert(entity);

        // One 0.5-block step gives sqrt(0.25) x 4 = 2.0, clamped to 1.0, so
        // the first amount is (1.0 - 0) x 0.4 = 0.4.
        entities.apply_relative_move(1, [0.5, 0.0, 0.0]);
        entities.tick();
        assert!(
            (entities.get(1).expect("id 1 is live").limb_swing_amount - 0.4).abs() < 1e-5,
            "the clamped amount, saw {}",
            entities.get(1).expect("id 1 is live").limb_swing_amount
        );

        // Standing still, the amount eases toward zero: 0.4 - 0.4 x 0.4.
        entities.tick();
        assert!(
            (entities.get(1).expect("id 1 is live").limb_swing_amount - 0.24).abs() < 1e-5,
            "the eased-down amount, saw {}",
            entities.get(1).expect("id 1 is live").limb_swing_amount
        );
    }

    #[test]
    fn the_swing_runs_a_six_tick_period_and_returns_to_zero() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Player));

        entities.get_mut(1).expect("id 1 is live").swing();
        assert_eq!(
            entities.get(1).expect("id 1 is live").swing_progress,
            0.0,
            "the swing starts with the next tick"
        );

        // The counter advances before the progress is read, resets from six,
        // so the six readings after the start run zero to five sixths and
        // the seventh tick is the reset.
        let readings = [
            0.0_f32,
            1.0 / 6.0,
            2.0 / 6.0,
            3.0 / 6.0,
            4.0 / 6.0,
            5.0 / 6.0,
        ];
        for (step, &reading) in readings.iter().enumerate() {
            entities.tick();
            let entity = entities.get(1).expect("id 1 is live");
            assert!(
                (entity.swing_progress - reading).abs() < 1e-5,
                "tick {}: progress {}, wanted {}",
                step + 1,
                entity.swing_progress,
                reading
            );
            if step > 0 {
                assert!(
                    (entity.last_swing_progress - readings[step - 1]).abs() < 1e-5,
                    "tick {}: the pair holds the reading from the tick before",
                    step + 1
                );
            }
        }

        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.swing_progress - 0.0).abs() < 1e-5,
            "the seventh tick resets the cycle, saw {}",
            entity.swing_progress
        );
        assert!(
            (entity.last_swing_progress - 5.0 / 6.0).abs() < 1e-5,
            "and the pair holds the reading before the reset"
        );
    }

    #[test]
    fn a_swing_before_the_halfway_point_is_ignored() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Player));

        entities.get_mut(1).expect("id 1 is live").swing();
        entities.tick(); // counter 0
        entities.get_mut(1).expect("id 1 is live").swing(); // ignored: not yet halfway
        entities.tick(); // counter 1
        assert!(
            (entities.get(1).expect("id 1 is live").swing_progress - 1.0 / 6.0).abs() < 1e-5,
            "the early swing did not restart the cycle, saw {}",
            entities.get(1).expect("id 1 is live").swing_progress
        );

        entities.tick(); // counter 2
        entities.tick(); // counter 3, the halfway point
        assert!((entities.get(1).expect("id 1 is live").swing_progress - 0.5).abs() < 1e-5);
        entities.get_mut(1).expect("id 1 is live").swing(); // past halfway: restarts
        entities.tick(); // counter 0 again
        assert!(
            (entities.get(1).expect("id 1 is live").swing_progress - 0.0).abs() < 1e-5,
            "the late swing restarted the cycle, saw {}",
            entities.get(1).expect("id 1 is live").swing_progress
        );
    }

    #[test]
    fn the_hurt_window_counts_down_to_zero() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Zombie));
        entities.apply_status(1, 2);
        assert_eq!(entities.get(1).expect("id 1 is live").hurt_ticks, 10);

        for _ in 0..3 {
            entities.tick();
        }
        assert_eq!(entities.get(1).expect("id 1 is live").hurt_ticks, 7);

        for _ in 0..7 {
            entities.tick();
        }
        assert_eq!(entities.get(1).expect("id 1 is live").hurt_ticks, 0);

        for _ in 0..5 {
            entities.tick();
        }
        assert_eq!(
            entities.get(1).expect("id 1 is live").hurt_ticks,
            0,
            "and it stays at zero"
        );
    }

    #[test]
    fn the_death_counter_counts_up_once_started() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Zombie));

        entities.tick();
        entities.tick();
        assert_eq!(
            entities.get(1).expect("id 1 is live").death_ticks,
            0,
            "a living entity does not count"
        );

        entities.apply_status(1, 3);
        assert_eq!(
            entities.get(1).expect("id 1 is live").death_ticks,
            0,
            "the status seeds the counter; it does not count yet"
        );
        entities.tick();
        assert_eq!(entities.get(1).expect("id 1 is live").death_ticks, 1);
        entities.tick();
        assert_eq!(entities.get(1).expect("id 1 is live").death_ticks, 2);

        // Another death status does not restart the count — the source
        // only zeroes the health there.
        entities.apply_status(1, 3);
        assert_eq!(entities.get(1).expect("id 1 is live").death_ticks, 2);
    }

    #[test]
    fn age_advances_once_per_tick() {
        let mut entities = Entities::new();
        entities.insert(spawned(1, EntityKind::Cow));
        entities.tick();
        entities.tick();
        entities.tick();
        assert_eq!(entities.get(1).expect("id 1 is live").age, 3);
    }

    #[test]
    fn the_mob_body_chases_the_head_across_the_angle_seam() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.yaw = -170.0;
        entity.head_yaw = -170.0;
        entity.render_yaw_offset = 170.0;
        entities.insert(entity);

        // The still mob's body helper bounds the body to the head: the
        // wrapped difference from 170 to -170 is +20 the short way, and the
        // first held tick keeps the full reach, so the body lands on
        // -170 - 20 = -190 and sits there while the head stays put.
        for tick in 1..=3 {
            entities.tick();
            let entity = entities.get(1).expect("id 1 is live");
            assert!(
                (entity.render_yaw_offset + 190.0).abs() < 1e-4,
                "tick {}: the crossed body, saw {}",
                tick,
                entity.render_yaw_offset
            );
        }
        assert_eq!(entities.get(1).expect("id 1 is live").head_yaw, -170.0);
    }

    #[test]
    fn the_player_body_chases_toward_the_movement_direction() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Player);
        entity.position = [0.5, 64.0, 0.0];
        entity.last_tick_position = [0.0, 64.0, 0.0];
        entity.yaw = 90.0;
        entity.head_yaw = 0.0;
        entity.render_yaw_offset = 170.0;
        entities.insert(entity);

        // Moving along +x puts the movement target at atan2(0, 0.5) in
        // degrees minus 90 = -90. The body eases 0.3 of the wrapped step
        // from 170, landing on 170 + 30 = 200; the body-yaw bound then
        // clamps the 90-to-200 difference to -75, so the body reads
        // 90 - (-75) = 165, and the past-50 release pulls 75 x 0.2 = 15 of
        // it back: 150.
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.render_yaw_offset - 150.0).abs() < 1e-4,
            "after one tick, saw {}",
            entity.render_yaw_offset
        );
    }

    #[test]
    fn the_player_body_chases_the_body_yaw_through_a_swing() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Player);
        entity.yaw = -170.0;
        entity.head_yaw = 0.0;
        entity.render_yaw_offset = 170.0;
        entities.insert(entity);
        entities.get_mut(1).expect("id 1 is live").swing();

        // The swing starts with the next tick, so the first tick still
        // holds the render yaw and the body bound alone lands it on
        // -170 - 20 = -190; from the second tick the running swing hands
        // the target to the body yaw and the body eases 0.3 of the wrapped
        // 20 degrees: -190 + 6 = -184.
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.render_yaw_offset + 190.0).abs() < 1e-4,
            "tick 1: the held body, saw {}",
            entity.render_yaw_offset
        );

        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.render_yaw_offset + 184.0).abs() < 1e-4,
            "tick 2: the swung body, saw {}",
            entity.render_yaw_offset
        );
    }

    #[test]
    fn the_player_body_chase_settles_within_the_body_yaws_reach() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Player);
        entity.yaw = 100.0;
        entity.head_yaw = 0.0;
        entity.render_yaw_offset = 0.0;
        entities.insert(entity);

        // The still player holds its own render yaw as the target, so the
        // eased step is zero; the body bound then clamps the 100-degree
        // difference to 75 (100 - 75 = 25) and the release past 50 pulls it
        // another 75 x 0.2 = 15: 25 + 15 = 40.
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.render_yaw_offset - 40.0).abs() < 1e-4,
            "saw {}",
            entity.render_yaw_offset
        );
        assert!(
            entity.prev_render_yaw_offset.abs() < 1e-6,
            "the pair holds the pre-chase value, saw {}",
            entity.prev_render_yaw_offset
        );
    }

    #[test]
    fn the_moving_mob_body_chase_bounds_the_head_to_the_body() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.position = [0.5, 64.0, 0.0];
        entity.last_tick_position = [0.0, 64.0, 0.0];
        entity.yaw = 0.0;
        entity.head_yaw = 170.0;
        entity.render_yaw_offset = 0.0;
        entities.insert(entity);

        // A moving mob snaps its body to the body yaw (0) and bounds the
        // head to 75 of it: the wrapped difference from the head is -170,
        // clamped to the reach, so the head lands on 0 - (-75) = 75; the
        // pair keeps the pre-bound head for the renderer to lerp from.
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            entity.render_yaw_offset.abs() < 1e-6,
            "the snapped body, saw {}",
            entity.render_yaw_offset
        );
        assert!(
            (entity.head_yaw - 75.0).abs() < 1e-4,
            "the bounded head, saw {}",
            entity.head_yaw
        );
        assert!(
            (entity.last_tick_head_yaw - 170.0).abs() < 1e-4,
            "the pair holds the pre-bound head, saw {}",
            entity.last_tick_head_yaw
        );
    }

    #[test]
    fn the_held_mob_body_chase_decays_after_ten_ticks() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.yaw = 0.0;
        entity.head_yaw = 0.0;
        entity.render_yaw_offset = 90.0;
        entities.insert(entity);

        // Held with the head steady, the body bounds toward the head at
        // the full 75 reach for the first ten ticks, ...
        for tick in 1..=10 {
            entities.tick();
            let entity = entities.get(1).expect("id 1 is live");
            assert!(
                (entity.render_yaw_offset - 75.0).abs() < 1e-4,
                "held tick {}: the full reach body, saw {}",
                tick,
                entity.render_yaw_offset
            );
        }

        // ... then the reach decays a tenth of 75 each held tick — 67.5,
        // 60, 52.5 — down to zero by the twentieth.
        let decay = [67.5_f32, 60.0, 52.5];
        for (index, &expected) in decay.iter().enumerate() {
            entities.tick();
            let entity = entities.get(1).expect("id 1 is live");
            assert!(
                (entity.render_yaw_offset - expected).abs() < 1e-4,
                "held tick {}: the decayed body, saw {}",
                index + 11,
                entity.render_yaw_offset
            );
        }
        for _ in 0..7 {
            entities.tick();
        }
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            entity.render_yaw_offset.abs() < 1e-4,
            "the decayed body sits on the head, saw {}",
            entity.render_yaw_offset
        );
    }

    #[test]
    fn the_pose_pairs_fold_back_across_the_seam() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Player);
        entity.yaw = -170.0;
        entity.head_yaw = 170.0;
        entity.render_yaw_offset = 170.0;
        entities.insert(entity);

        // The held player's chase lands the body on -170 - 20 = -190 while
        // the copied partner still reads 170: the fold steps the partner
        // 360 back, so the pair reads the short way, -190 against -190.
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.render_yaw_offset + 190.0).abs() < 1e-4,
            "the crossed body, saw {}",
            entity.render_yaw_offset
        );
        assert!(
            (entity.prev_render_yaw_offset + 190.0).abs() < 1e-4,
            "the folded partner, saw {}",
            entity.prev_render_yaw_offset
        );
    }

    #[test]
    fn the_pose_pairs_fold_forward_across_the_seam() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Player);
        entity.yaw = 170.0;
        entity.head_yaw = -170.0;
        entity.render_yaw_offset = -170.0;
        entities.insert(entity);

        // The held player's chase lands the body on 170 + 20 = 190 while
        // the copied partner still reads -170: the fold steps the partner
        // 360 forward, so the pair reads 190 against 190.
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.render_yaw_offset - 190.0).abs() < 1e-4,
            "the crossed body, saw {}",
            entity.render_yaw_offset
        );
        assert!(
            (entity.prev_render_yaw_offset - 190.0).abs() < 1e-4,
            "the folded partner, saw {}",
            entity.prev_render_yaw_offset
        );
    }

    #[test]
    fn the_fold_keeps_a_half_turn_delta_seated_at_the_boundary() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.position = [0.5, 64.0, 0.0];
        entity.last_tick_position = [0.0, 64.0, 0.0];
        entity.yaw = 190.0;
        entity.head_yaw = 190.0;
        entity.render_yaw_offset = 10.0;
        entities.insert(entity);

        // The moving mob's body snaps to 190, exactly half a turn from the
        // copied partner, 10. The source's `>= 180` fold fires at exactly
        // half a turn and seats the partner on 10 + 360 = 370, so the
        // delta reads -180, not +180.
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.prev_render_yaw_offset - 370.0).abs() < 1e-4,
            "the seated half-turn partner, saw {}",
            entity.prev_render_yaw_offset
        );
    }

    #[test]
    fn the_fold_leaves_a_negative_half_turn_delta_alone() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.position = [0.5, 64.0, 0.0];
        entity.last_tick_position = [0.0, 64.0, 0.0];
        entity.yaw = -170.0;
        entity.head_yaw = -170.0;
        entity.render_yaw_offset = 10.0;
        entities.insert(entity);

        // The partner sits exactly half a turn the other way, -180 from
        // the body: the source's `< -180` test is strict, so the partner
        // stays put and the pair keeps the -180 delta.
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.prev_render_yaw_offset - 10.0).abs() < 1e-4,
            "the untouched half-turn partner, saw {}",
            entity.prev_render_yaw_offset
        );
    }

    #[test]
    fn the_fold_pulls_a_delta_past_half_a_turn_back_in_range() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Zombie);
        entity.position = [0.5, 64.0, 0.0];
        entity.last_tick_position = [0.0, 64.0, 0.0];
        entity.yaw = -170.5;
        entity.head_yaw = -170.5;
        entity.render_yaw_offset = 10.0;
        entities.insert(entity);

        // The partner is half a degree past the half turn, -180.5 from the
        // body, so the fold steps it 360 back: 10 - 360 = -350, a 179.5
        // degree delta.
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert!(
            (entity.prev_render_yaw_offset + 350.0).abs() < 1e-4,
            "the pulled-back partner, saw {}",
            entity.prev_render_yaw_offset
        );
    }

    #[test]
    fn the_pose_pairs_copy_at_tick() {
        let mut entities = Entities::new();
        let mut entity = spawned(1, EntityKind::Player);
        entity.position = [1.0, 2.0, 3.0];
        entity.yaw = 30.0;
        entity.pitch = 10.0;
        entity.head_yaw = 20.0;
        entities.insert(entity);

        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(entity.last_tick_position, [1.0, 2.0, 3.0]);
        assert_eq!(entity.last_tick_yaw, 30.0);
        assert_eq!(entity.last_tick_pitch, 10.0);
        assert_eq!(entity.last_tick_head_yaw, 20.0);

        // A teleport leaves the pair alone; the next tick copies it.
        entities.apply_teleport(1, [9.0, 8.0, 7.0], 90.0, 5.0, false);
        assert_eq!(
            entities.get(1).expect("id 1 is live").last_tick_position,
            [1.0, 2.0, 3.0]
        );
        entities.tick();
        let entity = entities.get(1).expect("id 1 is live");
        assert_eq!(entity.last_tick_position, [9.0, 8.0, 7.0]);
        assert_eq!(entity.last_tick_yaw, 90.0);
        assert_eq!(entity.last_tick_pitch, 5.0);
    }
}
