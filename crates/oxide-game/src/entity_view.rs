//! The per-entity view frame: what one tracked entity looks like at the tick
//! boundary, extracted from the store's raw state for the window.
//!
//! The session owns the store (`oxide_world::entity::Entities`) and builds one
//! [`EntityFrame`] per entity once per tick; the window draws from the frames
//! and never sees the store or the world. Everything a frame carries that the
//! store holds raw — the per-kind metadata fields, the flag bits, a boss's
//! health, the brightness at the entity's feet — is extracted here, against
//! the source's own accessors and index table (`protocol-47-reference.md` §6.2,
//! `refs/_src/MCP-919/src/minecraft/net/minecraft/`).
//!
//! The player-list model and the display-name composition live here too: the
//! list is connection-scoped state the session mutates from clientbound 0x38,
//! and [`display_name`] is the one composition point a player's frame text
//! takes — the entry's own text wrapped in its team's clauses
//! ([`format_entry`]).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use oxide_proto_v47::entity::{MetadataItem, MetadataValue};
use oxide_world::entity::{Entities, Entity, EntityKind, KindData};
use oxide_world::light;
use oxide_world::world::World;

use crate::scoreboard::{Scoreboard, format_entry};

/// The metadata index of the base flag byte (`Entity.java:286`,
/// `dataWatcher.addObject(0, Byte(0))`).
const FLAGS_INDEX: u8 = 0;

/// The flag bit the source's `setSneaking` writes: `setFlag(1, …)`, bit `1`
/// (`Entity.java:2154`, `isSneaking:2144-2146` reads the same bit).
const SNEAKING_BIT: i8 = 0x02;

/// The flag bit the source's `setInvisible` writes: `setFlag(5, …)`, bit `5`
/// (`Entity.java:2190`, `isInvisible:2173-2175` reads the same bit).
const INVISIBLE_BIT: i8 = 0x20;

/// The metadata index of the custom name string (`Entity.java:2614-2618`;
/// `hasCustomName:2622-2625` reads it).
const CUSTOM_NAME_INDEX: u8 = 2;

/// The metadata index of the "always show" byte (`Entity.java:2632-2635` reads
/// it as `== 1`), the same field `setAlwaysRenderNameTag:2627-2630` writes.
const NAME_VISIBLE_INDEX: u8 = 3;

/// The metadata index of the living entity's health float
/// (`EntityLivingBase.java:214-217`).
const HEALTH_INDEX: u8 = 6;

/// The metadata index of a dropped item's stack (`EntityItem.java:75-78`).
const ITEM_STACK_INDEX: u8 = 10;

/// The metadata index of an item frame's displayed stack
/// (`EntityItemFrame.java:33-36`).
const FRAME_ITEM_INDEX: u8 = 8;

/// The metadata index of an item frame's own item rotation
/// (`EntityItemFrame.java:36`; read at `:177-180`).
const FRAME_ROTATION_INDEX: u8 = 9;

/// The ender dragon's maximum health: the attribute base the source's
/// `applyEntityAttributes` sets, `200.0` (`EntityDragon.java:96`).
const ENDER_DRAGON_MAX_HEALTH: f32 = 200.0;

/// The wither's maximum health: the attribute base the source's
/// `applyEntityAttributes` sets, `300.0` (`EntityWither.java:608`).
const WITHER_MAX_HEALTH: f32 = 300.0;

/// One entity as the window's surfaces see it at a tick boundary.
///
/// Every pose pair's partner holds the value from before the tick the frame
/// was built after — the store's `last_tick_*` fields, exactly the pair the
/// renderer interpolates with.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityFrame {
    /// The entity id the server uses.
    pub id: i32,
    /// What the entity is.
    pub kind: EntityKind,
    /// The profile UUID; players only.
    pub uuid: Option<String>,
    /// The player's account name, from the player-list record; `None` for
    /// every other kind and for a player without a record.
    pub name: Option<Arc<str>>,
    /// The position at the last tick, in blocks.
    pub prev: [f64; 3],
    /// The position in blocks.
    pub pos: [f64; 3],
    /// The body yaw at the last tick, in degrees.
    pub prev_yaw: f32,
    /// The body yaw in degrees.
    pub yaw: f32,
    /// The pitch at the last tick, in degrees.
    pub prev_pitch: f32,
    /// The pitch in degrees.
    pub pitch: f32,
    /// The head's yaw at the last tick, in degrees.
    pub prev_head_yaw: f32,
    /// The head's yaw in degrees.
    pub head_yaw: f32,
    /// The body's render yaw in degrees (`EntityLivingBase.renderYawOffset`).
    pub render_yaw_offset: f32,
    /// The render yaw from before the last tick's chase.
    pub prev_render_yaw_offset: f32,
    /// Whether the wire last reported the entity on the ground.
    pub on_ground: bool,
    /// Whether the flag byte's invisible bit is set.
    pub invisible: bool,
    /// Whether the flag byte's sneaking bit is set.
    pub sneaking: bool,
    /// The entity's age in ticks (the source's `ticksExisted`).
    pub age: u32,
    /// The limb swing's accumulated distance.
    pub limb_swing: f32,
    /// The limb swing's eased amount.
    pub limb_swing_amount: f32,
    /// The limb swing amount at the last tick.
    pub prev_limb_swing_amount: f32,
    /// The arm swing's progress within its cycle, `0.0..1.0`.
    pub swing_progress: f32,
    /// The swing progress at the last tick.
    pub prev_swing_progress: f32,
    /// The hurt window's remaining ticks (`EntityLivingBase.hurtTime`).
    pub hurt_ticks: u16,
    /// The death animation's ticks since death started
    /// (`EntityLivingBase.deathTime`).
    pub death_ticks: u16,
    /// The brightness at the entity's feet, from the world's light — the
    /// source's `getBrightness` combination and table (see [`snapshot`]).
    pub brightness: f32,
    /// The health pair `(current, maximum)` for the kinds whose maximum is a
    /// pinned class constant — this milestone's two boss kinds; `None` for
    /// every other kind.
    pub health: Option<(f32, f32)>,
    /// The composed nametag text, when the name may show.
    pub nametag: Option<Arc<str>>,
    /// The kind-specific extras the renderer reads.
    pub extra: EntityExtra,
}

/// The kind-specific extras a frame carries beyond the pose and metadata
/// flags.
///
/// The fields are exactly the values a renderer's per-class path reads; the
/// metadata they come from is the §6.2 index table. `EntityExtra::None` is
/// the kinds with nothing extra (and the global entities, which are tracked
/// but never drawn).
#[derive(Debug, Clone, PartialEq)]
pub enum EntityExtra {
    /// An entity with no kind-specific extras.
    None,
    /// Another player; the animated model's own state is all it needs.
    Player,
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
    /// An item frame's content and own rotation.
    ItemFrame {
        /// The displayed stack; `None` is the empty frame.
        item: Option<MetadataItem>,
        /// The item's rotation within the frame, `0..8` (byte index 9).
        rotation: u8,
    },
    /// A boat.
    Boat,
    /// A minecart.
    Minecart,
    /// A projectile kind without extra state (arrow, snowball, egg, ender
    /// pearl, eye of ender, potion, XP bottle, firework, the fireballs, the
    /// wither skull).
    Projectile,
    /// An experience orb.
    Orb,
    /// A mob with its metadata-driven state.
    Mob(MobExtra),
}

/// The per-mob state a renderer reads from metadata (§6.2's per-class map).
#[derive(Debug, Clone, PartialEq)]
pub enum MobExtra {
    /// A sheep's wool colour and shearing (byte index 16: bits 0–3 colour,
    /// bit 4 sheared — `EntitySheep.getFleeceColor:262`, `getSheared:279`).
    Sheep {
        /// The wool colour, `0..16`.
        wool: u8,
        /// Whether the sheep is sheared.
        sheared: bool,
    },
    /// A wolf's taming and collar (byte index 16 bit 2 tamed —
    /// `EntityTameable.isTamed:116-119`; byte index 20 bits 0–3 collar —
    /// `EntityWolf.getCollarColor:542`).
    Wolf {
        /// Whether the wolf is tamed.
        tamed: bool,
        /// The collar colour, `0..16`; the source's default is red, `14`
        /// (`EntityWolf.entityInit:133`).
        collar: u8,
        /// Whether the wolf is angry — its own sheet and tail
        /// (`EntityWolf.isAngry`, byte 16 bit 1).
        angry: bool,
        /// Whether the wolf sits (`EntityTameable.isSitting`, byte 16 bit 0).
        sitting: bool,
        /// The wolf's health, float index 18 (`EntityWolf`'s own watcher value),
        /// which the tamed tail's droop reads.
        health: f32,
    },
    /// A slime's size — a magma cube's too, which inherits the fold (byte
    /// index 16 — `EntitySlime.getSlimeSize:69`; `EntityMagmaCube.java`:10).
    Slime {
        /// The size, one or more; the source's default is `1`.
        size: u8,
    },
    /// An ocelot's variant (byte index 18 — `EntityOcelot.getCatType:301`).
    Ocelot {
        /// The cat type: `0` wild, `1..4` the cat coats.
        variant: u8,
        /// Whether the cat is tamed (`EntityTameable.isTamed`, byte 16 bit 2) —
        /// the sitting fold and the scene's own size ladder.
        tamed: bool,
        /// Whether the cat sits (`EntityTameable.isSitting`, byte 16 bit 0) —
        /// the folded leg and tail chain.
        sitting: bool,
    },
    /// A rabbit's variant (byte index 18 — `EntityRabbit.getRabbitType:417`).
    Rabbit {
        /// The rabbit type.
        variant: u8,
        /// Whether the rabbit is a child (byte 12, negative = child) —
        /// the renderer's own hop-shaped child fold.
        child: bool,
    },
    /// A villager's profession and age (int index 16 through the source's own
    /// fold — `EntityVillager.getProfession:360`; the growing age at byte
    /// index 12, negative = child).
    Villager {
        /// The profession, `0..5`.
        profession: u8,
        /// Whether the villager is a child.
        child: bool,
    },
    /// A bat's hanging state (byte index 16 bit 0 —
    /// `EntityBat.getIsBatHanging:96`).
    Bat {
        /// Whether the bat hangs.
        hanging: bool,
    },
    /// A pig's saddle (byte index 16 bit 0 —
    /// `EntityPig.getSaddled:178`).
    Pig {
        /// Whether the pig is saddled.
        saddle: bool,
    },
    /// A horse's state: type byte index 19 (`EntityHorse.getHorseType:127`),
    /// variant int index 20 (`getHorseVariant:138`), the tamed and saddled
    /// bits of the flags int index 16 (`getHorseWatchableBoolean`,
    /// `:175-178`, read through `:249` and `:345`), and the growing age at
    /// byte index 12.
    Horse {
        /// The horse type: `0` horse, `1` donkey, `2` mule, `3` zombie, `4`
        /// skeleton.
        variant: u8,
        /// The colour: the variant's low byte.
        colour: u8,
        /// Whether the horse is tamed.
        tamed: bool,
        /// Whether the horse is saddled.
        saddle: bool,
        /// Whether the horse is an adult.
        adult: bool,
        /// The marking: the variant int's high byte (`0..5`) — the marking layer's gate
        /// (`EntityHorse.getHorseVariant:138`).
        markings: u8,
        /// Whether the horse is chested — the flags int 16 bit 3, the mule chests' gate
        /// (`EntityHorse.getHorseWatchableBoolean:175-178` through `:249`).
        chested: bool,
        /// The worn armour's table index (`0..4`) — the armour watcher's slot
        /// (`RenderHorse.getEntityTexture`:76).
        armour: u8,
    },
    /// A ghast's attacking flag (byte 16 — `EntityGhast.isAttacking`), which swaps its
    /// sheet (`RenderGhast.getEntityTexture`:21-24).
    Ghast {
        /// Whether the ghast is shooting.
        shooting: bool,
    },
    /// A guardian's elder flag (int 16's bit 2 — `EntityGuardian`), the size ladder and
    /// the elder sheet (`RenderGuardian.getEntityTexture`:177-180).
    Guardian {
        /// Whether the guardian is an elder.
        elder: bool,
    },
    /// A wither's spawn invulnerability timer (int 20 — `EntityWither.getInvulTime`),
    /// which the sheet flickers by.
    Wither {
        /// The timer in ticks; zero once the spawn shield drops.
        invul_time: u16,
    },
    /// A zombie's villager flag (byte index 13 —
    /// `EntityZombie.isVillager:197`).
    Zombie {
        /// Whether the zombie is a villager zombie.
        villager: bool,
    },
    /// A creeper; its fuse and powered state are not read this milestone.
    Creeper,
    /// An enderman; its carried block and screaming are not read this
    /// milestone.
    Enderman,
    /// A mob with no metadata-driven state in this milestone.
    Other,
}

/// One player-list entry as the session keeps it.
///
/// Deliberately distinct from the wire's `PlayerListEntry`: this is the merged
/// state the list holds across packets, not one packet's transport shape.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerListRecord {
    /// The profile's hyphenated UUID: the key the list holds the record
    /// under, carried on the record itself so one that travels alone — a
    /// `PlayerList` report's entry — still names its player.
    pub uuid: String,
    /// The account name.
    pub name: String,
    /// The profile properties (name/value pairs; a signed property's
    /// signature is dropped).
    pub properties: Vec<(String, String)>,
    /// The gamemode.
    pub gamemode: u8,
    /// The ping in milliseconds, kept as sent (negatives included).
    pub latency: i32,
    /// The display name, the raw wire string.
    pub display_name: Option<String>,
}

/// The player list: every known profile UUID and its merged record.
///
/// Keyed by the hyphenated UUID string the spawn packets use, so a spawned
/// player's frame can look its record up directly. Connection-scoped state:
/// a dimension rebuild clears it, and `SpawnPlayer`'s UUID is its key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerList {
    /// The records, keyed by the hyphenated UUID.
    entries: BTreeMap<String, PlayerListRecord>,
}

impl PlayerList {
    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many records are held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no record is held.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Removes every record.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The record for a hyphenated UUID, if the list holds one.
    pub fn get(&self, uuid: &str) -> Option<&PlayerListRecord> {
        self.entries.get(uuid)
    }

    /// Every record, in ascending UUID order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &PlayerListRecord)> + '_ {
        self.entries
            .iter()
            .map(|(uuid, record)| (uuid.as_str(), record))
    }

    /// Adds or replaces an entry (the add action; a duplicate add is the
    /// newer state, as the source's map put is).
    pub fn insert(&mut self, uuid: [u8; 16], record: PlayerListRecord) {
        self.entries.insert(hyphenated(&uuid), record);
    }

    /// Removes an entry, returning it when one was held.
    pub fn remove(&mut self, uuid: [u8; 16]) -> Option<PlayerListRecord> {
        self.entries.remove(&hyphenated(&uuid))
    }

    /// Applies a gamemode update; `false` when no record is held.
    pub fn set_gamemode(&mut self, uuid: [u8; 16], gamemode: u8) -> bool {
        match self.entries.get_mut(&hyphenated(&uuid)) {
            Some(record) => {
                record.gamemode = gamemode;
                true
            }
            None => false,
        }
    }

    /// Applies a latency update; `false` when no record is held.
    pub fn set_latency(&mut self, uuid: [u8; 16], latency: i32) -> bool {
        match self.entries.get_mut(&hyphenated(&uuid)) {
            Some(record) => {
                record.latency = latency;
                true
            }
            None => false,
        }
    }

    /// Applies a display-name update (a null clears it); `false` when no
    /// record is held.
    pub fn set_display_name(&mut self, uuid: [u8; 16], display_name: Option<String>) -> bool {
        match self.entries.get_mut(&hyphenated(&uuid)) {
            Some(record) => {
                record.display_name = display_name;
                true
            }
            None => false,
        }
    }
}

/// The composed display text for a player, from its list entry and the board.
///
/// The entry's display name when the server sent one, else the account name,
/// wrapped in the team's clauses ([`format_entry`], the source's
/// `ScorePlayerTeam.formatString:95-98`) — the text a player's nametag and
/// its tab-list row both read, composed once here. `None` when no entry is
/// known — §8's ordering obligation has the list item arrive before the
/// spawn, so a player frame without one is the not-yet-named case.
pub fn display_name(entry: Option<&PlayerListRecord>, board: &Scoreboard) -> Option<String> {
    let entry = entry?;
    let fallback = entry.display_name.as_deref().unwrap_or(entry.name.as_str());
    Some(format_entry(board, &entry.name, fallback))
}

/// One frame per entity, in ascending id order.
///
/// Everything the frame's extraction reads the store raw for is resolved here
/// (see the field docs and the private helpers), and the brightness is
/// sampled from the world when the session holds one: the source's
/// `getBrightness` chain at the entity's feet — the block the sample lands
/// in, the world's light there (the greater of the two kinds), and the
/// provider's table for that level. A position no loaded column holds answers
/// zero brightness, the source's own unloaded answer (`Entity.java:1256-1260`
/// through `World.getLightBrightness:845-848`).
pub fn snapshot(
    entities: &Entities,
    world: Option<&World>,
    player_list: &PlayerList,
    board: &Scoreboard,
) -> Vec<EntityFrame> {
    // The ridden ids, one pass over the store: a mount attachment (`!leash`)
    // names its holder — the entity being ridden — and the living renderer
    // drops a ridden entity's name.
    let ridden: BTreeSet<i32> = entities
        .iter()
        .filter_map(|entity| match &entity.attachment {
            Some(attachment) if !attachment.leash => Some(attachment.holder),
            _ => None,
        })
        .collect();
    entities
        .iter()
        .map(|entity| frame_of(entity, world, player_list, board, &ridden))
        .collect()
}

/// The state one entity's frame carries, extracted from the store.
fn frame_of(
    entity: &Entity,
    world: Option<&World>,
    player_list: &PlayerList,
    board: &Scoreboard,
    ridden: &BTreeSet<i32>,
) -> EntityFrame {
    let flags = byte_at(entity, FLAGS_INDEX).unwrap_or(0);
    EntityFrame {
        id: entity.id,
        kind: entity.kind,
        uuid: entity.uuid.clone(),
        name: player_name(entity, player_list),
        prev: entity.last_tick_position,
        pos: entity.position,
        prev_yaw: entity.last_tick_yaw,
        yaw: entity.yaw,
        prev_pitch: entity.last_tick_pitch,
        pitch: entity.pitch,
        prev_head_yaw: entity.last_tick_head_yaw,
        head_yaw: entity.head_yaw,
        render_yaw_offset: entity.render_yaw_offset,
        prev_render_yaw_offset: entity.prev_render_yaw_offset,
        on_ground: entity.on_ground,
        invisible: flags & INVISIBLE_BIT != 0,
        sneaking: flags & SNEAKING_BIT != 0,
        age: entity.age,
        limb_swing: entity.limb_swing,
        limb_swing_amount: entity.limb_swing_amount,
        prev_limb_swing_amount: entity.last_limb_swing_amount,
        swing_progress: entity.swing_progress,
        prev_swing_progress: entity.last_swing_progress,
        hurt_ticks: entity.hurt_ticks,
        death_ticks: entity.death_ticks,
        brightness: brightness(world, entity.position),
        health: health(entity),
        nametag: nametag(entity, player_list, board, ridden),
        extra: extra(entity),
    }
}

/// The brightness at an entity's feet, from the world's light.
///
/// The chain is the source's float `Entity.getBrightness`
/// (`Entity.java:1256-1260`): the sample at `posY + getEyeHeight()` reads
/// the world's light brightness at that block
/// (`World.getLightBrightness:845-848`) and runs it through the provider's
/// table (see [`brightness_of_level`]). Here the sample lands in the block
/// the feet stand in; a position no loaded column holds answers nothing
/// (zero), which is the source's unloaded answer; otherwise the level is the
/// world's light there — the greater of the two kinds, exactly the number
/// the mesher shades its cells with (`oxide_world::light::light_at`).
///
/// The eye height arrives with the models, so this milestone pins the feet
/// block (the tracked-entity interfaces), and the sample position is pinned
/// by a literal test against a scripted light state.
fn brightness(world: Option<&World>, position: [f64; 3]) -> f32 {
    let Some(world) = world else {
        return 0.0;
    };
    let level = light::light_at(
        world,
        position[0].floor() as i32,
        position[1].floor() as i32,
        position[2].floor() as i32,
    );
    brightness_of_level(level)
}

/// The provider's light-to-brightness table at one level
/// (`WorldProvider.generateLightBrightnessTable:63-71`, the base provider's
/// `f = 0`): `f1 = 1 - level / 15`, then `(1 - f1) / (f1 * 3 + 1)` — level
/// `15` is `1.0`, level `12` is `0.5`, level `0` is `0.0`.
///
/// Overworld scope this milestone: the nether's `f = 0.1` floor term
/// (`WorldProviderHell.generateLightBrightnessTable:34-43`) is not ported.
fn brightness_of_level(level: u8) -> f32 {
    let f1 = 1.0 - f32::from(level) / 15.0;
    (1.0 - f1) / (f1 * 3.0 + 1.0)
}

/// The health pair for the kinds whose maximum is a pinned class constant.
///
/// The two boss kinds this milestone pins: the ender dragon, `200`
/// (`EntityDragon.java:96`), and the wither, `300` (`EntityWither.java:608`).
/// The current health is the living metadata's index 6 float
/// (`EntityLivingBase.java:214-217`) as the last update left it; when no
/// update has landed, a fresh client entity holds its maximum, seeded by the
/// constructor's `setHealth(getMaxHealth())` (`EntityLivingBase.java:201`).
/// Every other kind answers `None`: its §6.2 health index may exist, but no
/// consumer exists this milestone and no non-boss maximum is pinned.
fn health(entity: &Entity) -> Option<(f32, f32)> {
    let maximum = match entity.kind {
        EntityKind::EnderDragon => ENDER_DRAGON_MAX_HEALTH,
        EntityKind::WitherBoss => WITHER_MAX_HEALTH,
        _ => return None,
    };
    Some((float_at(entity, HEALTH_INDEX).unwrap_or(maximum), maximum))
}

/// The composed nametag text, when the name may show.
///
/// The source's own resolution, as far as the session can evaluate it: the
/// living renderer's last line (`RendererLivingEntity.canRenderName:547-581`)
/// drops an invisible entity and one that is ridden — an entity its own
/// `riddenByEntity` names (`Entity.java:63-64`; the term at `:580`). The
/// store keeps the attachment on the passenger ("what it rides"), so the
/// ridden set arrives as [`snapshot`]'s reverse lookup over the attachment
/// holders; a player is offered without any flag — the class override
/// answers `true` (`EntityPlayer.getAlwaysRenderNameTagForRender:2163-2166`),
/// so the text is [`display_name`]'s composition of the list entry — while a
/// mob takes the living renderer's two branches
/// (`RenderLiving.canRenderName:21-24`, `flag || (hasCustomName && pointedEntity)`):
/// the cursor term cannot be evaluated here, and the flag's own branch
/// carries no text without a name, so a custom name (index 2,
/// `Entity.hasCustomName:2622-2625`; the always-show byte is index 3,
/// `Entity.getAlwaysRenderNameTag:2632-2635`) is what a mob's text resolves
/// to.
fn nametag(
    entity: &Entity,
    player_list: &PlayerList,
    board: &Scoreboard,
    ridden: &BTreeSet<i32>,
) -> Option<Arc<str>> {
    let flags = byte_at(entity, FLAGS_INDEX).unwrap_or(0);
    if flags & INVISIBLE_BIT != 0 || ridden.contains(&entity.id) {
        return None;
    }
    if entity.kind == EntityKind::Player {
        let uuid = entity.uuid.as_deref()?;
        return display_name(player_list.get(uuid), board).map(Arc::from);
    }
    let always = byte_at(entity, NAME_VISIBLE_INDEX).unwrap_or(0) == 1;
    let named = string_at(entity, CUSTOM_NAME_INDEX).filter(|name| !name.is_empty());
    if always || named.is_some() {
        named.map(Arc::from)
    } else {
        None
    }
}

/// The player's account name from the player-list record, when the list
/// holds one.
///
/// The name the source's below-name lookup reads (`RenderPlayer.java:148`'s
/// `entityIn.getName()`): the record's own name, not the display-name
/// composition the nametag draws. `None` for every kind but a player and for
/// a player the list never named — a frame without a record resolves no
/// below-name line, the same way it resolves no nametag.
fn player_name(entity: &Entity, player_list: &PlayerList) -> Option<Arc<str>> {
    if entity.kind != EntityKind::Player {
        return None;
    }
    let uuid = entity.uuid.as_deref()?;
    player_list
        .get(uuid)
        .map(|record| Arc::from(record.name.as_str()))
}

/// The kind-specific extras one entity's frame carries.
fn extra(entity: &Entity) -> EntityExtra {
    match entity.kind {
        EntityKind::Player => EntityExtra::Player,
        // The stack is the metadata's own slot (index 10), the source's path
        // for a dropped item; an empty slot or no entry reads the spawn's
        // data, which the wire leaves empty (§6.3) — the air stack's zeroes.
        EntityKind::Item => {
            let stack = match last_value(entity, ITEM_STACK_INDEX) {
                Some(MetadataValue::Item(Some(item))) => item.clone(),
                Some(MetadataValue::Item(None)) => MetadataItem {
                    id: 0,
                    count: 0,
                    damage: 0,
                    nbt: None,
                },
                _ => match &entity.data {
                    KindData::Item { id, count, damage } => MetadataItem {
                        id: *id,
                        count: *count,
                        damage: *damage,
                        nbt: None,
                    },
                    _ => MetadataItem {
                        id: 0,
                        count: 0,
                        damage: 0,
                        nbt: None,
                    },
                },
            };
            EntityExtra::Item {
                id: stack.id,
                count: stack.count,
                damage: stack.damage,
            }
        }
        // The frame's content and own rotation (indices 8 and 9).
        EntityKind::ItemFrame => EntityExtra::ItemFrame {
            item: item_at(entity, FRAME_ITEM_INDEX),
            rotation: byte_at(entity, FRAME_ROTATION_INDEX)
                .unwrap_or(0)
                .rem_euclid(8) as u8,
        },
        // The art and hanging are the spawn's own.
        EntityKind::Painting => match &entity.data {
            KindData::Painting { title, facing } => EntityExtra::Painting {
                title: Arc::clone(title),
                facing: *facing,
            },
            _ => EntityExtra::None,
        },
        EntityKind::Boat => EntityExtra::Boat,
        EntityKind::Minecart => EntityExtra::Minecart,
        EntityKind::XpOrb => EntityExtra::Orb,
        // The projectile family: no state beyond the pose and the velocity.
        EntityKind::Arrow
        | EntityKind::Snowball
        | EntityKind::Egg
        | EntityKind::EnderPearl
        | EntityKind::EyeOfEnder
        | EntityKind::Potion
        | EntityKind::XpBottle
        | EntityKind::Firework
        | EntityKind::Fireball
        | EntityKind::SmallFireball
        | EntityKind::WitherSkull => EntityExtra::Projectile,
        // The global entities and the kinds outside the table are tracked,
        // never drawn.
        EntityKind::Global | EntityKind::Unknown => EntityExtra::None,
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
        | EntityKind::Villager => EntityExtra::Mob(mob_extra(entity.kind, entity)),
    }
}

/// The per-mob state one mob's metadata carries (§6.2).
fn mob_extra(kind: EntityKind, entity: &Entity) -> MobExtra {
    match kind {
        // A sheep: byte 16, bits 0–3 the wool and bit 4 the shearing
        // (`EntitySheep.getFleeceColor:262`, `getSheared:279`).
        EntityKind::Sheep => {
            let data = byte_at(entity, 16).unwrap_or(0) as u8;
            MobExtra::Sheep {
                wool: data & 0x0f,
                sheared: data & 0x10 != 0,
            }
        }
        // A wolf: byte 16's tamed (bit 2, `EntityTameable.isTamed:116-119`), angry
        // (bit 1) and sitting (bit 0) flags, byte 20's collar (`EntityWolf`, default
        // red `14`) and float 18's health, which the tamed tail's droop reads.
        EntityKind::Wolf => {
            let flags = byte_at(entity, 16).unwrap_or(0);
            MobExtra::Wolf {
                tamed: flags & 0x04 != 0,
                collar: (byte_at(entity, 20).unwrap_or(14) & 0x0f) as u8,
                angry: flags & 0x02 != 0,
                sitting: flags & 0x01 != 0,
                health: float_at(entity, 18).unwrap_or(20.0),
            }
        }
        // A slime or magma cube: byte 16 the size
        // (`EntitySlime.getSlimeSize:69`; `EntityMagmaCube.java`:10 inherits
        // it), default one.
        EntityKind::Slime | EntityKind::LavaSlime => MobExtra::Slime {
            size: byte_at(entity, 16).unwrap_or(1) as u8,
        },
        // An ocelot: byte 18 the cat type (`EntityOcelot.getCatType:301`) and byte 16's
        // tamed (bit 2) and sitting (bit 0) flags (`EntityTameable`).
        EntityKind::Ozelot => {
            let flags = byte_at(entity, 16).unwrap_or(0);
            MobExtra::Ocelot {
                variant: byte_at(entity, 18).unwrap_or(0) as u8,
                tamed: flags & 0x04 != 0,
                sitting: flags & 0x01 != 0,
            }
        }
        // A rabbit: byte 18 the rabbit type (`EntityRabbit.getRabbitType:417`),
        // byte 12 the growing age (negative = child).
        EntityKind::Rabbit => MobExtra::Rabbit {
            variant: byte_at(entity, 18).unwrap_or(0) as u8,
            child: byte_at(entity, 12).unwrap_or(0) < 0,
        },
        // A villager: int 16 the profession through the source's own fold
        // (`EntityVillager.getProfession:360`, `max(p % 5, 0)`), byte 12 the
        // growing age (negative = child).
        EntityKind::Villager => MobExtra::Villager {
            profession: (int_at(entity, 16).unwrap_or(0) % 5).max(0) as u8,
            child: byte_at(entity, 12).unwrap_or(0) < 0,
        },
        // A bat: byte 16 bit 0 the hanging state
        // (`EntityBat.getIsBatHanging:96`).
        EntityKind::Bat => MobExtra::Bat {
            hanging: byte_at(entity, 16).unwrap_or(0) & 0x01 != 0,
        },
        // A pig: byte 16 bit 0 the saddle (`EntityPig.getSaddled:178`).
        EntityKind::Pig => MobExtra::Pig {
            saddle: byte_at(entity, 16).unwrap_or(0) & 0x01 != 0,
        },
        // A horse: int 16's bits 1 tamed, 2 saddled and 3 chested
        // (`EntityHorse.getHorseWatchableBoolean:175-178` through `:249`/`:345`),
        // byte 19 the type (`getHorseType:127`), int 20 the colour/variant whose
        // high byte is the marking (`getHorseVariant:138`), int 22 the worn armour's
        // table index, byte 12 the growing age.
        EntityKind::EntityHorse => {
            let flags = int_at(entity, 16).unwrap_or(0);
            let variant = int_at(entity, 20).unwrap_or(0);
            MobExtra::Horse {
                variant: byte_at(entity, 19).unwrap_or(0) as u8,
                colour: (variant & 0xff) as u8,
                tamed: flags & 0x02 != 0,
                saddle: flags & 0x04 != 0,
                adult: byte_at(entity, 12).unwrap_or(0) >= 0,
                markings: ((variant >> 8) & 0xff) as u8,
                chested: flags & 0x08 != 0,
                armour: int_at(entity, 22).unwrap_or(0).clamp(0, 4) as u8,
            }
        }
        // A ghast: byte 16 the attacking flag (`EntityGhast.isAttacking`).
        EntityKind::Ghast => MobExtra::Ghast {
            shooting: byte_at(entity, 16).unwrap_or(0) != 0,
        },
        // A guardian: int 16's bit 2 the elder flag (`EntityGuardian`).
        EntityKind::Guardian => MobExtra::Guardian {
            elder: int_at(entity, 16).unwrap_or(0) & 0x04 != 0,
        },
        // A wither: int 20 the spawn invulnerability's timer
        // (`EntityWither.getInvulTime`); negatives and values past the wire's short
        // clamp to it.
        EntityKind::WitherBoss => MobExtra::Wither {
            invul_time: int_at(entity, 20).unwrap_or(0).clamp(0, 65_535) as u16,
        },
        // A zombie: byte 13 the villager flag
        // (`EntityZombie.isVillager:195-198` answers `== 1`).
        EntityKind::Zombie => MobExtra::Zombie {
            villager: byte_at(entity, 13).unwrap_or(0) == 1,
        },
        EntityKind::Creeper => MobExtra::Creeper,
        EntityKind::Enderman => MobExtra::Enderman,
        _ => MobExtra::Other,
    }
}

/// The last value a metadata index holds, the source's own rule: a block may
/// repeat an index and the watcher keeps the last write.
fn last_value(entity: &Entity, index: u8) -> Option<&MetadataValue> {
    entity
        .metadata
        .entries
        .iter()
        .rev()
        .find(|(entry_index, _)| *entry_index == index)
        .map(|(_, value)| value)
}

/// The byte a metadata index holds, when it holds a byte.
fn byte_at(entity: &Entity, index: u8) -> Option<i8> {
    match last_value(entity, index) {
        Some(MetadataValue::Byte(value)) => Some(*value),
        _ => None,
    }
}

/// The int a metadata index holds, when it holds an int.
fn int_at(entity: &Entity, index: u8) -> Option<i32> {
    match last_value(entity, index) {
        Some(MetadataValue::Int(value)) => Some(*value),
        _ => None,
    }
}

/// The float a metadata index holds, when it holds a float.
fn float_at(entity: &Entity, index: u8) -> Option<f32> {
    match last_value(entity, index) {
        Some(MetadataValue::Float(value)) => Some(*value),
        _ => None,
    }
}

/// The stack a metadata index holds, when it holds a non-empty slot.
fn item_at(entity: &Entity, index: u8) -> Option<MetadataItem> {
    match last_value(entity, index) {
        Some(MetadataValue::Item(item)) => item.clone(),
        _ => None,
    }
}

/// The string a metadata index holds, when it holds a string.
fn string_at(entity: &Entity, index: u8) -> Option<&str> {
    match last_value(entity, index) {
        Some(MetadataValue::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

/// The hyphenated form of a profile UUID, the shape the spawn decoder writes
/// (`oxide-proto-v47`'s `format_uuid`, `entity.rs:1604-1616`): the list keys
/// it, the spawns carry it, and a record's own `uuid` holds it.
pub(crate) fn hyphenated(uuid: &[u8; 16]) -> String {
    const HEX: [char; 16] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
    ];
    let mut out = String::with_capacity(36);
    for (index, byte) in uuid.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push(HEX[(byte >> 4) as usize]);
        out.push(HEX[(byte & 0x0f) as usize]);
    }
    out
}

#[cfg(test)]
mod tests {
    //! The extraction tables: every field's metadata index against a scripted
    //! map, every default against the source's own `DataWatcher.addObject`
    //! default, the flag bits, the nametag visibility cases, the boss health
    //! maxima, the brightness table and its scripted sample, and the list's
    //! merge rules.

    use super::*;
    use oxide_proto_v47::column::{ColumnData, SectionData, block_index};
    use oxide_proto_v47::entity::Metadata;
    use oxide_world::chunk::SECTION_SIZE;
    use oxide_world::entity::Attachment;

    /// A metadata block of the listed entries, in order.
    fn metadata(entries: &[(u8, MetadataValue)]) -> Metadata {
        Metadata {
            entries: entries.to_vec(),
        }
    }

    /// The frame one entity produces against no world and an empty list.
    fn frame_for(entity: &Entity) -> EntityFrame {
        let mut entities = Entities::new();
        entities.insert(entity.clone());
        snapshot(&entities, None, &PlayerList::new(), &Scoreboard::new())
            .into_iter()
            .next()
            .expect("one frame")
    }

    /// The extra one entity produces.
    fn extra_for(entity: &Entity) -> EntityExtra {
        frame_for(entity).extra
    }

    /// The player-list entry for the UUID `[7; 16]` with the given name.
    fn listed(name: &str) -> (Entity, PlayerList) {
        let mut entity = Entity::new(7, EntityKind::Player);
        entity.uuid = Some(hyphenated(&[7; 16]));
        let mut list = PlayerList::new();
        list.insert(
            [7; 16],
            PlayerListRecord {
                name: name.to_owned(),
                ..PlayerListRecord::default()
            },
        );
        (entity, list)
    }

    /// The frame one entity produces against a list.
    fn frame_against(entity: &Entity, list: &PlayerList) -> EntityFrame {
        let mut entities = Entities::new();
        entities.insert(entity.clone());
        snapshot(&entities, None, list, &Scoreboard::new())
            .into_iter()
            .next()
            .expect("one frame")
    }

    /// The frames the listed entities produce against no world and an empty
    /// list.
    fn frames_of(entities: &[Entity]) -> Vec<EntityFrame> {
        let mut store = Entities::new();
        for entity in entities {
            store.insert(entity.clone());
        }
        snapshot(&store, None, &PlayerList::new(), &Scoreboard::new())
    }

    /// The frame with this id among the listed frames.
    fn frame_named(frames: &[EntityFrame], id: i32) -> &EntityFrame {
        frames.iter().find(|frame| frame.id == id).expect("a frame")
    }

    // -----------------------------------------------------------------
    // The flag bits.
    // -----------------------------------------------------------------

    #[test]
    fn the_flag_bits_are_the_sources_own() {
        // `setFlag(1, …)` is sneaking (0x02) and `setFlag(5, …)` invisible
        // (0x20); every other bit leaves both false.
        let cases = [
            (0x02_i8, true, false),
            (0x20, false, true),
            (0x22, true, true),
            (0x01, false, false),
            (0x10, false, false),
        ];
        for (bits, sneaking, invisible) in cases {
            let mut entity = Entity::new(4, EntityKind::Pig);
            entity.metadata = metadata(&[(0, MetadataValue::Byte(bits))]);
            let frame = frame_for(&entity);
            assert_eq!(
                (frame.sneaking, frame.invisible),
                (sneaking, invisible),
                "the flag byte {bits:#04x} resolves both bits"
            );
        }
        // An absent flag byte is the source's zero.
        let frame = frame_for(&Entity::new(4, EntityKind::Pig));
        assert_eq!((frame.sneaking, frame.invisible), (false, false));
    }

    // -----------------------------------------------------------------
    // The frame's store mapping.
    // -----------------------------------------------------------------

    #[test]
    fn the_frame_carries_the_stores_pose_pairs() {
        let mut entity = Entity::new(21, EntityKind::Player);
        entity.uuid = Some("the-player".to_owned());
        entity.position = [1.5, 65.0, -2.5];
        entity.last_tick_position = [1.25, 65.0, -2.375];
        entity.yaw = 90.0;
        entity.last_tick_yaw = 80.0;
        entity.pitch = 10.0;
        entity.last_tick_pitch = 5.0;
        entity.head_yaw = 95.0;
        entity.last_tick_head_yaw = 85.0;
        entity.render_yaw_offset = 70.0;
        entity.prev_render_yaw_offset = 65.0;
        entity.on_ground = true;
        entity.age = 33;
        entity.limb_swing = 1.5;
        entity.limb_swing_amount = 0.32;
        entity.last_limb_swing_amount = 0.24;
        entity.swing_progress = 2.0 / 6.0;
        entity.last_swing_progress = 1.0 / 6.0;
        entity.hurt_ticks = 4;
        entity.death_ticks = 7;
        let frame = frame_for(&entity);
        assert_eq!(frame.id, 21);
        assert_eq!(frame.kind, EntityKind::Player);
        assert_eq!(frame.uuid.as_deref(), Some("the-player"));
        assert_eq!(frame.prev, [1.25, 65.0, -2.375]);
        assert_eq!(frame.pos, [1.5, 65.0, -2.5]);
        assert_eq!((frame.prev_yaw, frame.yaw), (80.0, 90.0));
        assert_eq!((frame.prev_pitch, frame.pitch), (5.0, 10.0));
        assert_eq!((frame.prev_head_yaw, frame.head_yaw), (85.0, 95.0));
        assert_eq!(
            (frame.prev_render_yaw_offset, frame.render_yaw_offset),
            (65.0, 70.0)
        );
        assert!(frame.on_ground);
        assert_eq!(frame.age, 33);
        assert_eq!(frame.limb_swing, 1.5);
        assert_eq!(
            (frame.prev_limb_swing_amount, frame.limb_swing_amount),
            (0.24, 0.32)
        );
        assert_eq!(
            (frame.prev_swing_progress, frame.swing_progress),
            (1.0 / 6.0, 2.0 / 6.0)
        );
        assert_eq!((frame.hurt_ticks, frame.death_ticks), (4, 7));
    }

    #[test]
    fn the_frames_are_ascending_by_id() {
        let mut entities = Entities::new();
        entities.insert(Entity::new(9, EntityKind::Pig));
        entities.insert(Entity::new(3, EntityKind::Item));
        entities.insert(Entity::new(5, EntityKind::Cow));
        let frames = snapshot(&entities, None, &PlayerList::new(), &Scoreboard::new());
        let ids: Vec<i32> = frames.iter().map(|frame| frame.id).collect();
        assert_eq!(ids, vec![3, 5, 9]);
    }

    // -----------------------------------------------------------------
    // The per-kind extraction: one test per variant.
    // -----------------------------------------------------------------

    #[test]
    fn the_sheep_reads_index_16() {
        let mut entity = Entity::new(1, EntityKind::Sheep);
        entity.metadata = metadata(&[(16, MetadataValue::Byte(0x0a))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Sheep {
                wool: 10,
                sheared: false
            })
        );
        entity.metadata = metadata(&[(16, MetadataValue::Byte((0x0f | 0x10) as i8))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Sheep {
                wool: 15,
                sheared: true
            })
        );
        // The source's default is `Byte(0)`.
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Sheep)),
            EntityExtra::Mob(MobExtra::Sheep {
                wool: 0,
                sheared: false
            })
        );
    }

    #[test]
    fn the_wolf_reads_16_and_20() {
        let mut entity = Entity::new(1, EntityKind::Wolf);
        entity.metadata = metadata(&[
            (16, MetadataValue::Byte(0x04)),
            (20, MetadataValue::Byte(0x03)),
        ]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Wolf {
                tamed: true,
                collar: 3,
                angry: false,
                sitting: false,
                health: 20.0
            })
        );
        entity.metadata = metadata(&[(16, MetadataValue::Byte(0x02))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Wolf {
                tamed: false,
                collar: 14,
                angry: true,
                sitting: false,
                health: 20.0
            }),
            "the source's collar default is red, 14"
        );
        entity.metadata = metadata(&[
            (16, MetadataValue::Byte(0x05)),
            (18, MetadataValue::Float(7.5)),
        ]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Wolf {
                tamed: true,
                collar: 14,
                angry: false,
                sitting: true,
                health: 7.5
            })
        );
    }

    #[test]
    fn the_slime_reads_index_16() {
        let mut entity = Entity::new(1, EntityKind::Slime);
        entity.metadata = metadata(&[(16, MetadataValue::Byte(3))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Slime { size: 3 })
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Slime)),
            EntityExtra::Mob(MobExtra::Slime { size: 1 }),
            "the source's default size is one"
        );
    }

    #[test]
    fn the_magma_cube_reads_the_same_size_byte() {
        let mut entity = Entity::new(1, EntityKind::LavaSlime);
        entity.metadata = metadata(&[(16, MetadataValue::Byte(2))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Slime { size: 2 }),
            "the magma cube inherits the slime's size fold"
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::LavaSlime)),
            EntityExtra::Mob(MobExtra::Slime { size: 1 })
        );
    }

    #[test]
    fn the_ocelot_reads_index_18() {
        let mut entity = Entity::new(1, EntityKind::Ozelot);
        entity.metadata = metadata(&[(18, MetadataValue::Byte(2))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Ocelot {
                variant: 2,
                tamed: false,
                sitting: false
            })
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Ozelot)),
            EntityExtra::Mob(MobExtra::Ocelot {
                variant: 0,
                tamed: false,
                sitting: false
            })
        );
        entity.metadata = metadata(&[
            (16, MetadataValue::Byte(0x05)),
            (18, MetadataValue::Byte(1)),
        ]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Ocelot {
                variant: 1,
                tamed: true,
                sitting: true
            })
        );
    }

    #[test]
    fn the_rabbit_reads_index_18() {
        let mut entity = Entity::new(1, EntityKind::Rabbit);
        entity.metadata = metadata(&[(18, MetadataValue::Byte(99))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Rabbit {
                variant: 99,
                child: false
            })
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Rabbit)),
            EntityExtra::Mob(MobExtra::Rabbit {
                variant: 0,
                child: false
            })
        );
    }

    #[test]
    fn the_villager_folds_the_profession_and_reads_the_age() {
        let mut entity = Entity::new(1, EntityKind::Villager);
        entity.metadata = metadata(&[(16, MetadataValue::Int(7)), (12, MetadataValue::Byte(-2))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Villager {
                profession: 2,
                child: true
            }),
            "the source's fold is `max(profession % 5, 0)`"
        );
        entity.metadata = metadata(&[(16, MetadataValue::Int(-3))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Villager {
                profession: 0,
                child: false
            })
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Villager)),
            EntityExtra::Mob(MobExtra::Villager {
                profession: 0,
                child: false
            })
        );
    }

    #[test]
    fn the_bat_reads_the_hanging_bit() {
        let mut entity = Entity::new(1, EntityKind::Bat);
        entity.metadata = metadata(&[(16, MetadataValue::Byte(1))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Bat { hanging: true })
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Bat)),
            EntityExtra::Mob(MobExtra::Bat { hanging: false })
        );
    }

    #[test]
    fn the_pig_reads_the_saddle_bit() {
        let mut entity = Entity::new(1, EntityKind::Pig);
        entity.metadata = metadata(&[(16, MetadataValue::Byte(1))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Pig { saddle: true })
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Pig)),
            EntityExtra::Mob(MobExtra::Pig { saddle: false })
        );
    }

    #[test]
    fn the_horse_reads_its_four_indices() {
        let mut entity = Entity::new(1, EntityKind::EntityHorse);
        entity.metadata = metadata(&[
            (16, MetadataValue::Int(0x02 | 0x04)),
            (19, MetadataValue::Byte(1)),
            (20, MetadataValue::Int(0x0302)),
            (12, MetadataValue::Byte(-1)),
        ]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Horse {
                variant: 1,
                colour: 0x02,
                tamed: true,
                saddle: true,
                adult: false,
                markings: 0x03,
                chested: false,
                armour: 0
            })
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::EntityHorse)),
            EntityExtra::Mob(MobExtra::Horse {
                variant: 0,
                colour: 0,
                tamed: false,
                saddle: false,
                adult: true,
                markings: 0,
                chested: false,
                armour: 0
            })
        );
    }

    #[test]
    fn the_zombie_reads_the_villager_byte() {
        let mut entity = Entity::new(1, EntityKind::Zombie);
        entity.metadata = metadata(&[(13, MetadataValue::Byte(1))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Mob(MobExtra::Zombie { villager: true })
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Zombie)),
            EntityExtra::Mob(MobExtra::Zombie { villager: false })
        );
    }

    #[test]
    fn the_mobless_mobs_are_their_own_extras() {
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Creeper)),
            EntityExtra::Mob(MobExtra::Creeper)
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Enderman)),
            EntityExtra::Mob(MobExtra::Enderman)
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Spider)),
            EntityExtra::Mob(MobExtra::Other)
        );
    }

    #[test]
    fn an_item_reads_index_10() {
        let mut entity = Entity::new(1, EntityKind::Item);
        entity.metadata = metadata(&[(
            10,
            MetadataValue::Item(Some(MetadataItem {
                id: 5,
                count: 3,
                damage: 1,
                nbt: None,
            })),
        )]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Item {
                id: 5,
                count: 3,
                damage: 1
            }
        );
        // The fresh entity holds the source's air stack: the zeroes, and an
        // empty slot reads the same.
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::Item)),
            EntityExtra::Item {
                id: 0,
                count: 0,
                damage: 0
            }
        );
        entity.metadata = metadata(&[(10, MetadataValue::Item(None))]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Item {
                id: 0,
                count: 0,
                damage: 0
            }
        );
    }

    #[test]
    fn an_item_frame_reads_its_two_indices() {
        let mut entity = Entity::new(1, EntityKind::ItemFrame);
        entity.metadata = metadata(&[
            (
                8,
                MetadataValue::Item(Some(MetadataItem {
                    id: 3,
                    count: 1,
                    damage: 7,
                    nbt: None,
                })),
            ),
            (9, MetadataValue::Byte(5)),
        ]);
        assert_eq!(
            extra_for(&entity),
            EntityExtra::ItemFrame {
                item: Some(MetadataItem {
                    id: 3,
                    count: 1,
                    damage: 7,
                    nbt: None,
                }),
                rotation: 5
            }
        );
        assert_eq!(
            extra_for(&Entity::new(1, EntityKind::ItemFrame)),
            EntityExtra::ItemFrame {
                item: None,
                rotation: 0
            }
        );
    }

    #[test]
    fn a_painting_carries_the_spawn_data() {
        let mut entity = Entity::new(1, EntityKind::Painting);
        entity.data = KindData::Painting {
            title: Arc::from("Kebab"),
            facing: 2,
        };
        assert_eq!(
            extra_for(&entity),
            EntityExtra::Painting {
                title: Arc::from("Kebab"),
                facing: 2
            }
        );
    }

    #[test]
    fn the_object_families_map_their_extras() {
        let cases = [
            (EntityKind::Boat, EntityExtra::Boat),
            (EntityKind::Minecart, EntityExtra::Minecart),
            (EntityKind::Arrow, EntityExtra::Projectile),
            (EntityKind::EnderPearl, EntityExtra::Projectile),
            (EntityKind::WitherSkull, EntityExtra::Projectile),
            (EntityKind::XpOrb, EntityExtra::Orb),
            (EntityKind::Player, EntityExtra::Player),
            (EntityKind::Global, EntityExtra::None),
            (EntityKind::Unknown, EntityExtra::None),
        ];
        for (kind, expected) in cases {
            assert_eq!(extra_for(&Entity::new(1, kind)), expected, "{kind:?}");
        }
    }

    // -----------------------------------------------------------------
    // The nametag visibility rule.
    // -----------------------------------------------------------------

    #[test]
    fn a_nameless_mob_has_no_nametag() {
        let frame = frame_for(&Entity::new(1, EntityKind::Cow));
        assert_eq!(frame.nametag, None);
        // The flag needs a name to show.
        let mut flagged = Entity::new(1, EntityKind::Cow);
        flagged.metadata = metadata(&[(3, MetadataValue::Byte(1))]);
        assert_eq!(frame_for(&flagged).nametag, None);
        // An empty custom name is no name (`hasCustomName`'s length check).
        let mut empty = Entity::new(1, EntityKind::Cow);
        empty.metadata = metadata(&[(2, MetadataValue::String(String::new()))]);
        assert_eq!(frame_for(&empty).nametag, None);
    }

    #[test]
    fn a_named_mob_shows() {
        let mut entity = Entity::new(1, EntityKind::Cow);
        entity.metadata = metadata(&[(2, MetadataValue::String("Bessie".to_owned()))]);
        assert_eq!(
            frame_for(&entity).nametag.as_deref(),
            Some("Bessie"),
            "a custom name shows"
        );
        // The always-show byte with a name shows the same text.
        entity.metadata = metadata(&[
            (2, MetadataValue::String("Bessie".to_owned())),
            (3, MetadataValue::Byte(1)),
        ]);
        assert_eq!(frame_for(&entity).nametag.as_deref(), Some("Bessie"));
    }

    #[test]
    fn a_player_name_comes_from_the_list_entry() {
        let (entity, list) = listed("OxideDev");
        assert_eq!(
            frame_against(&entity, &list).nametag.as_deref(),
            Some("OxideDev"),
            "the account name names the player"
        );
        let mut list = list;
        assert!(list.set_display_name([7; 16], Some("§bOxideDev".to_owned())));
        assert_eq!(
            frame_against(&entity, &list).nametag.as_deref(),
            Some("§bOxideDev"),
            "the display name beats the account name"
        );
    }

    #[test]
    fn a_player_without_an_entry_has_no_name() {
        let mut entity = Entity::new(7, EntityKind::Player);
        entity.uuid = Some(hyphenated(&[7; 16]));
        assert_eq!(frame_against(&entity, &PlayerList::new()).nametag, None);
    }

    #[test]
    fn the_frames_name_is_the_account_name_the_list_holds() {
        let (entity, list) = listed("OxideDev");
        assert_eq!(
            frame_against(&entity, &list).name.as_deref(),
            Some("OxideDev"),
            "the account name the below-name score lookup reads"
        );
        // The display name does not replace it (`RenderPlayer.java:148`).
        let mut list = list;
        assert!(list.set_display_name([7; 16], Some("§bOxideDev".to_owned())));
        assert_eq!(
            frame_against(&entity, &list).name.as_deref(),
            Some("OxideDev"),
            "the account name, not the display name"
        );
        // No record resolves no name, as it resolves no nametag; every other
        // kind resolves none either.
        let mut orphan = Entity::new(7, EntityKind::Player);
        orphan.uuid = Some(hyphenated(&[7; 16]));
        assert_eq!(frame_against(&orphan, &PlayerList::new()).name, None);
        assert_eq!(frame_for(&Entity::new(1, EntityKind::Cow)).name, None);
    }

    #[test]
    fn an_invisible_entity_and_a_ridden_one_have_no_nametag() {
        // The living renderer's last line drops an invisible entity and a
        // ridden one — an entity the source's `riddenByEntity` names; the
        // store keeps the attachment on the passenger ("what it rides"), so
        // the ridden entity is the attachment's holder. A leash is not a
        // ride.
        let mut cow = Entity::new(1, EntityKind::Cow);
        cow.metadata = metadata(&[(2, MetadataValue::String("Bessie".to_owned()))]);
        let mut pig = Entity::new(4, EntityKind::Pig);
        pig.metadata = metadata(&[(2, MetadataValue::String("Porkchop".to_owned()))]);

        // A leash is not a ride: the leash's anchor keeps its name.
        pig.attachment = Some(Attachment {
            holder: 1,
            leash: true,
        });
        let frames = frames_of(&[cow.clone(), pig.clone()]);
        assert_eq!(
            frame_named(&frames, 1).nametag.as_deref(),
            Some("Bessie"),
            "a leashed cow keeps its name"
        );
        assert_eq!(
            frame_named(&frames, 4).nametag.as_deref(),
            Some("Porkchop"),
            "the leashed pig keeps its name"
        );

        // The pig rides the cow: the cow is the ridden one and drops its
        // name; the pig keeps its own.
        pig.attachment = Some(Attachment {
            holder: 1,
            leash: false,
        });
        let frames = frames_of(&[cow.clone(), pig]);
        assert_eq!(
            frame_named(&frames, 1).nametag,
            None,
            "the ridden cow's name hides"
        );
        assert_eq!(
            frame_named(&frames, 4).nametag.as_deref(),
            Some("Porkchop"),
            "the riding pig keeps its name"
        );

        // An invisible entity loses its name with no attachment in play.
        cow.metadata = metadata(&[
            (2, MetadataValue::String("Bessie".to_owned())),
            (0, MetadataValue::Byte(INVISIBLE_BIT)),
        ]);
        assert_eq!(frame_for(&cow).nametag, None, "nor an invisible one");

        let (player, list) = listed("OxideDev");
        let mut invisible = player.clone();
        invisible.metadata = metadata(&[(0, MetadataValue::Byte(INVISIBLE_BIT))]);
        assert_eq!(
            frame_against(&invisible, &list).nametag,
            None,
            "players follow the same last line"
        );
    }

    #[test]
    fn display_name_prefers_the_display_name() {
        let record = PlayerListRecord {
            name: "OxideDev".to_owned(),
            display_name: Some("§bOx".to_owned()),
            ..PlayerListRecord::default()
        };
        assert_eq!(
            display_name(Some(&record), &Scoreboard::new()).as_deref(),
            Some("§bOx")
        );
        let plain = PlayerListRecord {
            name: "OxideDev".to_owned(),
            ..PlayerListRecord::default()
        };
        assert_eq!(
            display_name(Some(&plain), &Scoreboard::new()).as_deref(),
            Some("OxideDev")
        );
        assert_eq!(display_name(None, &Scoreboard::new()), None);
    }

    #[test]
    fn the_display_name_carries_the_team_clauses() {
        // `ScorePlayerTeam.formatString:95-98` around whichever text the entry
        // resolves to: the name's own team wraps the display name here and
        // adds nothing when the team holds the player under another key.
        let record = PlayerListRecord {
            name: "OxideDev".to_owned(),
            display_name: Some("§bOx".to_owned()),
            ..PlayerListRecord::default()
        };
        let mut board = Scoreboard::new();
        assert_eq!(
            display_name(Some(&record), &board).as_deref(),
            Some("§bOx"),
            "no team yet: the entry's own text"
        );
        board.set_team("red", "Red", "§c[Red] ", "§r", 1, "always", Some(0x0c));
        board.add_team_players("red", &["OxideDev".to_owned()]);
        assert_eq!(
            display_name(Some(&record), &board).as_deref(),
            Some("§c[Red] §bOx§r"),
            "the team's clauses wrap the display name"
        );
    }

    // -----------------------------------------------------------------
    // The boss health pairs.
    // -----------------------------------------------------------------

    #[test]
    fn the_boss_healths_are_the_class_maxima() {
        let dragon = Entity::new(1, EntityKind::EnderDragon);
        assert_eq!(dragon_health(&dragon), Some((200.0, 200.0)));
        let mut hurt = Entity::new(1, EntityKind::EnderDragon);
        hurt.metadata = metadata(&[(6, MetadataValue::Float(150.5))]);
        assert_eq!(dragon_health(&hurt), Some((150.5, 200.0)));

        let wither = Entity::new(2, EntityKind::WitherBoss);
        assert_eq!(dragon_health(&wither), Some((300.0, 300.0)));
        let mut hurt = Entity::new(2, EntityKind::WitherBoss);
        hurt.metadata = metadata(&[(6, MetadataValue::Float(7.0))]);
        assert_eq!(dragon_health(&hurt), Some((7.0, 300.0)));

        // Every other kind answers `None`, its health indexed or not.
        let mut cow = Entity::new(3, EntityKind::Cow);
        cow.metadata = metadata(&[(6, MetadataValue::Float(4.0))]);
        assert_eq!(dragon_health(&cow), None);
        let mut player = Entity::new(4, EntityKind::Player);
        player.metadata = metadata(&[(6, MetadataValue::Float(20.0))]);
        assert_eq!(dragon_health(&player), None);
    }

    /// The health pair one entity's frame carries.
    fn dragon_health(entity: &Entity) -> Option<(f32, f32)> {
        frame_for(entity).health
    }

    // -----------------------------------------------------------------
    // The brightness.
    // -----------------------------------------------------------------

    #[test]
    fn the_brightness_table_is_the_sources_own() {
        assert_eq!(brightness_of_level(0), 0.0);
        assert_eq!(brightness_of_level(15), 1.0);
        assert!(
            (brightness_of_level(12) - 0.5).abs() < 1e-7,
            "level 12 is one half"
        );
        assert!(
            (brightness_of_level(7) - 7.0 / 39.0).abs() < 1e-6,
            "the source's formula at seven"
        );
    }

    #[test]
    fn the_brightness_samples_the_feet_block_and_the_greater_kind() {
        // A scripted world: a stone floor through y 63, one air layer, a stone
        // roof at y 65 with a one-cell hole at (13, 65, 1), and a torch at
        // (3, 64, 1) — all inside the 3x3-column region the engine recomputes
        // around (1, 64, 1). The feet block (1, 64, 1) then holds sky 3 (the
        // hole twelve steps away, one lost per step) and block light 12 (the
        // torch, two away); the same sample at the eye would land above the
        // roof, at sky 15.
        const STONE: u16 = 1 << 4;
        const TORCH: u16 = 50 << 4;
        let mut world = World::new(true);
        for cx in 0..=5 {
            let mut column = ColumnData::empty();
            for section in 0..16 {
                let mut blocks = Box::new([0u16; 4096]);
                for ly in 0..SECTION_SIZE {
                    for lz in 0..SECTION_SIZE {
                        for lx in 0..SECTION_SIZE {
                            let y = section * SECTION_SIZE + ly;
                            let block = if y <= 63 || (y == 65 && !(lx == 13 && lz == 1)) {
                                STONE
                            } else if y == 64 && cx == 0 && lx == 3 && lz == 1 {
                                TORCH
                            } else {
                                0
                            };
                            blocks[block_index(lx, ly, lz)] = block;
                        }
                    }
                }
                column.sections[section] = Some(SectionData {
                    blocks,
                    block_light: Box::new([0; 2048]),
                    sky_light: Some(Box::new([0; 2048])),
                });
                column.mask |= 1u16 << section;
            }
            world.apply_column(cx, 0, &column, true);
        }
        // The engine runs where the referenced light tests run it: over the
        // 3x3 columns around the sample.
        light::recompute(&mut world, 1, 64, 1);

        // The premise, read back through the store: the feet block's own
        // light state.
        assert_eq!(
            (world.sky_light(1, 64, 1), world.block_light(1, 64, 1)),
            (3, 12),
            "the scripted light state"
        );
        // The greater kind wins (12 over the sky's 3) and the sample is the
        // feet block: table[12] = 0.5, while an eye sample above the roof
        // would read table[15] = 1.0 and a sky-only read table[3] = 0.0588…
        assert!(
            (brightness(Some(&world), [1.5, 64.5, 1.5]) - 0.5).abs() < 1e-7,
            "the feet block's brightness"
        );
        // A floor position pushes the sample out of the world's y range and a
        // far position out of its loaded columns: both answer zero, the
        // source's unloaded answer.
        assert_eq!(brightness(Some(&world), [1.5, -0.5, 1.5]), 0.0);
        assert_eq!(brightness(Some(&world), [100.5, 64.5, 1.5]), 0.0);
        // A session with no world has no light at all.
        assert_eq!(brightness(None, [1.5, 64.5, 1.5]), 0.0);
    }

    // -----------------------------------------------------------------
    // The player list.
    // -----------------------------------------------------------------

    #[test]
    fn the_uuid_text_is_hyphenated_like_the_spawns() {
        assert_eq!(
            hyphenated(&[0x06; 16]),
            "06060606-0606-0606-0606-060606060606"
        );
    }

    #[test]
    fn the_list_merges_the_actions() {
        let uuid = [0x11; 16];
        let mut list = PlayerList::new();
        list.insert(
            uuid,
            PlayerListRecord {
                uuid: "11111111-1111-1111-1111-111111111111".to_owned(),
                name: "OxideDev".to_owned(),
                properties: vec![("textures".to_owned(), "abc".to_owned())],
                gamemode: 0,
                latency: 5,
                display_name: None,
            },
        );
        let key = "11111111-1111-1111-1111-111111111111";
        let record = list.get(key).expect("the add landed");
        assert_eq!(record.uuid, key, "the record carries its own uuid");
        assert_eq!(record.name, "OxideDev");
        assert_eq!(
            record.properties,
            vec![("textures".to_owned(), "abc".to_owned())]
        );
        assert_eq!((record.gamemode, record.latency), (0, 5));
        assert!(list.set_gamemode(uuid, 1));
        assert!(list.set_latency(uuid, 42));
        assert!(list.set_display_name(uuid, Some("§bOx".to_owned())));
        let record = list.get(key).expect("still held");
        assert_eq!(
            (
                record.gamemode,
                record.latency,
                record.display_name.as_deref()
            ),
            (1, 42, Some("§bOx"))
        );
        assert_eq!(list.len(), 1);
        assert_eq!(
            list.iter().next().map(|(uuid, _)| uuid),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(list.remove(uuid).expect("removed").name, "OxideDev");
        assert!(list.is_empty());
        // An update naming no record is refused rather than invented.
        assert!(!list.set_latency(uuid, 1));
        assert!(!list.set_gamemode(uuid, 1));
        assert!(!list.set_display_name(uuid, None));
        list.insert(uuid, PlayerListRecord::default());
        list.clear();
        assert!(list.is_empty());
    }
}
