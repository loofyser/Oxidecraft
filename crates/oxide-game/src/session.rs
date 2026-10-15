//! The client session: one framed connection, driven to completion.
//!
//! The session owns what the window cannot: the connection, the version 47
//! world store, and the column meshes (plan Decision 3). It logs in, answers
//! everything a vanilla server expects of a client — the keepalive echo, the
//! teleport echo, Client Settings and the brand plugin message — and reports
//! what the window draws through a [`ClientEvent`] channel. The world clock
//! rides along: every Time Update is reported, and the sky the clock and the
//! view block produce is computed here and reported with it, because the
//! window's own crate may not reach the world store.
//!
//! The session thread is also the tick thread: a fixed
//! 20 Hz step scheduled inside the play loop drives the player state — the
//! window's input, the clock's and the cloud counter's advance, and the
//! per-tick report — while the read loop's discipline holds: ticks are
//! deadline-polled and capped (`Ticker`), and a readable frame is always read
//! before any further tick work. Every tick also sends the packets a server
//! expects from a moving player: the walking report (0x03–0x06) under the
//! source's own rule, the sprint and sneak edges (0x0B), and the ability
//! flips the double-tap flight toggle and a landing produce (0x13).
//!
//! A packet the client has no use for is skipped, and one whose id it cannot
//! even name is not fatal (spec S2), so a proxy or a modded server cannot end
//! the session by sending something unexpected.
//!
//! The column meshes are built off this thread, on a per-session rayon pool:
//! each applied column enters a dirty set with a generation, the idle read
//! copies its snapshot out of the store and hands the build to a worker, and
//! the finished meshes come back as events. The snapshot copies happen only
//! between frames — in the wait for the next one — never while frames are
//! already readable, so a burst of columns on join cannot hold the read loop,
//! and the server closes a session whose keepalive echo goes unanswered for
//! about thirty seconds.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use oxide_assets::atlas::Atlas;
use oxide_proto::codec::CodecError;
use oxide_proto::conn::{Conn, DeadlineStream, RecvOutcome};
use oxide_proto::crypto::{encrypt_rsa, new_secret_key, parse_server_key, server_id_hash};
use oxide_proto::frame::{Compression, FrameError};
use oxide_proto::varint::VarIntError;
use oxide_proto_v47::PacketError;
use oxide_proto_v47::clientbound::{
    self, BlockBreakAnimation, BlockChange, BlockUpdate, ChunkData, EntityStatus, JoinGame,
    KeepAlive, LoginPacket, MapChunkBulk, MultiBlockChange, PlayDisconnect, PlayerAbilities,
    PlayerListItem, PlayerPositionAndLook, PluginMessage, Respawn, TimeUpdate, UpdateHealth,
    read_packet_id,
};
use oxide_proto_v47::effects_wire::{
    ChangeGameState, Particle, SoundEffect, SpawnGlobalEntity, decode_change_game_state,
    decode_particle, decode_sound_effect, decode_spawn_global_entity,
};
use oxide_proto_v47::entity::{
    self, Animation, AttachEntity, CollectItem, DestroyEntities, EntityEquipment, EntityHeadLook,
    EntityLook, EntityLookAndRelativeMove, EntityMetadata, EntityRelativeMove, EntityTeleport,
    EntityVelocity, MetadataItem, MetadataValue, MobType, ObjectType, SpawnMob, SpawnObject,
    SpawnPainting, SpawnPlayer, SpawnXpOrb, UseBed,
};
use oxide_proto_v47::handshake::write_handshake;
use oxide_proto_v47::serverbound::{
    ClientSettings, ClientStatusAction, DiggingStatus, EntityAction, UseEntityAction,
    write_animation, write_click_window, write_client_settings, write_client_status,
    write_close_window, write_confirm_transaction, write_creative_inventory_action,
    write_enchant_item, write_encryption_response, write_entity_action, write_held_item_change,
    write_keep_alive, write_login_start, write_player, write_player_abilities,
    write_player_block_placement, write_player_digging, write_player_look, write_player_position,
    write_player_position_and_look, write_plugin_message, write_update_sign, write_use_entity,
};
use oxide_proto_v47::ui::{
    ChatMessage, ScoreboardDisplay, ScoreboardObjective, ScoreboardScore, ScoreboardTeam,
    TabHeaderFooter, write_chat,
};
use oxide_proto_v47::window::{
    CloseWindow, ConfirmTransaction, EntityEffect, MerchantOffer, MerchantOffers, OpenWindow,
    RemoveEntityEffect, SetExperience, SetSlot, SignEditorOpen, UpdateSign, WindowItems,
    WindowKind, WindowProperty,
};
use oxide_proto_v47::{NEXT_STATE_LOGIN, PROTOCOL};
use oxide_render::terrain::ChunkMesh;
use oxide_world::behaviour::{
    Material, RenderKind, TintKind, behaviour, liquid_height_percent, liquid_kind,
};
use oxide_world::biome::{ColorMap, TintMaps, tint_at_9_biome};
use oxide_world::chunk::SECTION_SIZE;
use oxide_world::entity::{Entities, Entity, EntityKind, KindData};
use oxide_world::light::{self, view_light_level};
use oxide_world::sky::{
    celestial_angle, cloud_colour, moon_phase, sky_colour, star_brightness, sun_brightness,
    sunrise_sunset_colours,
};
use oxide_world::world::World;
use rand::{RngExt, SeedableRng, rngs::StdRng};
use rayon::{ThreadPool, ThreadPoolBuildError, ThreadPoolBuilder};
use tracing::{debug, info, warn};

use crate::audio::SoundCategory;
use crate::audio::sounds::{
    DEATH_SOUND, GUARDIAN_CURSE_PARTICLE_ID, GUARDIAN_CURSE_SOUND, HURT_SOUND, LEVEL_SOUND,
    MobVariant, ORB_SOUND, PICKUP_SOUND, SPLASH_SOUND, SUCCESSFUL_HIT_SOUND, SWIM_SOUND,
    SoundGroup, hurt_pitch, level_sounds, level_volume, mob_sounds, pickup_pitch, splash_volume,
    swim_pitch, swim_volume,
};
use crate::entity_view::{self, EntityFrame, PlayerList, PlayerListRecord, hyphenated};
use crate::input::{InputEvent, Intent, Key, MouseButton, SessionPush, look_delta};
use crate::interaction::{
    Aim, BreakStages, DigAction, DigAim, DigState, camera_clip_distance, creative, hand_rate,
    look_vector, mode_from_value, pick_entity_target, placement, raycast, reach, spectator,
    tool_not_required,
};
use crate::local_particles::{
    FULL_CUBE_BOUNDS, crit_burst, crit_id_for_animation, dig_destroy, dig_hit, dig_tint,
    sprint_spawn, water_spawns,
};
use crate::mesh_queue::{MeshJob, MeshQueue};
use crate::mesher::{
    BlockModelSet, ColumnSnapshot, MeshContext, SmoothLighting, build_column_meshes,
};
use crate::particle_world::SessionParticleWorld;
use crate::particles::{PER_LIST_CAP, ParticleEngine, ParticleFrame};
use crate::physics;
use crate::player::{Abilities, Player};
use crate::scoreboard::{Scoreboard, colour_from_wire};
use crate::settings::{CHAT_MODE_RANGE, ParticleSetting, VIEW_DISTANCE_RANGE};
use crate::ticker::Ticker;
use crate::weather::{WeatherColumn, splash_spawns, weather_columns};
use crate::windows::{CURSOR_WINDOW_ID, HOTBAR_SIZE, OpenWindowState, Windows};
use crate::world_view::WorldView;

/// The plugin channel a vanilla client announces itself on.
const BRAND_CHANNEL: &str = "MC|Brand";

/// The merchant trade list's own custom-payload channel
/// (`NetHandlerPlayClient.handleCustomPayload:1826-1845`).
const TRADE_LIST_CHANNEL: &str = "MC|TrList";

/// The written book's open channel: the server sends an empty `MC|BOpen`
/// buffer (`EntityPlayerMP.displayGUIBook:840-848`) and the client opens
/// its own held stack (`NetHandlerPlayClient.handleCustomPayload:1855-1863`).
const BOOK_OPEN_CHANNEL: &str = "MC|BOpen";

/// The written book's item id, the open guard's identity test
/// (`== Items.written_book`, never `instanceof`, `EntityPlayerMP:844` and
/// `NetHandlerPlayClient:1859`).
const WRITTEN_BOOK_ID: i16 = 387;

/// Clientbound Held Item Change's play id (0x09): one byte, the selected
/// hotbar slot (`S09PacketHeldItemChange.readPacketData:24-27`).
///
/// The protocol crate carries the serverbound writer alone, so the reader
/// lives here beside its caller.
const HELD_ITEM_CHANGE_ID: i32 = 0x09;

/// The own entity's metadata index the air supply rides
/// (`Entity.java:286-287`: index 1 of the own block, a short).
const AIR_METADATA_INDEX: u8 = 1;

/// The own player's metadata index the absorption rides
/// (`EntityPlayer.java:200, :2350-2356`: index 17 of the own block, a float).
const ABSORPTION_METADATA_INDEX: u8 = 17;

/// The own player's metadata index the score rides
/// (`EntityPlayer.getScore:688-691` reads index 18 of the own block, an int).
const SCORE_METADATA_INDEX: u8 = 18;

/// The bed block's id: the camera's sleeping branch reads the block at the
/// entity and applies the facing turn only for this id
/// (`EntityRenderer.java:650-655`).
const BED_BLOCK_ID: u16 = 26;

/// The brand payload, as the capture records the vanilla client sending it: a
/// length-prefixed `vanilla`.
const BRAND_PAYLOAD: &[u8] = b"\x07vanilla";

/// How long the play loop waits for a frame to start before it tends the mesh
/// queue.
///
/// The wait is a deadline, not a blocking read: it consumes nothing, so a
/// keepalive behind a column burst is still read — and echoed — as soon as its
/// bytes are readable. One mesh batch is bounded by this wait plus the pump
/// itself, which is what keeps the echo well inside the server's timeout.
const MESH_TICK: Duration = Duration::from_millis(20);

/// The session's tick period: twenty ticks per second.
///
/// The source runs its game ticks from `new Timer(20.0F)`
/// (`Minecraft.java:223`), one tick every 50 ms of real time while the window
/// keeps up; the session schedules the same period.
const TICK_PERIOD: Duration = Duration::from_millis(50);

/// The local sounds' dice seed: fixed, so a replayed stream stages the same
/// pitches. Only the staging formulas carry parity, never the draws.
const SOUND_DICE_SEED: u64 = 9;

/// The particle pool's dice seed: fixed, so a replayed stream spawns the
/// same wire fans and splash columns. Only the spawn formulas carry parity,
/// never the draws.
const PARTICLE_DICE_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// The snow layer's block id: the sheet that lends its own step when it sits
/// on the stood-on block (`Entity.playStepSound:988-993`).
const SNOW_LAYER_ID: u16 = 78;

/// How many of the window's input events one pass drains.
///
/// The bound has no source counterpart: the source's input arrives from the
/// window it already owns. The backlog drains across passes and nothing is
/// dropped; the bound only keeps one pass' work finite when the window
/// outruns the session.
const INPUTS_PER_PASS: usize = 32;

/// The mouse sensitivity the look mapping runs with until the client pushes
/// the store's value over [`InputEvent::PushSettings`].
///
/// `GameSettings.mouseSensitivity`'s own default, 0.5
/// (`GameSettings.java:65`).
const MOUSE_SENSITIVITY: f32 = 0.5;

/// The pitch clamp, in degrees: `MathHelper.clamp_float(this.rotationPitch,
/// -90.0F, 90.0F)` (`Entity.setAngles`, `Entity.java:395`).
const PITCH_LIMIT: f32 = 90.0;

/// The squared-distance threshold above which one tick reports its position:
/// `d0 * d0 + d1 * d1 + d2 * d2 > 9.0E-4D`
/// (`EntityPlayerSP.onUpdateWalkingPlayer`, `EntityPlayerSP.java:230`).
const POSITION_EPSILON: f64 = 9.0e-4;

/// Ticks after which a quiet player still reports its position:
/// `this.positionUpdateTicks >= 20` (`EntityPlayerSP.java:230`).
const POSITION_STALE_TICKS: i32 = 20;

/// How many queued columns one pump hands to the pool.
///
/// The snapshot copy happens on the session's thread, so the cap is what
/// bounds this thread's own work in one idle wait: at most this many copies
/// per pump, whatever the burst behind them. The pump runs only when no frame
/// has been readable for a whole [`MESH_TICK`], so those copies never sit
/// between two frames that were already waiting.
const PENDING_JOBS_CAP: usize = 4;

/// How long the end-of-session drain waits for the pool's outstanding jobs.
///
/// A clean stop reports its last meshes. The drain hands the queue's pending
/// columns to the pool too, one snapshot copy each on this thread, so the
/// bound covers the copies as well as the builds: a build is pure CPU work
/// with no IO to wait on, and under load — other sessions' pools, or another
/// test in the same suite — an outstanding build can take far longer than it
/// does on an idle core, while a column's copy alone runs to tens of
/// milliseconds in a debug build. It is still a bound, so a wedged build
/// cannot hold the session's end forever. The pool is dropped when the session
/// returns.
const END_OF_SESSION_WAIT: Duration = Duration::from_millis(5000);

/// The login-state packet ids the client decodes. Anything else is skipped
/// rather than refused, so the login survives a packet M1 does not know.
const LOGIN_PACKET_IDS: [i32; 4] = [0x00, 0x01, 0x02, 0x03];

/// The play-state Set Compression id. The reference records it as broken in
/// 1.8 and says not to use it, so the session logs it and changes nothing.
const PLAY_SET_COMPRESSION_ID: i32 = 0x46;

/// The seam an online-mode login posts its session join through.
///
/// The error side is deliberately a plain message: it is shown verbatim on
/// the disconnected screen, and the classification lives in the production
/// implementation (Task 9 maps the typed auth errors to it).
pub trait JoinServer: Send + Sync {
    /// Posts the session join: the profile UUID (32 hex digits), the account's
    /// access token, and the server-id hash the login computed.
    fn join(&self, uuid_hex: &str, access_token: &str, server_id: &str) -> Result<(), String>;
}

/// The signed-in account an online-mode login runs with.
///
/// The profile carries Login Start's name, and the joiner posts the session
/// join the encrypted flow computes the hash for. The `Debug` names the uuid
/// and holds a placeholder for the joiner — never the token.
pub struct SessionAccount {
    /// The profile UUID, 32 hex digits without dashes.
    pub uuid_hex: String,
    /// The account's access token, for the session join. Never logged.
    pub access_token: String,
    /// The seam the join is posted through.
    pub joiner: Arc<dyn JoinServer>,
}

impl std::fmt::Debug for SessionAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionAccount")
            .field("uuid_hex", &self.uuid_hex)
            .field("access_token", &"<redacted>")
            .field("joiner", &"<joiner>")
            .finish()
    }
}

/// What the session needs to connect and behave like a vanilla client.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Server host.
    pub host: String,
    /// Server port.
    pub port: u16,
    /// The name Login Start sends — the signed-in profile's name when an
    /// account is present, else the offline name.
    pub username: String,
    /// The signed-in account an online-mode login runs with: its profile
    /// carries Login Start's name, and its joiner posts the session join.
    /// `None` stays the offline default — a server that then asks for
    /// encryption is refused with [`SessionError::EncryptionRequired`].
    pub account: Option<Arc<SessionAccount>>,
    /// The settings sent in Client Settings.
    pub settings: ClientSettings,
    /// The assets the column meshes are built from.
    ///
    /// `None` — and a session whose client has no store — meshes every block as
    /// the atlas's fallback sprite; the session says so once, and the replay
    /// tests pass `None`.
    pub mesh: Option<Arc<MeshAssets>>,
}

/// The assets a session meshes with: the model join, the atlas and the colour
/// maps.
///
/// The client loads these once from the extraction tree; the session only reads
/// them.
#[derive(Debug)]
pub struct MeshAssets {
    /// The block states' baked models.
    pub models: BlockModelSet,
    /// The stitched atlas.
    pub atlas: Atlas,
    /// The colour maps the biome tints read.
    pub tint_maps: TintMaps,
}

impl MeshAssets {
    /// The asset-less stand-in: no models, the atlas's own fallback sprite, and
    /// neutral white colour maps.
    ///
    /// Every block draws the fallback sprite, which is what a store-less run
    /// gets. [`Session::new`] logs it once.
    pub fn fallback() -> MeshAssets {
        /// A 256 x 256 RGBA colour map of one value.
        fn neutral() -> ColorMap {
            ColorMap::from_rgba(&[255u8; 256 * 256 * 4])
                .expect("an all-white 256x256 map is a valid colour map")
        }
        MeshAssets {
            models: BlockModelSet::empty(),
            atlas: Atlas::fallback(),
            tint_maps: TintMaps {
                grass: neutral(),
                foliage: neutral(),
            },
        }
    }
}

/// The graphics settings the session meshes with until the client owns them.
///
/// M2 renders Fast graphics only — the plan's first known limit — so the leaf
/// rule Task 9 adds reads `true` here.
const GRAPHICS_FAST: bool = true;

/// The smooth-lighting setting: `GameSettings.ambientOcclusion`'s own default,
/// 2 (`client/settings/GameSettings.java:78`).
const SMOOTH_LIGHTING: SmoothLighting = SmoothLighting::Maximum;

/// Maps a stored smooth-lighting setting to the mesher's regime: 0 is Off,
/// 1 Minimum, 2 Maximum (`AMBIENT_OCCLUSIONS`, `GameSettings.java:59`);
/// past-the-end values clamp to Maximum, the renderer's own
/// [`oxide_render::renderer::Renderer::set_smooth_lighting`] rule.
fn smooth_lighting_of(mode: u8) -> SmoothLighting {
    match mode {
        0 => SmoothLighting::Off,
        1 => SmoothLighting::Minimum,
        _ => SmoothLighting::Maximum,
    }
}

/// Applies one settings push to the session-held state: the wire fields,
/// the mesh settings, the look sensitivity and the live particle regime.
///
/// The wire payload is clamped into the store's ranges and compared whole:
/// a changed payload returns true and the caller re-sends C15 with every
/// field current, an identical one returns false and nothing is sent. The
/// mesh settings, the sensitivity and the particle regime apply regardless.
fn apply_session_push(
    settings: &mut ClientSettings,
    graphics_fast: &mut bool,
    smooth_lighting: &mut SmoothLighting,
    sensitivity: &mut f32,
    invert_mouse: &mut bool,
    particles: &mut ParticleEngine,
    push: &SessionPush,
) -> bool {
    let mut next = push.settings.clone();
    next.view_distance = next
        .view_distance
        .clamp(VIEW_DISTANCE_RANGE.0, VIEW_DISTANCE_RANGE.1);
    next.chat_mode = next.chat_mode.min(CHAT_MODE_RANGE.1);
    *graphics_fast = push.graphics_fast;
    *smooth_lighting = smooth_lighting_of(push.smooth_lighting);
    *sensitivity = push.sensitivity.clamp(0.0, 1.0);
    *invert_mouse = push.invert_mouse;
    particles.set_limit(push.particles);
    if next == *settings {
        return false;
    }
    *settings = next;
    true
}

/// The context a column build reads: the session's assets and its held
/// graphics settings.
///
/// The settings ride the session's own state: they start at the two defaults
/// above, and the client's settings pushes move them from there.
fn mesh_context(
    assets: &MeshAssets,
    graphics_fast: bool,
    smooth_lighting: SmoothLighting,
) -> MeshContext<'_> {
    MeshContext {
        models: &assets.models,
        atlas: &assets.atlas,
        tint_maps: &assets.tint_maps,
        graphics_fast,
        smooth_lighting,
    }
}

/// The per-tick easing rate of the weather strengths.
///
/// `World.updateWeather` moves each strength toward its rainy or thundery
/// side by `0.01` a tick — `(float)((double)strength ± 0.01D)` — and clamps
/// the result to `[0, 1]` (`World.java:2541-2553` for thunder, `:2576-2587`
/// for rain). The source eases toward its raining/thundering flags; the
/// session's targets stand in for those flags per the task's interface, so
/// reasons 7/8 name fade targets where the source snaps the strength.
pub const WEATHER_EASE_RATE: f32 = 0.01;

/// The session's weather: the eased strengths the window reads and the
/// targets the wire sets.
///
/// Clientbound 0x2B's weather reasons write the targets — 1 begins the rain
/// (1.0), 2 ends it (0.0), 7 fades the rain to its value, 8 fades the
/// thunder to its value
/// (`NetHandlerPlayClient.handleChangeGameState:1371-1421`) — and [`Self::tick`]
/// eases the strengths toward them once per tick. The window reads the eased
/// pair through [`ClientEvent::WeatherChanged`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeatherState {
    /// The eased rain strength, in `[0, 1]`.
    pub rain: f32,
    /// The eased thunder strength, in `[0, 1]`.
    pub thunder: f32,
    /// The rain target the wire last named, in `[0, 1]`.
    pub target_rain: f32,
    /// The thunder target the wire last named, in `[0, 1]`.
    pub target_thunder: f32,
}

impl WeatherState {
    /// One tick of easing: each strength moves toward its target by
    /// [`WEATHER_EASE_RATE`], snapping when it arrives, and the result stays
    /// clamped to `[0, 1]`.
    ///
    /// Returns whether either strength moved: the session reports
    /// [`ClientEvent::WeatherChanged`] exactly on the ticks that moved.
    pub fn tick(&mut self) -> bool {
        // One strength's step: within a rate step of the target it arrives
        // instead of stepping, else it moves one rate toward it, and the
        // result stays in `[0, 1]`. The arrival bound carries a thousandth of
        // a step of slop so the float dust a full 0-to-1 ease accumulates
        // (~7e-7 observed) cannot push an arrival one tick late; the snapped
        // value is exact either way.
        fn ease(current: f32, target: f32) -> (f32, bool) {
            let arrived = (target - current).abs() <= WEATHER_EASE_RATE * 1.001;
            let next = if arrived {
                target
            } else if target > current {
                current + WEATHER_EASE_RATE
            } else {
                current - WEATHER_EASE_RATE
            };
            let next = next.clamp(0.0, 1.0);
            let moved = next != current;
            (next, moved)
        }
        let (rain, rain_moved) = ease(self.rain, self.target_rain);
        let (thunder, thunder_moved) = ease(self.thunder, self.target_thunder);
        self.rain = rain;
        self.thunder = thunder;
        rain_moved || thunder_moved
    }
}

/// Everything the session reports to the window.
#[derive(Debug, Clone, PartialEq)]
// The scoreboard report carries the source's own board — its nineteen-slot
// display table included (`Scoreboard.java:20`) — one rare event per change;
// boxing the payload would reshape the event's consumers for no measured gain.
#[allow(clippy::large_enum_variant)]
pub enum ClientEvent {
    /// Login succeeded.
    LoggedIn {
        /// The account UUID, hyphenated, as the server sent it.
        uuid: String,
        /// The account name.
        username: String,
    },
    /// Join Game arrived: the world exists and settings have been sent.
    Joined {
        /// The player's entity id.
        entity_id: i32,
        /// Gamemode; the 0x08 bit means hardcore.
        gamemode: u8,
        /// Dimension: -1 nether, 0 overworld, 1 end.
        dimension: i8,
        /// Difficulty.
        difficulty: u8,
        /// Maximum player count the server advertises.
        max_players: u8,
        /// Level type, for example `default`.
        level_type: String,
        /// Whether Join Game asks for reduced debug info: the second half of
        /// the source's `isReducedDebug` (`GuiOverlayDebug.java`:56-59), the
        /// first being the settings flag the window reads itself.
        reduced_debug_info: bool,
    },
    /// The first Player Position And Look landed after the join or a
    /// world-rebuilding respawn: the terrain screen's clear signal.
    ///
    /// Reported once per arming — the join arms it, a cross-dimension
    /// respawn re-arms it — when the S08 the source clears
    /// `GuiDownloadTerrain` on arrives
    /// (`NetHandlerPlayClient.handlePlayerPosLook:719-726` flips
    /// `doneLoadingTerrain` false→true beside `displayGuiScreen(null)`).
    /// Later S08 packets are ordinary corrections and report nothing of
    /// this kind.
    TerrainReady,
    /// The session ticked the player: the state one 20 Hz step leaves.
    ///
    /// Reported every tick, and immediately — with `snapped: true` — when a
    /// server correction (clientbound 0x08) moved the player: a snapped tick
    /// resets the window's interpolation rather than sliding to the pose. The
    /// `tick` count is the session's own, gap-free, and it is what the cloud
    /// offset advances from (the source's `cloudTickCounter` advances once
    /// per tick, `RenderGlobal.updateClouds`, `RenderGlobal.java:1138-1146`).
    PlayerTick {
        /// Absolute x of the feet.
        x: f64,
        /// Absolute y of the feet.
        y: f64,
        /// Absolute z of the feet.
        z: f64,
        /// Absolute yaw in degrees.
        yaw: f32,
        /// Absolute pitch in degrees.
        pitch: f32,
        /// Whether the player stands on something.
        on_ground: bool,
        /// Whether the player is sprinting.
        sprinting: bool,
        /// Whether the player is sneaking.
        sneaking: bool,
        /// Whether the player is flying.
        flying: bool,
        /// Whether the player is in water.
        in_water: bool,
        /// The tick this state belongs to.
        tick: u64,
        /// Whether a server correction produced this tick.
        snapped: bool,
        /// Ticks left of the hurt flash (`hurtTime`), for the camera's roll.
        hurt_time: u32,
        /// The yaw the last hurt came from (`attackedAtYaw`); zero until M4
        /// tracks attackers.
        attacked_at_yaw: f32,
    },
    /// The tracked entities, once per tick.
    ///
    /// Reported immediately behind the tick's `PlayerTick`; a tick that
    /// changed nothing is still reported — the window's entity interpolation
    /// needs the cadence, and change detection is `PlayerTick`'s own rule.
    /// The frames come in ascending entity id, each carrying the pose pair
    /// the interpolation slides between.
    EntitiesTick {
        /// One frame per tracked entity.
        entities: Vec<EntityFrame>,
    },
    /// The eye-block read for one tick: the viewpoint cell, the portal
    /// overlap and the block-in-hand probe's winner, from the world data the
    /// session owns.
    ///
    /// Reported every tick immediately behind the tick's `EntitiesTick` — a
    /// tick that changed nothing is still reported, like that feed — and on
    /// the snapped correction's pair too. The window keeps the values for
    /// the overlay draws, the portal clock and the lava fog arm; it derives
    /// no world reads of its own.
    EyeBlock {
        /// The packed eye-viewpoint block (`World::block` packing, the id in
        /// the high bits): `ActiveRenderInfo.getBlockAtEntityViewpoint`'s
        /// cell read as data (`ActiveRenderInfo.java:84-107`).
        eye: u16,
        /// Whether the tick's box overlaps a portal block: the collision's
        /// `setPortal` read as a bool (`BlockPortal.java:197-204`).
        in_portal: bool,
        /// The block-in-hand probe's winning packed state, or `None` when the
        /// gate is shut or its render type is `-1`
        /// (`ItemRenderer.java:424-447`).
        inside: Option<u16>,
        /// Whether the feet block's column is loaded: the source's
        /// `isBlockLoaded` gate for the `Biome:`/`Light:`/`Local Difficulty:`
        /// tail (`GuiOverlayDebug.java`:136).
        block_loaded: bool,
        /// The feet column's biome id; 0 (ocean) when the column is missing.
        biome_id: u8,
        /// The feet block's sky-light nibble; 0 when the column is missing.
        sky_light: u8,
        /// The feet block's block-light nibble; 0 when the column is missing.
        block_light: u8,
    },
    /// The player list's whole entry set, from clientbound 0x38.
    ///
    /// Reported once per change — an add, an update of either kind or a
    /// remove — and not at all for a packet that leaves the list as it was;
    /// the records come in ascending uuid order, the list's own order.
    PlayerList {
        /// The records, ascending uuid.
        entries: Vec<PlayerListRecord>,
    },
    /// The scoreboard's whole state, from clientbound 0x3B–0x3E.
    ///
    /// Reported once per change to any part of it — an objective, a score, a
    /// display slot, a team or a membership — and not at all for a packet
    /// that writes what the board already holds. The whole of it travels —
    /// the source's own nineteen-slot display table included
    /// (`Scoreboard.java:20`) — and the window keeps no merge logic.
    ScoreboardChanged {
        /// The state as it stands.
        board: Scoreboard,
    },
    /// The tab list's header and footer text, from clientbound 0x47.
    ///
    /// The two strings travel as sent — chat JSON — and the pair is reported
    /// when it changed.
    TabText {
        /// The header as sent.
        header: String,
        /// The footer as sent.
        footer: String,
    },
    /// A chat message, from clientbound 0x02.
    ///
    /// Reported for every message; the text travels as sent — chat JSON —
    /// and the position names where it shows.
    Chat {
        /// The chat component as JSON, exactly as sent.
        text: String,
        /// Where the message shows: 0 the chat box, 1 the system line, 2
        /// above the hotbar.
        position: i8,
    },
    /// The player's health, food and saturation, from clientbound 0x06.
    ///
    /// Reported for every 0x06. Health at or below zero is the death the
    /// `Died` that follows reports.
    Health {
        /// The health the packet carried.
        health: f32,
        /// The food level the packet carried.
        food: i32,
        /// The food saturation the packet carried.
        saturation: f32,
    },
    /// The player died: a 0x06 left the health at or below zero.
    ///
    /// Reported once per death, when the state is entered. The window shows
    /// the interim death view on it; a click or Space then asks the session
    /// for the respawn, which the server answers with clientbound 0x07.
    Died,
    /// The server respawned the player (clientbound 0x07).
    ///
    /// The world was kept when the packet's dimension matched the one the
    /// session held and rebuilt otherwise, in which case a `WorldCleared`
    /// precedes this event. The window clears the death view on it and reads
    /// the dimension for its fog.
    Respawned {
        /// The dimension respawned into: -1 nether, 0 overworld, 1 end.
        dimension: i8,
        /// The gamemode, as Join Game carries it.
        gamemode: u8,
    },
    /// The server changed the game mode mid-session (clientbound 0x2B
    /// reason 3).
    ///
    /// Reported on every game-state packet naming reason 3, carrying the
    /// byte the floored value names
    /// (`NetHandlerPlayClient.handleChangeGameState:1364`, `:1381-1384`).
    /// The window folds its spectator read from it at once; the creative
    /// redirect keeps its join-only fold (recorded).
    GameType {
        /// Gamemode; the 0x08 bit means hardcore.
        gamemode: u8,
    },
    /// The own player fell asleep in a bed, from clientbound 0x0A.
    ///
    /// Reported once per sleep — a repeated bed packet for the already
    /// sleeping player answers nothing — when the S0A naming the own
    /// player arrives (`NetHandlerPlayClient.handleUseBed:903-907` runs
    /// `trySleep`, which sets `sleeping` with `sleepTimer = 0`,
    /// `EntityPlayer.java:1568-1571`). The bed's facing rides along for the
    /// camera's rise; a bed for another player answers nothing.
    Sleeping {
        /// The bed head's world x.
        x: i32,
        /// The bed head's world y.
        y: i32,
        /// The bed head's world z.
        z: i32,
        /// The bed's facing quarter-turn, `meta & 3` at the head (south 0,
        /// west 1, north 2, east 3) — south when the map holds no bed there.
        facing: u8,
    },
    /// The own player woke, from clientbound 0x0B's animation 2.
    ///
    /// Reported once, when the wake animation names the sleeping own
    /// player (`NetHandlerPlayClient.handleAnimation:882-885`); a wake
    /// with no sleep behind it answers nothing. The window's sleep timer
    /// fades 100→110 on it.
    Woke,
    /// The aimed block changed.
    ///
    /// The session recomputes the aim after look input and on every tick —
    /// from the pose eye, along the look vector, for the gamemode's reach —
    /// and reports only a change, `None` included: a look that leaves every
    /// block clears the aim. The window's outline and crack passes draw from
    /// it, and the click paths act on it.
    Aim {
        /// The block the interaction ray meets, or `None` when it meets none.
        aim: Option<Aim>,
        /// The packed block at the aimed cell (`World::block` packing), or
        /// `None` when the ray meets none. The debug overlay's block section
        /// reads the registry name and the state properties from it.
        block: Option<u16>,
    },
    /// The third-person occlusion moved: the tick's corner rays from the
    /// pose eye toward the wanted camera point shortened (or cleared) the
    /// camera's distance (`EntityRenderer.java`:684-702).
    ///
    /// Reported only while a third-person mode holds, and only on a change;
    /// leaving third person resets the window's read to the full distance
    /// with no event. The window feeds the frame's camera from it.
    CameraClip {
        /// The clipped distance in blocks.
        distance: f32,
    },
    /// A destroy stage landed on a block.
    ///
    /// Reported when the stage map's entry for the breaker changes — the
    /// session's own digging writes it, and decoded clientbound 0x25 writes
    /// it — with the breaker's entity id and the stage the map holds, `0..=9`
    /// (the values a set can carry: `RenderGlobal.sendBlockBreakProgress`'s
    /// `progress >= 0 && progress < 10`, `RenderGlobal.java`:2364).
    /// The window's crack overlay draws from it.
    BreakStage {
        /// The breaking player's entity id.
        breaker: i32,
        /// The block's x.
        x: i32,
        /// The block's y.
        y: i32,
        /// The block's z.
        z: i32,
        /// The stage the map now holds, 0..=9.
        stage: u8,
    },
    /// A destroy stage left a block.
    ///
    /// Reported when the stage map removes an entry: a dig stopped or
    /// completed, a 0x25 carried a stage outside `0..=9`
    /// (`RenderGlobal.sendBlockBreakProgress:2377-2380`), or the entry
    /// expired in the sweep (`cleanupDamagedBlocks:1131`).
    BreakCleared {
        /// The breaking player's entity id.
        breaker: i32,
        /// The block's x.
        x: i32,
        /// The block's y.
        y: i32,
        /// The block's z.
        z: i32,
    },
    /// Time Update arrived: the world's clock.
    ///
    /// The time of day is the value the celestial angle and the moon phase are read from, after
    /// the receive rule's negation of the wire's frozen-sun sign
    /// (`S03PacketTimeUpdate.java:17-31`, `WorldClient.java:468-481`): a stopped cycle's frame
    /// reports the positive time its sky renders from — the wire's `-6000` is noon. The age is
    /// carried for later work.
    Time {
        /// The world's age in ticks.
        world_age: i64,
        /// The world's time of day in ticks, after the receive rule's negation.
        time_of_day: i64,
    },
    /// The sky the session's world and view block produce, from the last clock.
    ///
    /// The session computes these because its client may not: `oxide-client` has no edge to
    /// `oxide-world`, and `oxide-render` may not reach it at all, so the world-derived values
    /// travel with the event. `partial_ticks` is zero (the session reports whole ticks;
    /// interpolation is the renderer's) and the strengths are the session's live eased weather.
    Sky {
        /// The celestial angle in `0..1`, from the world time.
        celestial_angle: f32,
        /// The sky's colour at the view block's biome.
        colour: [f32; 3],
        /// The sun's brightness.
        sun_brightness: f32,
        /// The stars' brightness before the rain dim; the sky pass applies `1 - rain`.
        star_brightness: f32,
        /// The clouds' tint.
        cloud_colour: [f32; 3],
        /// The moon's phase in `0..8`, from the world time.
        moon_phase: u8,
        /// The light level at the view block, 0..15: the fog colour's brightness factor reads
        /// it (`EntityRenderer.java:362`), and the client's renderer builds the table
        /// (`oxide-world`'s `light::view_light_level`).
        light_level: u8,
        /// The sunset disc's `[r, g, b, alpha]` quad, or `None` outside the band
        /// (`WorldProvider.calcSunriseSunsetColors`, `WorldProvider.java:151-175`).
        horizon: Option<[f32; 4]>,
    },
    /// The world was rebuilt: every column the window holds is stale.
    ///
    /// Emitted when a respawn crossed dimensions and the session built a new
    /// world for it, before the `Respawned` that follows. The window drops
    /// its whole chunk store on it; a same-dimension respawn keeps the world
    /// and emits nothing of this kind.
    WorldCleared,
    /// A column's meshes were (re)built; `None` means the section now draws
    /// nothing.
    ChunkUpdated {
        /// The column's chunk x.
        cx: i32,
        /// The column's chunk z.
        cz: i32,
        /// All sixteen sections, by section index.
        sections: Vec<(usize, Option<ChunkMesh>)>,
    },
    /// A column was unloaded by the server.
    ChunkUnloaded {
        /// The column's chunk x.
        cx: i32,
        /// The column's chunk z.
        cz: i32,
    },
    /// A keepalive was answered.
    KeepAlive {
        /// The id that was echoed.
        id: i32,
    },
    /// The server closed the session, with its reason.
    Disconnected {
        /// The kick reason as chat JSON, as the server sent it.
        reason: String,
    },
    /// A window the server opened, from clientbound 0x2D.
    ///
    /// Window 0 is the player's own and is never announced — the state holds
    /// it from Join Game — and the server opens at most one further window at
    /// a time (`NetHandlerPlayClient.handleOpenWindow:1092-1120`). The title
    /// travels as sent — chat JSON — and the kind names the screen the window
    /// belongs to. The slot count travels too: the container-side size the
    /// screen's layout pick reads (`S2DPacketOpenWindow.readPacketData:51-61`
    /// carries the window id, the type string, the title and the slot count).
    WindowOpened {
        /// The window id the server assigned.
        window_id: u8,
        /// The window's kind, from its type string.
        kind: WindowKind,
        /// The title as chat JSON, exactly as sent.
        title: String,
        /// The window's container-side slot count, exactly as sent: the
        /// chest's row pick reads `slot count / 9`.
        slot_count: u8,
        /// The horse's entity id, carried only by a horse window
        /// (`S2DPacketOpenWindow.readPacketData:51-61` reads it when the type
        /// is `EntityHorse`): the horse screen resolves the live horse for
        /// its preview and its chested/armour flags from it, while the title
        /// stays the packet string (`handleOpenWindow:1092-1120`).
        entity_id: Option<i32>,
    },
    /// A window left the state: the server closed it (clientbound 0x2E) or the
    /// view closed its own screen and the session's close put the window away.
    ///
    /// Reported for the live open window alone, which the close cleared — the
    /// carried stack dropped with it
    /// (`handleCloseWindow:1311-1315` reaching
    /// `EntityPlayerSP.closeScreenAndDropStack:336-341`) — never for window 0,
    /// the player's own.
    WindowClosed {
        /// The window id the close named.
        window_id: u8,
    },
    /// One window's state after a change.
    ///
    /// Built per change — a whole-set write, a slot write, a property write,
    /// the hotbar pop's tick — and never per frame: a write that lands
    /// nowhere reports nothing. Window 0's snapshot projects the crafting
    /// slots and the inventory into the container's 45-slot layout
    /// (`ContainerPlayer.java:36-67`), and an opened window projects its own
    /// slot vector. `hotbar_pop` is the source's `animationsToGo`, which the
    /// HUD's pop math reads (`GuiIngame.java:1043-1053`).
    WindowSnapshot {
        /// The window the snapshot belongs to.
        window_id: u8,
        /// The window's slots, window 0's 45-slot layout included.
        slots: Vec<Option<MetadataItem>>,
        /// The carried stack (`InventoryPlayer.getItemStack`), which rides
        /// every window.
        cursor: Option<MetadataItem>,
        /// The window's properties by index: a furnace's burn and cook state,
        /// an enchanting table's levels.
        properties: Vec<i16>,
        /// The hotbar's pop, one counter per slot, window 0's own.
        hotbar_pop: [u8; 9],
    },
    /// The selected hotbar slot, from clientbound 0x09.
    ///
    /// The session sets the inventory's own selection and reports the slot; a
    /// slot outside the hotbar's nine is ignored
    /// (`NetHandlerPlayClient.handleHeldItemChange:598-606`).
    HeldItemSlot {
        /// The hotbar slot, 0 through 8.
        slot: i16,
    },
    /// The player's experience, from clientbound 0x1F.
    Experience {
        /// The bar's fill, 0 through 1.
        bar: f32,
        /// The player's level.
        level: i32,
        /// The player's total experience.
        total: i32,
    },
    /// The own player's air supply, from clientbound 0x1C's metadata index 1.
    ///
    /// The air rides the own entity's metadata block (`Entity.java:286-287`,
    /// index 1) and arrives as the source's own short.
    Air {
        /// The air in ticks; 300 is a full breath.
        air: i16,
    },
    /// The own player's absorption, from clientbound 0x1C's metadata index 17.
    ///
    /// The absorption rides the own player's metadata block as a float
    /// (`EntityPlayer.getAbsorptionAmount` reads DataWatcher 17,
    /// `EntityPlayer.java:2353-2356`, registered in `entityInit` at :200) and
    /// feeds the hearts row's yellow overlay directly.
    Absorption {
        /// The absorption in half-hearts, as the server sent it.
        amount: f32,
    },
    /// The own player's score, from clientbound 0x1C's metadata index 18.
    ///
    /// The score rides the own player's metadata block as an int
    /// (`EntityPlayer.getScore` reads DataWatcher 18,
    /// `EntityPlayer.java:688-691`) and feeds the death screen's score line
    /// (`GuiGameOver.drawScreen:120`); the window keeps the last one it saw.
    Score {
        /// The score, as the server sent it.
        score: i32,
    },
    /// The own player's live effects, after an addition or a removal
    /// (clientbound 0x1D and 0x1E).
    ///
    /// The store holds the own player's effects alone, ascending by effect
    /// id; a later effect on a held id combines into the one held, the
    /// source's own rule (`PotionEffect.combine:63-85`).
    Effects {
        /// Every effect the player holds, ascending by id.
        effects: Vec<StatusEffect>,
    },
    /// The server opened a sign's editor, from clientbound 0x36.
    SignEditorOpen {
        /// The sign's world x.
        x: i32,
        /// The sign's world y.
        y: i32,
        /// The sign's world z.
        z: i32,
    },
    /// A sign's text arrived, from clientbound 0x33.
    ///
    /// The lines travel as sent — chat JSON — and the session's sign map holds
    /// the last text per position for the view that draws the sign. The
    /// block the store holds at the position rides along, so the draw knows
    /// the board's variant and orientation without a block query of its own.
    SignTextChanged {
        /// The sign's world x.
        x: i32,
        /// The sign's world y.
        y: i32,
        /// The sign's world z.
        z: i32,
        /// The four lines, as sent.
        lines: [String; 4],
        /// The block id the store holds at the position (`63` standing,
        /// `68` wall; anything else — usually air — when the text arrived
        /// before its block).
        block_id: u16,
        /// The metadata nibble the store holds at the position: the
        /// standing sign's rotation 0..15, the wall sign's facing.
        metadata: u8,
    },
    /// A mapped sign's block stopped being a sign, from a block change, a
    /// chunk load or an unload.
    ///
    /// A port rule (recorded): the source has no such event — its tile
    /// entity dies with the block — but the port's view keeps its own copy
    /// of the map and must drop the entry, or the text would float where
    /// no board stands.
    SignTextCleared {
        /// The sign's world x.
        x: i32,
        /// The sign's world y.
        y: i32,
        /// The sign's world z.
        z: i32,
    },
    /// A merchant's trade list, from the `MC|TrList` custom payload.
    ///
    /// Read through the payload's own decoder
    /// (`NetHandlerPlayClient.handleCustomPayload:1826-1845` reaching
    /// `MerchantRecipeList.readFromBuf:76-106`); the merchant screen that
    /// shows it is a later task's.
    MerchantOffers {
        /// The offers, in wire order.
        offers: Vec<MerchantOffer>,
    },
    /// A written book's open, from the `MC|BOpen` custom payload.
    ///
    /// The payload itself is empty
    /// (`EntityPlayerMP.displayGUIBook:840-848` sends an empty buffer); the
    /// stack is the session's held-slot snapshot at receive time — the own
    /// player's currently held item
    /// (`NetHandlerPlayClient.handleCustomPayload:1855-1863` opens
    /// `getCurrentEquippedItem`). A payload arriving with no held written
    /// book opens nothing and reports nothing.
    BookOpen {
        /// The held stack, as the inventory held it at receive time.
        stack: MetadataItem,
    },
    /// The eased weather strengths changed on a tick, from clientbound 0x2B's
    /// weather reasons.
    ///
    /// Reported once per tick while the easing moves — never for a settled
    /// tick. The window's sky and weather draws read the pair from here.
    WeatherChanged {
        /// The eased rain strength, in `[0, 1]`.
        rain: f32,
        /// The eased thunder strength, in `[0, 1]`.
        thunder: f32,
    },
    /// The rain/snow columns around the viewer for one tick: what the
    /// window's weather pass draws.
    ///
    /// Reported once per tick while it rains, plus the first clear tick
    /// after — never for a settled clear sky. The window keeps the last
    /// report and feeds it to the pass every frame; the tick scrolls the
    /// pass's sheets.
    WeatherColumns {
        /// The tick's columns, in draw order.
        columns: Vec<WeatherColumn>,
        /// The session tick the columns rode in on.
        tick: u32,
    },
    /// A sound effect played, from clientbound 0x29.
    ///
    /// Reported once per packet; the audio engine resolves the name. The
    /// position rides the wire's own 1/8-block ints
    /// (`S29PacketSoundEffect.readPacketData`); divide by 8 for blocks
    /// ([`oxide_proto_v47::effects_wire::read_effect_position`]). The pitch
    /// is the wire's own byte, 63 for full pitch
    /// ([`oxide_proto_v47::effects_wire::read_sound_pitch`]).
    SoundEffect {
        /// The sound to play, exactly as sent.
        name: String,
        /// The position in 1/8-block units: x, y, z.
        pos: [i32; 3],
        /// The volume, the source's own float scale.
        volume: f32,
        /// The pitch byte, 63 for full pitch.
        pitch: u8,
    },
    /// A local sound the client's own play staged: a block break or place,
    /// a footstep, a swim or splash, the own player's hurt or death, an item
    /// pickup, or a level-up.
    ///
    /// Reported once per staging; the client's pump plays it through the
    /// audio engine as-is — the position already rides in blocks and the
    /// pitch is already a factor, unlike [`ClientEvent::SoundEffect`]'s wire
    /// units.
    LocalSound {
        /// The sound event to play, exactly as the call site names it.
        name: String,
        /// The position in blocks: x, y, z.
        pos: [f32; 3],
        /// The staged volume.
        volume: f32,
        /// The staged pitch factor.
        pitch: f32,
        /// The call site's category, used when the event's own category
        /// string names nothing known.
        category_hint: SoundCategory,
    },
    /// A particle spawn, from clientbound 0x2A.
    ///
    /// Reported once per packet; the particle engine spawns from it. An id
    /// outside the table already decodes as `BARRIER` (id 35), the source's
    /// own fallback (`S2APacketParticles.readPacketData`).
    Particle {
        /// The particle id, mapped into 0-41 at decode.
        particle_id: i32,
        /// Whether the particle renders past the range cull.
        long_distance: bool,
        /// The spawn position: x, y, z.
        pos: [f32; 3],
        /// The per-axis spawn spread: x, y, z.
        offset: [f32; 3],
        /// The particle speed.
        speed: f32,
        /// The spawn count.
        count: i32,
        /// The trailing data run (block/item ids).
        data: Vec<i32>,
    },
    /// The live particles' draw-ready snapshot for one tick: what the
    /// window's particle pass draws.
    ///
    /// Reported once per tick while the pool holds a visible particle, plus
    /// the first empty tick after — never for a settled empty pool. The
    /// window keeps the last report's frames and feeds them to the pass every
    /// frame, interpolated at the frame's own fraction; the tick only names
    /// the snapshot — the particle pass takes no scroll clock.
    Particles {
        /// The snapshot's frames, in draw order.
        frames: Vec<ParticleFrame>,
        /// The session tick the snapshot rode in on.
        tick: u32,
        /// Every live particle the engine holds, drawn or not: the debug
        /// overlay's `P:` count, which the visible-only frames cannot answer.
        total: usize,
    },
    /// A global entity spawned, from clientbound 0x2C.
    ///
    /// Reported once per packet; the entity registers for 0x13 removal under
    /// the standing entity store path. The kind rides raw — 1 is lightning,
    /// and any other kind keeps the record and draws nothing
    /// (`NetHandlerPlayClient.handleSpawnGlobalEntity:463-486`). The
    /// position rides the wire's own fixed-point ints; divide by 32 for
    /// blocks.
    GlobalEntity {
        /// The entity id the server will use for this entity.
        entity_id: i32,
        /// The kind byte; 1 is lightning.
        kind: u8,
        /// The raw fixed-point position: x, y, z.
        pos: [i32; 3],
    },
}

/// One status effect the player holds.
///
/// The store's unit: the effect's id, its amplifier and the ticks left —
/// `PotionEffect`'s own `potionID`, `amplifier` and `duration`. The source's
/// ambient and particles flags are carried by neither this type nor
/// [`ClientEvent::Effects`]: the wire's hide-particles byte feeds the source's
/// `showParticles`, and no consumer of this event reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusEffect {
    /// The effect's id.
    pub effect_id: u8,
    /// The effect's amplifier: 0 is level I.
    pub amplifier: u8,
    /// The effect's remaining duration in ticks.
    pub duration: i32,
}

/// One sign's last text, keyed by its position in the session's sign map.
///
/// The lines are the source's chat components as sent; the sign they are drawn
/// on is the world's, and an entry arrives only from clientbound 0x33. The
/// block rides along so the draw knows the board's variant and orientation.
///
/// There is NO all-empty clearing rule: S33 always carries four strings and
/// the map is insert-only — an entry leaves only when its block stops being
/// a sign (a port rule, reported as [`ClientEvent::SignTextCleared`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SignText {
    /// The four lines, as sent.
    pub lines: [String; 4],
    /// The block id the store held at the position when the text arrived.
    pub block_id: u16,
    /// The metadata nibble the store held at the position then.
    pub metadata: u8,
}

/// The standing sign's block id (`Block.java`:1321).
pub const SIGN_STANDING_ID: u16 = 63;
/// The wall sign's block id (`Block.java`:1326).
pub const SIGN_WALL_ID: u16 = 68;

/// The stack a `MC|BOpen` custom payload opens: the session's held-slot
/// snapshot at receive time, for the written-book identity alone.
///
/// The payload itself is empty and is never read — it takes no payload
/// parameter by construction (`EntityPlayerMP.displayGUIBook:840-848` sends
/// an empty buffer); the client opens its own held stack, guarded by the
/// written-book identity (`NetHandlerPlayClient.handleCustomPayload:1855-1863`).
/// Another channel, an empty hand or a non-written held item opens nothing.
fn book_open_stack(channel: &str, held: &Option<MetadataItem>) -> Option<MetadataItem> {
    if channel != BOOK_OPEN_CHANNEL {
        return None;
    }
    let stack = held.clone()?;
    if stack.id == WRITTEN_BOOK_ID {
        Some(stack)
    } else {
        None
    }
}

/// Whether the block id is a sign's: the standing and wall variants alone.
pub fn is_sign_block(block_id: u16) -> bool {
    block_id == SIGN_STANDING_ID || block_id == SIGN_WALL_ID
}

/// The packed store value's block id: the value shifted down four
/// (`world_view`'s own split).
pub fn block_id_of(packed: u16) -> u16 {
    packed >> 4
}

/// The packed store value's metadata nibble.
pub fn block_meta_of(packed: u16) -> u8 {
    (packed & 0xF) as u8
}

/// Something went wrong in the session.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The connection or framing failed.
    #[error("connection error: {0}")]
    Frame(#[from] oxide_proto::frame::FrameError),
    /// A packet could not be decoded.
    #[error("packet error: {0}")]
    Packet(#[from] oxide_proto_v47::PacketError),
    /// The server asked for encryption and there is no signed-in account to
    /// answer with: the client never prompts, it refuses with the sign-in
    /// sentence.
    #[error(
        "this server requires a signed-in account — run `oxide-launcher auth`, then start the client with --account"
    )]
    EncryptionRequired,
    /// The session join the seam was asked to post failed; the text is the
    /// seam's own, shown verbatim on the disconnected screen.
    #[error("the session join failed: {message}")]
    JoinFailed {
        /// The seam's failure text.
        message: String,
    },
    /// The server's key did not parse, or the response did not encrypt.
    #[error("crypto error: {0}")]
    Crypto(#[from] oxide_proto::crypto::CryptoError),
    /// A write of our own reply failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The session's mesh pool could not be built.
    #[error("the mesh pool could not be built: {0}")]
    Pool(#[from] ThreadPoolBuildError),
}

/// A client session over one framed connection.
pub struct Session<S> {
    /// The framed connection to the server.
    conn: Conn<S>,
    /// What the session connects with and sends.
    config: SessionConfig,
    /// The assets the meshes are built from, resolved once.
    mesh: Arc<MeshAssets>,
}

impl Session<TcpStream> {
    /// Connects and returns the session; nothing is sent until [`Session::run`].
    pub fn connect(config: &SessionConfig) -> Result<Self, SessionError> {
        let stream = TcpStream::connect((config.host.as_str(), config.port))?;
        Ok(Self::new(Conn::new(stream), config.clone()))
    }
}

impl<S: Read + Write + DeadlineStream> Session<S> {
    /// Wraps an existing connection.
    ///
    /// A config with no assets gets the fallback ones, and this logs once that
    /// every block will draw the fallback sprite: the mesher never logs per
    /// block.
    pub fn new(conn: Conn<S>, config: SessionConfig) -> Self {
        let mesh = match &config.mesh {
            Some(assets) => Arc::clone(assets),
            None => {
                info!("no mesh assets: every block draws the atlas's fallback sprite");
                Arc::new(MeshAssets::fallback())
            }
        };
        Self { conn, config, mesh }
    }

    /// Runs the handshake, Login Start and the login phase, returning how the
    /// login settled.
    ///
    /// The mock-server harness settles the login on its own through this: the
    /// happy path returns the accepted profile with the ciphers enabled, and
    /// the refusal cases return before it. [`Session::run_over`] runs this
    /// first, then continues into the play loop.
    pub fn run_login(
        &mut self,
        events: &Sender<ClientEvent>,
    ) -> Result<LoginOutcome, SessionError> {
        // Nothing has touched the framing yet: the handshake and Login Start go
        // out plain, and the server answers by naming a compression threshold.
        let handshake = payload_of(|out| {
            write_handshake(
                out,
                PROTOCOL,
                &self.config.host,
                self.config.port,
                NEXT_STATE_LOGIN,
            )
        })?;
        send_reply(&mut self.conn, &handshake)?;
        let login_start = payload_of(|out| write_login_start(out, &self.config.username))?;
        send_reply(&mut self.conn, &login_start)?;

        login(&mut self.conn, events, self.config.account.as_deref())
    }

    /// Whether the login's ciphers are enabled: true once an Encryption
    /// Response went out and the connection started decrypting.
    pub fn is_encrypted(&self) -> bool {
        self.conn.is_encrypted()
    }

    /// Runs the session to completion: handshake, login, then the play loop.
    ///
    /// Returns `Ok(())` when the server closed the connection or the stream
    /// ended. Malformed packets are reported as an error, never a panic.
    ///
    /// The play loop reads with [`Conn::recv_or_idle`]; when a wait for the
    /// next frame times out — and only then — it hands the mesh queue's next
    /// columns to the pool and reports whatever builds have finished. A frame
    /// that is already readable is always read before any mesh or tick work,
    /// and a build never runs on this thread, which is what keeps the keepalive
    /// echo — and the teleport echo — answerable while a burst of columns is
    /// arriving.
    ///
    /// The window's input arrives on `inputs` and is drained into the held
    /// intent; the ticks that have come due then run, with the player, the
    /// clock and the sky reported as they move.
    pub fn run_over(
        self,
        events: &Sender<ClientEvent>,
        inputs: Receiver<InputEvent>,
    ) -> Result<(), SessionError> {
        let mut this = self;
        // The handshake, Login Start and the login phase run first, on the
        // same path the harness settles on its own.
        match this.run_login(events)? {
            LoginOutcome::Accepted { uuid, username } => {
                report(events, ClientEvent::LoggedIn { uuid, username });
            }
            LoginOutcome::Ended => return Ok(()),
        }
        let Session {
            mut conn,
            config,
            mesh,
        } = this;
        // The client settings the session holds: Join Game sends them, and
        // the view-distance slider re-sends them on change.
        let mut settings = config.settings;

        // The pool and the results channel live for the whole play loop; the
        // handshake and the login above mesh nothing.
        let pool = mesh_pool()?;
        let (finished, results) = crossbeam_channel::unbounded::<MeshResult>();
        let mut queue = MeshQueue::new();

        let mut world: Option<World> = None;
        let mut player = Player::new();
        let mut clock: Option<Clock> = None;
        let mut ticker = Ticker::new(TICK_PERIOD);
        let mut intent = Intent::neutral();
        // The local sounds' dice: one seeded roll source for the hurt,
        // pickup, swim and splash spreads. The seed is fixed, so a replayed
        // stream stages the same pitches — only the staging formulas carry
        // parity, never the draws.
        let mut sound_rng = StdRng::seed_from_u64(SOUND_DICE_SEED);
        // The experience level the last 0x1F carried, and the session tick
        // of the last level-up sound: the level-up gate reads both
        // (`EntityPlayer.addExperienceLevel:2037-2054`).
        let mut xp_level: i32 = 0;
        let mut last_xp_sound_tick: u64 = 0;
        // The footstep walk and the water flag the step sounds read: the
        // walked distance since the last step and the threshold past which
        // the next one stages (`Entity.java:139-146, :271`, `:872-877`),
        // and the water state the previous tick left
        // (`handleWaterMovement:1111-1129`).
        let mut distance_walked: f32 = 0.0;
        let mut next_step_distance: i32 = 1;
        let mut was_in_water: bool = false;
        // The gamemode the reach reads, from the last Join Game
        // (`PlayerControllerMP.getBlockReachDistance`, `:344-346`); the
        // controller's own default until one arrives is survival
        // (`PlayerControllerMP.java:59`).
        let mut gamemode: u8 = 0;
        // The aim the last recompute left; only a change is reported.
        let mut aim: Option<Aim> = None;
        // The camera mode the frame last stepped to, from the perspective
        // key's edge: the tick's occlusion rays read it, first person
        // reading none.
        let mut camera_mode: u8 = 0;
        // The clipped third-person distance the last tick reported; only a
        // change is reported, and leaving third person resets it with no
        // event.
        let mut camera_clip: f32 = oxide_render::camera::THIRD_PERSON_DISTANCE;
        // The dig machine and the destroy stages, and the left button they
        // read: the held flag and the presses queued since the last tick.
        let mut dig = DigState::new();
        let mut stages = BreakStages::new();
        // The destroy stages' own clock: the counter the stage writes carry
        // and the twenty-tick sweep cadence read (`RenderGlobal.updateClouds`,
        // `RenderGlobal.java`:1138-1146).
        let mut stage_counter: u32 = 0;
        let mut left_held = false;
        let mut left_presses: u32 = 0;
        let mut right_presses: u32 = 0;
        // The tracked entities and the player list: the connection's view of
        // the world's population, applied from the packets below and cleared
        // when a rebuild replaces the world.
        let mut entities = Entities::new();
        let mut player_list = PlayerList::new();
        // The scoreboard and the tab text: further connection-scoped view
        // state, fed by clientbound 0x3B–0x3E and 0x47 and reported on
        // change. The board is cleared with a fresh world and carried across
        // a dimension respawn — the source's own two rules
        // (`handleJoinGame:281`, `handleRespawn:1058-1065`); the tab text is
        // touched by nothing but its own packet.
        let mut board = Scoreboard::new();
        let mut tab_text: Option<(String, String)> = None;
        // The dimension the world was built for, from the last Join Game: a
        // respawn compares its own dimension against it to decide whether the
        // world survives (`NetHandlerPlayClient.handleRespawn:1056-1073`).
        let mut dimension: i8 = 0;
        // Whether the session waits for the 0x08 that places it after a
        // respawn. While it is set the tick sends no movement report: the
        // fresh player the respawn left must not race the correction that is
        // about to move it, and the source's own report waits for a world it
        // belongs to (`EntityPlayerSP.onUpdate:170`).
        let mut awaiting_respawn_position = false;
        // The respawn requests the window's clicks and Space asked for since
        // the last drain: each is answered with exactly one client-status
        // packet, and there is no held-repeat path
        // (`GuiGameOver.actionPerformed:57-77` sends one on the button's click).
        let mut respawn_requests: u32 = 0;
        // The chat messages the window's field sent since the last drain:
        // each is answered with exactly one Chat Message packet (play id
        // 0x01), in send order (`GuiChat.keyTyped`:104-137 sends on the
        // Enter press, one message per press).
        let mut chat_messages: Vec<String> = Vec::new();
        // The player's own window and the one window a server may open, with
        // the wire's apply rules for both.
        let mut windows = Windows::new();
        // The own player's live effects, by effect id: the store 0x1D and
        // 0x1E maintain (`EntityLivingBase.addPotionEffect:745-760` behind
        // `NetHandlerPlayClient.handleEntityEffect:1502-1513`).
        let mut effects: BTreeMap<u8, StatusEffect> = BTreeMap::new();
        // The last text every sign carried, by position: what the view draws
        // the sign's front face from.
        let mut signs: BTreeMap<(i32, i32, i32), SignText> = BTreeMap::new();
        // The weather the 0x2B reasons steer: the targets the wire names,
        // eased once per tick and reported while the easing moves.
        let mut weather = WeatherState::default();
        // The particle pool the wire and the weather feed: 0x2A spawns into
        // it through the setting reduction, the tick steps it against the
        // session's columns and rains the splashes into it, and the
        // snapshot below reports it to the window's pass.
        let mut particles = ParticleEngine::seeded(PARTICLE_DICE_SEED, PER_LIST_CAP);
        // The store's particle amount reaches the pool through Task 14's
        // apply chain; until that push lands the pool runs unreduced.
        particles.set_limit(ParticleSetting::All);
        // The splash sampler's dice: one seeded roll source for the column
        // picks and the lava-or-drop arms. The seed is fixed, so a replayed
        // stream samples the same columns — only the sampling formula
        // carries parity, never the draws.
        let mut splash_rng = StdRng::seed_from_u64(PARTICLE_DICE_SEED);
        // The particle clock the columns and the snapshots ride in on, and
        // the two liveness cells that keep the reports quiet on settled
        // ticks: one event per tick that changed the pool or the sky, none
        // for a settled one.
        let mut particle_tick: u32 = 0;
        let mut particles_live = false;
        let mut columns_live = false;
        // The graphics settings the column meshes build with: the store's
        // values once the client pushes them, the two defaults until then.
        let mut graphics_fast = GRAPHICS_FAST;
        let mut smooth_lighting = SMOOTH_LIGHTING;
        // The mouse sensitivity the look mapping runs with: the store's
        // value once the client pushes it, the option's own default until
        // then (`MOUSE_SENSITIVITY`). The invert flag rides beside it.
        let mut sensitivity = MOUSE_SENSITIVITY;
        let mut invert_mouse = false;
        // The terrain screen's arming: the join sets it and the first
        // Player Position And Look clears it with one `TerrainReady`;
        // a cross-dimension respawn re-arms it (`handleRespawn:1062-1068`
        // re-shows `GuiDownloadTerrain` beside the rebuild, `handlePlayerPosLook:719-726`
        // clears it on the first S08 after).
        let mut terrain_pending = false;

        loop {
            // The window's input, drained up to a bound: nothing is dropped —
            // the backlog drains across passes — and the bound keeps one pass'
            // work finite however far the window runs ahead. The mouse buttons
            // are the dig input, so they go to it rather than to the movement
            // intent.
            let mut looked = false;
            for _ in 0..INPUTS_PER_PASS {
                match inputs.try_recv() {
                    Ok(event) => {
                        // The settings pushes are not gameplay input: they
                        // land whether the player is dead or alive, so a
                        // change from the pause menu always reaches the state
                        // it owns. A changed wire payload re-sends C15 with
                        // every field current, the way the view-distance
                        // arm below does.
                        if let InputEvent::PushSettings(push) = &event {
                            if apply_session_push(
                                &mut settings,
                                &mut graphics_fast,
                                &mut smooth_lighting,
                                &mut sensitivity,
                                &mut invert_mouse,
                                &mut particles,
                                push,
                            ) {
                                let echo = payload_of(|out| write_client_settings(out, &settings))?;
                                send_reply(&mut conn, &echo)?;
                            }
                            continue;
                        }
                        if let InputEvent::SetParticleSetting { setting } = event {
                            particles.set_limit(setting);
                            continue;
                        }
                        if player.dead {
                            // While dead the window's input is the death
                            // view's: a click or Space asks for the respawn —
                            // one request per press — and nothing else
                            // reaches the movement, the look or the
                            // interaction. A focus loss still releases the
                            // held intent, so a window that stops receiving
                            // leaves nothing held.
                            match event {
                                InputEvent::MouseButton { pressed: true, .. }
                                | InputEvent::Key {
                                    key: Key::Space,
                                    pressed: true,
                                } => respawn_requests += 1,
                                InputEvent::FocusLost => intent.release_all(),
                                _ => {}
                            }
                        } else {
                            match event {
                                InputEvent::MouseButton { button, pressed } => {
                                    apply_button(
                                        button,
                                        pressed,
                                        &mut left_held,
                                        &mut left_presses,
                                        &mut right_presses,
                                    );
                                }
                                InputEvent::SendChat { text } => chat_messages.push(text),
                                InputEvent::ClickWindow {
                                    window_id,
                                    slot,
                                    button,
                                    mode,
                                } => {
                                    // One 0x0E per click, carrying the
                                    // container's own next action number
                                    // (`Container.getNextTransactionID:561-565`)
                                    // and the echo the local prediction's
                                    // branch returned
                                    // (`PlayerControllerMP.windowClick:534-540`),
                                    // never the cursor. A predicting click
                                    // republishes the window, the way the
                                    // source's own container mutation is what
                                    // the view draws.
                                    if let Some((request, mutated)) = click_window_packet(
                                        &mut windows,
                                        window_id,
                                        slot,
                                        button,
                                        mode,
                                    )? {
                                        send_reply(&mut conn, &request)?;
                                        if mutated {
                                            let snapshot_id = u8::try_from(window_id)
                                                .expect("a held window fits the wire's byte");
                                            report_window_snapshot(&windows, snapshot_id, events);
                                        }
                                    }
                                }
                                InputEvent::CloseWindow { window_id } => {
                                    // One 0x0D for a window the state holds —
                                    // the source's close path writes it for the
                                    // container the screen stood on
                                    // (`EntityPlayerSP.closeScreen:330-334`) —
                                    // and the window leaves the state with it,
                                    // reporting the same event a wire close
                                    // does. Window 0 is the player's own and
                                    // stays; a second close of the same window
                                    // answers nothing.
                                    if close_window(&mut windows, window_id) {
                                        let request =
                                            payload_of(|out| write_close_window(out, window_id))?;
                                        send_reply(&mut conn, &request)?;
                                        if window_id != 0 {
                                            report(events, ClientEvent::WindowClosed { window_id });
                                        }
                                    }
                                }
                                InputEvent::CreativeAction { slot, item } => {
                                    let request = payload_of(|out| {
                                        write_creative_inventory_action(out, slot, item.as_ref())
                                    })?;
                                    send_reply(&mut conn, &request)?;
                                }
                                InputEvent::EnchantItem { window_id, index } => {
                                    let request = payload_of(|out| {
                                        write_enchant_item(out, window_id, index)
                                    })?;
                                    send_reply(&mut conn, &request)?;
                                }
                                InputEvent::CustomPayload { channel, data } => {
                                    // The containers' C17 confirms (beacon,
                                    // villager, anvil): the channel and the
                                    // caller-framed body ride the landed
                                    // plugin-message writer verbatim.
                                    let request = payload_of(|out| {
                                        write_plugin_message(out, &channel, &data)
                                    })?;
                                    send_reply(&mut conn, &request)?;
                                }
                                InputEvent::UpdateSign { x, y, z, lines } => {
                                    let request =
                                        payload_of(|out| write_update_sign(out, x, y, z, &lines))?;
                                    send_reply(&mut conn, &request)?;
                                }
                                InputEvent::HeldItemChange { slot } => {
                                    // The selection rides the model
                                    // (`Minecraft.java:2085`'s hotbar keys and
                                    // `:1879`'s wheel reaching
                                    // `InventoryPlayer.changeCurrentItem:165`)
                                    // and the wire
                                    // (`PlayerControllerMP.syncCurrentPlayItem:379-388`
                                    // reaching `C09PacketHeldItemChange:32-35`),
                                    // and the HUD hears the same value.
                                    windows.player.inventory.set_selected(slot);
                                    let request =
                                        payload_of(|out| write_held_item_change(out, slot))?;
                                    send_reply(&mut conn, &request)?;
                                    report(events, ClientEvent::HeldItemSlot { slot });
                                }
                                InputEvent::DropItem { whole } => {
                                    // The drop key's own pair
                                    // (`EntityPlayerSP.dropOneItem:279-284`):
                                    // the whole stack is `DROP_ALL_ITEMS`, one
                                    // item `DROP_ITEM`, both at the origin and
                                    // facing down.
                                    let status = if whole {
                                        DiggingStatus::DropAll
                                    } else {
                                        DiggingStatus::Drop
                                    };
                                    let request = payload_of(|out| {
                                        write_player_digging(out, status, 0, 0, 0, 0)
                                    })?;
                                    send_reply(&mut conn, &request)?;
                                }
                                InputEvent::OpenInventory => {
                                    // The inventory key's open: one Client
                                    // Status carrying action 2, the source's
                                    // `OPEN_INVENTORY_ACHIEVEMENT`
                                    // (`Minecraft.java:2090-2103`, send at
                                    // :2100) — no guard, so every open sends
                                    // one.
                                    let request = payload_of(|out| {
                                        write_client_status(out, ClientStatusAction::OpenInventory)
                                    })?;
                                    send_reply(&mut conn, &request)?;
                                }
                                InputEvent::LeaveBed => {
                                    // The sleep cover's leave: the Escape key
                                    // and the Leave Bed button both send C0B
                                    // carrying action 2
                                    // (`GuiSleepMP.keyTyped:51-67` reaching
                                    // `C0BPacketEntityAction` with
                                    // `STOP_SLEEPING`). Before the join names
                                    // the id there is nothing to send.
                                    if let Some(entity_id) = player.entity_id {
                                        let request = payload_of(|out| {
                                            write_entity_action(
                                                out,
                                                entity_id,
                                                EntityAction::StopSleeping,
                                                0,
                                            )
                                        })?;
                                        send_reply(&mut conn, &request)?;
                                    } else {
                                        debug!("a leave-bed names no joined player");
                                    }
                                }
                                InputEvent::SetViewDistance { chunks } => {
                                    // The slider's own clamp is the store's
                                    // range (`VIEW_DISTANCE_RANGE`); an
                                    // unchanged value sends nothing.
                                    let chunks =
                                        chunks.clamp(VIEW_DISTANCE_RANGE.0, VIEW_DISTANCE_RANGE.1);
                                    if chunks != settings.view_distance {
                                        settings.view_distance = chunks;
                                        // The slider commits here: the
                                        // source's drag only sets the field
                                        // and dirties the renderers
                                        // (`setOptionFloatValue` RENDER_DISTANCE,
                                        // `GameSettings.java:328-332`, no
                                        // packet), and the Done button's
                                        // `saveOptions` sends the whole
                                        // settings (`:1186`, via
                                        // `sendSettingsToServer:1203-1216`).
                                        // The session has no screen close, so
                                        // the input event is the commit: one
                                        // C15 with every field current.
                                        let echo = payload_of(|out| {
                                            write_client_settings(out, &settings)
                                        })?;
                                        send_reply(&mut conn, &echo)?;
                                    }
                                }
                                InputEvent::Perspective { mode } => {
                                    // The F5 edge stepped the camera mode:
                                    // the tick's occlusion rays read it.
                                    camera_mode = mode.min(2);
                                }
                                InputEvent::RebuildMeshes => {
                                    // The A chord's rebuild is the source's
                                    // `loadRenderers` (`RenderGlobal.java`):
                                    // every loaded column is queued once more,
                                    // and the next builds read whatever world
                                    // now stands. Nothing goes on the wire.
                                    if let Some(world) = world.as_ref() {
                                        for (cx, cz) in world.loaded() {
                                            queue.mark_dirty(cx, cz);
                                        }
                                    }
                                }
                                other => {
                                    looked |= apply_input(
                                        other,
                                        &mut intent,
                                        &mut player,
                                        sensitivity,
                                        invert_mouse,
                                    );
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            // Each queued respawn request goes out now, one packet per press:
            // the requirement is the source's own button, and the server
            // answers with clientbound 0x07.
            for _ in 0..std::mem::take(&mut respawn_requests) {
                let request =
                    payload_of(|out| write_client_status(out, ClientStatusAction::Respawn))?;
                send_reply(&mut conn, &request)?;
            }
            // Each queued chat message goes out now, one packet per send, in
            // the order the field sent them: `write_chat` is the protocol's
            // own Chat Message (play id 0x01), the message as a
            // length-prefixed UTF-8 string. The field's 100-character cap is
            // the field's — nothing is cut here — so a message that slipped
            // past it still goes out whole, with the guard's log as the only
            // record that the field's rule was broken.
            for text in std::mem::take(&mut chat_messages) {
                if chat_over_cap(&text) {
                    debug!(
                        chars = text.chars().count(),
                        "the chat message is past the field's 100-character cap"
                    );
                }
                let request = write_chat(&text);
                send_reply(&mut conn, &request)?;
            }
            // A look moves the aim at once — the mouse is polled between
            // ticks (`EntityRenderer.updateMouse:1094-1123`) — and only a
            // change is reported.
            if looked {
                update_aim(world.as_ref(), &player, gamemode, &mut aim, events);
            }

            // One frame, or one idle wait. A frame that is already readable is
            // read before any further work; the due ticks run after it.
            match conn.recv_or_idle(MESH_TICK) {
                Ok(RecvOutcome::Frame(payload)) => {
                    let (id, body) = match read_packet_id(&payload) {
                        Ok(read) => read,
                        Err(error) => {
                            warn!(error = %error, packet = "play", "the packet id did not read");
                            return Err(SessionError::Packet(error));
                        }
                    };
                    match id {
                        KeepAlive::ID => {
                            let keep_alive = decoded(id, KeepAlive::decode(body))?;
                            let reply = payload_of(|out| write_keep_alive(out, keep_alive.id))?;
                            send_reply(&mut conn, &reply)?;
                            report(events, ClientEvent::KeepAlive { id: keep_alive.id });
                        }
                        JoinGame::ID => {
                            let join = decoded(id, JoinGame::decode(body))?;
                            // The dimension decides whether columns carry sky light. A
                            // re-sent Join Game starts the column set over: the queue's
                            // columns belong to the old world, so it is emptied in
                            // place — the generation counter survives, and any job still
                            // out completes stale and builds against the new one.
                            world = Some(World::new(join.dimension == 0));
                            queue.clear_in_place();
                            // A rebuilt world carries no tracked entities and
                            // no known players: the store and the list start
                            // over with it.
                            entities.clear();
                            let list_before = std::mem::take(&mut player_list);
                            let board_before = std::mem::take(&mut board);
                            // The rebuilt world reports the emptied list and
                            // board at once; a first Join Game held neither
                            // and reports nothing.
                            report_list_change(&player_list, &list_before, events);
                            report_board_change(&board, &board_before, events);
                            dimension = join.dimension;
                            // The player's own entity id is named here. It gates this
                            // client's tick sends: the source's tick is gated on the
                            // player standing in a loaded world
                            // (`EntityPlayerSP.onUpdate:170`), and the action edges
                            // carry the id.
                            player.entity_id = Some(join.entity_id);
                            // The gamemode the reach reads: `getBlockReachDistance`
                            // takes creative from the controller's game type
                            // (`PlayerControllerMP.java:344-346`).
                            gamemode = join.gamemode;
                            let settings = payload_of(|out| write_client_settings(out, &settings))?;
                            send_reply(&mut conn, &settings)?;
                            let brand = payload_of(|out| {
                                write_plugin_message(out, BRAND_CHANNEL, BRAND_PAYLOAD)
                            })?;
                            send_reply(&mut conn, &brand)?;
                            report(
                                events,
                                ClientEvent::Joined {
                                    entity_id: join.entity_id,
                                    gamemode: join.gamemode,
                                    dimension: join.dimension,
                                    difficulty: join.difficulty,
                                    max_players: join.max_players,
                                    level_type: join.level_type,
                                    reduced_debug_info: join.reduced_debug_info,
                                },
                            );
                            // The join arms the terrain screen: the first
                            // S08 after it reports `TerrainReady`.
                            terrain_pending = true;
                        }
                        ChatMessage::ID => {
                            let message = decoded(id, ChatMessage::decode(body))?;
                            report(
                                events,
                                ClientEvent::Chat {
                                    text: message.text,
                                    position: message.position,
                                },
                            );
                        }
                        UpdateHealth::ID => {
                            let update = decoded(id, UpdateHealth::decode(body))?;
                            player.health = update.health;
                            player.food = update.food;
                            player.saturation = update.saturation;
                            report(
                                events,
                                ClientEvent::Health {
                                    health: update.health,
                                    food: update.food,
                                    saturation: update.saturation,
                                },
                            );
                            // Health at or below zero is the death
                            // (`EntityLivingBase.onEntityUpdate:344-349` reads
                            // it off the health every tick); the state is
                            // entered once, and later 0x06 packets keep
                            // updating the health without re-reporting it.
                            if player.health <= 0.0 && !player.dead {
                                report(events, ClientEvent::Died);
                                player.dead = true;
                                player.death_time = 0;
                                // The death sound: the `!isSilent` branch of
                                // the same handler that stages the hurt
                                // (`EntityLivingBase.java:755-758`), at the
                                // player with the same spread.
                                report(
                                    events,
                                    ClientEvent::LocalSound {
                                        name: DEATH_SOUND.to_string(),
                                        pos: [
                                            player.position[0] as f32,
                                            player.position[1] as f32,
                                            player.position[2] as f32,
                                        ],
                                        volume: 1.0,
                                        pitch: hurt_pitch(sound_rng.random(), sound_rng.random()),
                                        category_hint: SoundCategory::Players,
                                    },
                                );
                                // The death screen unpresses every key
                                // (`Minecraft.setIngameNotInFocus:1469-1478`
                                // through `KeyBinding.unPressAllKeys`), and
                                // nothing of the movement survives the
                                // death: the flags and their server mirrors
                                // clear with it, so no stale sprint or sneak
                                // edge can follow the respawn. The mirrors
                                // alone owe the server nothing — a respawn
                                // replaces its own player too.
                                intent.release_all();
                                left_held = false;
                                left_presses = 0;
                                right_presses = 0;
                                player.sprinting = false;
                                player.server_sprint_state = false;
                                player.sneaking = false;
                                player.server_sneak_state = false;
                                // The dig is cancelled the way the death
                                // screen cancels it: `sendClickBlockToController`
                                // runs with no left click while the screen is
                                // up (`Minecraft.java:1515-1518`), which is
                                // the abort and its stage's removal.
                                for action in dig.reset_block_removing() {
                                    apply_dig_action(
                                        &action,
                                        world.as_mut(),
                                        &mut queue,
                                        &mut stages,
                                        stage_counter,
                                        player.entity_id,
                                        &mut conn,
                                        events,
                                        &mut particles,
                                        &mesh.tint_maps,
                                    )?;
                                }
                            }
                        }
                        EntityStatus::ID => {
                            let status = decoded(id, EntityStatus::decode(body))?;
                            // The status lands on whatever entity the id
                            // names — the store tracks the hurt window for
                            // every one of them — and the player's own hurt
                            // flash is the same signal arriving at its own
                            // state (`NetHandlerPlayClient.handleEntityStatus`).
                            entities.apply_status(status.entity_id, status.status);
                            if Some(status.entity_id) == player.entity_id {
                                if status.status == EntityStatus::HURT {
                                    // The hurt flash: `hurtTime` back to its
                                    // maximum, the attack yaw zeroed
                                    // (`EntityLivingBase.handleStatusUpdate:1362-1363`).
                                    player.apply_hurt_status();
                                    // The own player's hurt sound: the
                                    // handler is the `instanceof
                                    // EntityPlayer` branch
                                    // (`EntityLivingBase.java:747-750`),
                                    // staged at the player with the hurt
                                    // pitch spread.
                                    report(
                                        events,
                                        ClientEvent::LocalSound {
                                            name: HURT_SOUND.to_string(),
                                            pos: [
                                                player.position[0] as f32,
                                                player.position[1] as f32,
                                                player.position[2] as f32,
                                            ],
                                            volume: 1.0,
                                            pitch: hurt_pitch(
                                                sound_rng.random(),
                                                sound_rng.random(),
                                            ),
                                            category_hint: SoundCategory::Players,
                                        },
                                    );
                                } else {
                                    debug!(
                                        status = status.status,
                                        "a status this client has no use for"
                                    );
                                }
                            } else if status.status == EntityStatus::HURT
                                || status.status == EntityStatus::DEATH
                            {
                                // A foreign entity's hurt/death: the same
                                // handler's generic arm
                                // (`EntityLivingBase.handleStatusUpdate:1358-1384`)
                                // stages the kind's own names at the entity,
                                // with the same pitch spread. Kinds the
                                // source never sounds stay silent.
                                if let Some(entity) = entities.get(status.entity_id) {
                                    let variant = MobVariant {
                                        slime_size: watcher_byte(&entity.metadata, 16).unwrap_or(1)
                                            as i32,
                                        horse_type: watcher_byte(&entity.metadata, 19).unwrap_or(0)
                                            as i32,
                                        guardian_elder: watcher_int(&entity.metadata, 16)
                                            .is_some_and(|flags| flags & 4 != 0),
                                    };
                                    if let Some(sounds) = mob_sounds(entity.kind, &variant) {
                                        let name = if status.status == EntityStatus::HURT {
                                            sounds.hurt
                                        } else {
                                            sounds.death
                                        };
                                        report(
                                            events,
                                            ClientEvent::LocalSound {
                                                name: name.to_string(),
                                                pos: [
                                                    entity.position[0] as f32,
                                                    entity.position[1] as f32,
                                                    entity.position[2] as f32,
                                                ],
                                                volume: sounds.volume,
                                                pitch: hurt_pitch(
                                                    sound_rng.random(),
                                                    sound_rng.random(),
                                                ),
                                                category_hint: sounds.category,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                        Respawn::ID => {
                            let respawn = decoded(id, Respawn::decode(body))?;
                            // The dimension arrives as a raw `i32`; the world
                            // model's own width for it is the byte Join Game
                            // names it in, and a value outside that range
                            // cannot build a world this client can carry — it
                            // would silently pick a different sky flag — so it
                            // is refused rather than truncated.
                            let respawned = respawn_dimension(respawn.dimension)?;
                            // The source keeps the world when the dimension
                            // matches and rebuilds it otherwise
                            // (`NetHandlerPlayClient.handleRespawn:1056-1073`).
                            if world.is_some() && respawned != dimension {
                                world = Some(World::new(respawned == 0));
                                // The queue's columns belong to the world that
                                // is gone. The clear keeps the generation
                                // counter, so a build still out can never
                                // answer as the new world's own (backlog item
                                // 2); whatever it returns is discarded, and
                                // the column is queued once more against the
                                // new world.
                                queue.clear_in_place();
                                // The world that held them is gone: the
                                // tracked entities and the list go with it
                                // (`NetHandlerPlayClient.handleRespawn`).
                                entities.clear();
                                // The signs go with that world too: every
                                // mapped entry clears, so the view drops a
                                // text no board stands behind anymore.
                                let gone: Vec<(i32, i32, i32)> = signs.keys().copied().collect();
                                for (x, y, z) in gone {
                                    signs.remove(&(x, y, z));
                                    report(events, ClientEvent::SignTextCleared { x, y, z });
                                }
                                let list_before = std::mem::take(&mut player_list);
                                // The rebuilt world reports the emptied list
                                // at once.
                                report_list_change(&player_list, &list_before, events);
                                report(events, ClientEvent::WorldCleared);
                                // The rebuild re-arms the terrain screen: the
                                // next S08 reports `TerrainReady`.
                                terrain_pending = true;
                            }
                            dimension = respawned;
                            // The gamemode the reach reads, restated by the
                            // respawn: the source's handler sets the
                            // controller's game type from the packet
                            // (`:1072`, `PlayerControllerMP.setGameType:...`).
                            gamemode = respawn.gamemode;
                            // The player starts over where the source's fresh
                            // player would (see `Player::reset_for_respawn`).
                            player.reset_for_respawn();
                            // Nothing of the dig or the aim survives either:
                            // the dig's cancel goes out now (an idle machine
                            // produces nothing), and a live aim is cleared and
                            // reported so the window drops its outline; the
                            // tick after the position arrives recomputes it
                            // against whatever world now stands.
                            for action in dig.reset_block_removing() {
                                apply_dig_action(
                                    &action,
                                    world.as_mut(),
                                    &mut queue,
                                    &mut stages,
                                    stage_counter,
                                    player.entity_id,
                                    &mut conn,
                                    events,
                                    &mut particles,
                                    &mesh.tint_maps,
                                )?;
                            }
                            if aim.take().is_some() {
                                report(
                                    events,
                                    ClientEvent::Aim {
                                        aim: None,
                                        block: None,
                                    },
                                );
                            }
                            // The movement reports wait for the 0x08 the server
                            // follows every respawn with; it is the packet that
                            // places the fresh player, and the echo it answers
                            // with re-arms the reporters.
                            awaiting_respawn_position = true;
                            report(
                                events,
                                ClientEvent::Respawned {
                                    dimension: respawned,
                                    gamemode: respawn.gamemode,
                                },
                            );
                        }
                        PlayerAbilities::ID => {
                            let packet = decoded(id, PlayerAbilities::decode(body))?;
                            debug!(
                                flying = packet.flying,
                                allow_flying = packet.allow_flying,
                                creative = packet.creative,
                                "the server set the abilities"
                            );
                            // The packet's own flying value is taken in either
                            // direction (`NetHandlerPlayClient.handlePlayerAbilities`).
                            player.apply_abilities(Abilities {
                                flying: packet.flying,
                                allow_flying: packet.allow_flying,
                                creative: packet.creative,
                                invulnerable: packet.invulnerable,
                                fly_speed: packet.fly_speed,
                                walk_speed: packet.walk_speed,
                            });
                        }
                        PlayerPositionAndLook::ID => {
                            let teleport = decoded(id, PlayerPositionAndLook::decode(body))?;
                            // The correction is the placement a respawn was
                            // waiting for: the movement reports resume with
                            // this echo, and the gate clears before the
                            // reporters are re-armed below.
                            awaiting_respawn_position = false;
                            apply_correction(&mut player, &teleport);
                            // The echo below is the position report that reconciles the
                            // correction, so the walking reporters start from the corrected
                            // pose: the tick after the correction reports nothing of the
                            // pre-correction displacement, and the staleness clock measures
                            // from the report that was actually sent.
                            reset_reporters(&mut player);
                            let echo = payload_of(|out| {
                                write_player_position_and_look(
                                    out,
                                    player.position[0],
                                    player.position[1],
                                    player.position[2],
                                    player.yaw,
                                    player.pitch,
                                    false,
                                )
                            })?;
                            send_reply(&mut conn, &echo)?;
                            // The first S08 after the join or a
                            // world-rebuilding respawn clears the terrain
                            // screen (`handlePlayerPosLook:719-726`); later
                            // S08 packets are ordinary corrections.
                            if terrain_pending {
                                report(events, ClientEvent::TerrainReady);
                                terrain_pending = false;
                            }
                            // The correction settles at once: the window resets its interpolation
                            // on a snapped tick rather than sliding to the pose.
                            report(events, player_tick(&player, true));
                            // Every `PlayerTick` is paired with its feed, the
                            // correction's snapped report included: the window
                            // reads its entities from the very next event.
                            report(
                                events,
                                entities_tick(&entities, world.as_ref(), &player_list, &board),
                            );
                            report(events, eye_block_event(&player, world.as_ref()));
                            // The view block moved: the sky's colour is sampled at the player's own
                            // block, so a correction can change it without a new clock.
                            report_sky(
                                world.as_ref(),
                                player.position,
                                clock.as_ref(),
                                &weather,
                                events,
                            );
                        }
                        ChangeGameState::ID => {
                            let change = decoded(id, decode_change_game_state(body))?;
                            // Reason 3 is the game-mode change; the value maps
                            // as the source's handler maps it: floored and
                            // matched against the enum, everything unknown
                            // answering survival
                            // (`NetHandlerPlayClient.java:1364`, `:1383`).
                            if change.reason == 3 {
                                // The mode the reach and the dig machine's creative
                                // branch read. Both read it live — the aim recomputes
                                // against this byte each tick, and the dig step takes
                                // it per step — so the next recompute already uses the
                                // new reach (`PlayerControllerMP.java:344-346`).
                                gamemode = mode_from_value(change.value);
                                debug!(
                                    gamemode,
                                    value = change.value,
                                    "the server changed the game mode"
                                );
                                report(events, ClientEvent::GameType { gamemode });
                            } else if change.reason == 1 {
                                // The rain eases in: its target rises to
                                // full (the task's weather interface; the
                                // 1/2 begin/end mapping note rides the task
                                // report).
                                weather.target_rain = 1.0;
                            } else if change.reason == 2 {
                                // The rain eases out: its target falls to
                                // clear (same note as reason 1).
                                weather.target_rain = 0.0;
                            } else if change.reason == 7 {
                                // The rain fade: the strength heads for the
                                // value (`:1414-1417`).
                                weather.target_rain = change.value.clamp(0.0, 1.0);
                            } else if change.reason == 8 {
                                // The thunder fade: the strength heads for the
                                // value (`:1418-1421`).
                                weather.target_thunder = change.value.clamp(0.0, 1.0);
                            } else if change.reason == 6 {
                                // The landed attack (`:1411-1413`): the hit
                                // plays at the player's eye, fixed quiet
                                // and low.
                                report(
                                    events,
                                    ClientEvent::LocalSound {
                                        name: SUCCESSFUL_HIT_SOUND.to_string(),
                                        pos: [
                                            player.position[0] as f32,
                                            player.position[1] as f32 + player.eye_height() as f32,
                                            player.position[2] as f32,
                                        ],
                                        volume: 0.18,
                                        pitch: 0.45,
                                        category_hint: SoundCategory::Players,
                                    },
                                );
                            } else if change.reason == 10 {
                                // The guardian's curse (`:1423-1426`): the
                                // name at the player, full and level, plus
                                // the mob-appearance particle the same arm
                                // spawns — reported like a 0x2A arrival and
                                // pooled the same way when a world stands.
                                let at = [
                                    player.position[0] as f32,
                                    player.position[1] as f32,
                                    player.position[2] as f32,
                                ];
                                report(
                                    events,
                                    ClientEvent::LocalSound {
                                        name: GUARDIAN_CURSE_SOUND.to_string(),
                                        pos: at,
                                        volume: 1.0,
                                        pitch: 1.0,
                                        category_hint: SoundCategory::Mobs,
                                    },
                                );
                                report(
                                    events,
                                    ClientEvent::Particle {
                                        particle_id: GUARDIAN_CURSE_PARTICLE_ID,
                                        long_distance: false,
                                        pos: at,
                                        offset: [0.0, 0.0, 0.0],
                                        speed: 0.0,
                                        count: 1,
                                        data: Vec::new(),
                                    },
                                );
                                if let Some(world) = world.as_ref() {
                                    particles.spawn_wire(
                                        GUARDIAN_CURSE_PARTICLE_ID,
                                        false,
                                        at,
                                        [0.0, 0.0, 0.0],
                                        0.0,
                                        1,
                                        &[],
                                        at,
                                        &SessionParticleWorld::new(world),
                                    );
                                }
                            } else {
                                // Reason 9 has no handler and anything above
                                // 10 the source ignores
                                // (`:1358-1427`) — the decoder keeps the raw
                                // reason and the session does the same.
                                debug!(
                                    reason = change.reason,
                                    "ignoring a game-state change with no handler"
                                );
                            }
                        }
                        SoundEffect::ID => {
                            let sound = decoded(id, decode_sound_effect(body))?;
                            // One event per packet; the audio engine resolves
                            // the name.
                            report(
                                events,
                                ClientEvent::SoundEffect {
                                    name: sound.name,
                                    pos: [sound.x, sound.y, sound.z],
                                    volume: sound.volume,
                                    pitch: sound.pitch,
                                },
                            );
                        }
                        Particle::ID => {
                            let particle = decoded(id, decode_particle(body))?;
                            // One event per packet; the particle engine spawns
                            // from it.
                            report(
                                events,
                                ClientEvent::Particle {
                                    particle_id: particle.particle_id,
                                    long_distance: particle.long_distance,
                                    pos: [particle.x, particle.y, particle.z],
                                    offset: [
                                        particle.offset_x,
                                        particle.offset_y,
                                        particle.offset_z,
                                    ],
                                    speed: particle.speed,
                                    count: particle.count,
                                    data: particle.data.clone(),
                                },
                            );
                            // The pool spawns from the same packet, through
                            // the setting reduction and past the range cull
                            // from the viewer's feet. Without a world there
                            // is no light or ground to step against, so the
                            // spawn waits for the columns to arrive.
                            if let Some(world) = world.as_ref() {
                                let viewer = [
                                    player.position[0] as f32,
                                    player.position[1] as f32,
                                    player.position[2] as f32,
                                ];
                                particles.spawn_wire(
                                    particle.particle_id,
                                    particle.long_distance,
                                    [particle.x, particle.y, particle.z],
                                    [particle.offset_x, particle.offset_y, particle.offset_z],
                                    particle.speed,
                                    particle.count,
                                    &particle.data,
                                    viewer,
                                    &SessionParticleWorld::new(world),
                                );
                            }
                        }
                        TimeUpdate::ID => {
                            let update = decoded(id, TimeUpdate::decode(body))?;
                            // The frozen-sun convention: a stopped day-night cycle puts a negative
                            // time on the wire (`S03PacketTimeUpdate.java:17-31`), and the client
                            // negates it back before it becomes the world clock
                            // (`WorldClient.setWorldTime`, `WorldClient.java:468-481`), so the
                            // clock — and the sky built from it — is the value vanilla renders.
                            // The sign the negation was read from is the source's own frozen-daylight
                            // rule: `setWorldTime` stops the cycle for a negative time, and
                            // `WorldClient.tick` then holds the time of day (`:71-74`).
                            let time_of_day = received_time_of_day(update.time_of_day);
                            clock = Some(Clock {
                                world_age: update.world_age,
                                time_of_day,
                                frozen: update.time_of_day < 0,
                            });
                            report(
                                events,
                                ClientEvent::Time {
                                    world_age: update.world_age,
                                    time_of_day,
                                },
                            );
                            report_sky(
                                world.as_ref(),
                                player.position,
                                clock.as_ref(),
                                &weather,
                                events,
                            );
                        }
                        ChunkData::ID => match world.as_mut() {
                            Some(store) => {
                                let column = decoded(id, ChunkData::decode(body, store.has_sky()))?;
                                check_column_coordinate(column.chunk_x, column.chunk_z)?;
                                if store.apply_chunk_data(&column) {
                                    mark_column_changed(
                                        store,
                                        &mut queue,
                                        column.chunk_x,
                                        column.chunk_z,
                                    );
                                    refresh_sign_column(
                                        store,
                                        &mut signs,
                                        events,
                                        column.chunk_x,
                                        column.chunk_z,
                                    );
                                } else {
                                    queue.mark_column_unloaded(column.chunk_x, column.chunk_z);
                                    drop_sign_column(
                                        &mut signs,
                                        events,
                                        column.chunk_x,
                                        column.chunk_z,
                                    );
                                    report(
                                        events,
                                        ClientEvent::ChunkUnloaded {
                                            cx: column.chunk_x,
                                            cz: column.chunk_z,
                                        },
                                    );
                                }
                            }
                            None => warn!("a column arrived before Join Game built the world"),
                        },
                        MapChunkBulk::ID => match world.as_mut() {
                            Some(store) => {
                                let bulk = decoded(id, MapChunkBulk::decode(body))?;
                                for column in &bulk.columns {
                                    check_column_coordinate(column.chunk_x, column.chunk_z)?;
                                }
                                store.apply_bulk(&bulk);
                                for column in &bulk.columns {
                                    if store.chunk(column.chunk_x, column.chunk_z).is_some() {
                                        mark_column_changed(
                                            store,
                                            &mut queue,
                                            column.chunk_x,
                                            column.chunk_z,
                                        );
                                        refresh_sign_column(
                                            store,
                                            &mut signs,
                                            events,
                                            column.chunk_x,
                                            column.chunk_z,
                                        );
                                    } else {
                                        queue.mark_column_unloaded(column.chunk_x, column.chunk_z);
                                        drop_sign_column(
                                            &mut signs,
                                            events,
                                            column.chunk_x,
                                            column.chunk_z,
                                        );
                                        report(
                                            events,
                                            ClientEvent::ChunkUnloaded {
                                                cx: column.chunk_x,
                                                cz: column.chunk_z,
                                            },
                                        );
                                    }
                                }
                            }
                            None => warn!("a bulk column arrived before Join Game built the world"),
                        },
                        BlockChange::ID => match world.as_mut() {
                            Some(store) => {
                                let change = decoded(id, BlockChange::decode(body))?;
                                apply_block_change(
                                    store,
                                    &mut queue,
                                    change.x,
                                    change.y,
                                    change.z,
                                    change.value,
                                );
                                refresh_sign_at(
                                    store, &mut signs, events, change.x, change.y, change.z,
                                );
                            }
                            None => {
                                warn!("a block change arrived before Join Game built the world")
                            }
                        },
                        MultiBlockChange::ID => match world.as_mut() {
                            Some(store) => {
                                let change = decoded(id, MultiBlockChange::decode(body))?;
                                apply_multi_block_change(
                                    store,
                                    &mut queue,
                                    change.chunk_x,
                                    change.chunk_z,
                                    &change.updates,
                                )?;
                                refresh_sign_column(
                                    store,
                                    &mut signs,
                                    events,
                                    change.chunk_x,
                                    change.chunk_z,
                                );
                            }
                            None => {
                                warn!(
                                    "a multi block change arrived before Join Game built the world"
                                )
                            }
                        },
                        BlockBreakAnimation::ID => {
                            let animation = decoded(id, BlockBreakAnimation::decode(body))?;
                            // The stage map's own rule, from the packet's
                            // reader through `RenderGlobal.sendBlockBreakProgress`
                            // (`:2364-2380`): 0..=9 sets, anything else removes,
                            // and both key by the breaker id. A breaker the
                            // entity store does not know is logged and dropped:
                            // the source's 0x25 path carries no entity lookup
                            // (`NetHandlerPlayClient.handleBlockBreakAnim`,
                            // `:1331-1335`), so the filter is this client's
                            // own, following the entity-keyed handlers'
                            // null-guard convention (`handleEntityVelocity`:501-510;
                            // `handleAnimation`:867-872). The
                            // server never echoes the client's own id
                            // (`WorldManager.java`:96-113 excludes the breaker),
                            // so the own dig never arrives here.
                            if entities.get(animation.entity_id).is_none() {
                                warn!(
                                    breaker = animation.entity_id,
                                    "a block break animation for an unknown entity"
                                );
                            } else if animation.stage < 10 {
                                if stages.insert(
                                    animation.entity_id,
                                    [animation.x, animation.y, animation.z],
                                    animation.stage,
                                    stage_counter,
                                ) {
                                    report(
                                        events,
                                        ClientEvent::BreakStage {
                                            breaker: animation.entity_id,
                                            x: animation.x,
                                            y: animation.y,
                                            z: animation.z,
                                            stage: animation.stage,
                                        },
                                    );
                                }
                            } else if stages.clear(animation.entity_id) {
                                report(
                                    events,
                                    ClientEvent::BreakCleared {
                                        breaker: animation.entity_id,
                                        x: animation.x,
                                        y: animation.y,
                                        z: animation.z,
                                    },
                                );
                            }
                        }
                        EntityEquipment::ID => {
                            let equipment = decoded(id, entity::decode_entity_equipment(body))?;
                            entities.set_equipment(
                                equipment.entity_id,
                                equipment.slot,
                                equipment.item,
                            );
                        }
                        UseBed::ID => {
                            let bed = decoded(id, entity::decode_use_bed(body))?;
                            // The bed: `handleUseBed:903-907` runs `trySleep`
                            // on the named player, which sets `sleeping`
                            // with `sleepTimer = 0`
                            // (`EntityPlayer.java:1568-1571`). A repeated
                            // bed for the already sleeping player answers
                            // nothing, and a bed for another player answers
                            // nothing here.
                            if Some(bed.entity_id) == player.entity_id && !player.sleeping {
                                player.sleeping = true;
                                // The facing the camera's sleeping branch
                                // turns by: the head block's low two bits
                                // (`BlockBed.getDirectionFromMetadata`), south
                                // when the map holds no bed at the head.
                                let facing = world
                                    .as_ref()
                                    .map(|world| world.block(bed.x, bed.y, bed.z))
                                    .filter(|packed| block_id_of(*packed) == BED_BLOCK_ID)
                                    .map(|packed| block_meta_of(packed) & 3)
                                    .unwrap_or(0);
                                report(
                                    events,
                                    ClientEvent::Sleeping {
                                        x: bed.x,
                                        y: bed.y,
                                        z: bed.z,
                                        facing,
                                    },
                                );
                            }
                        }
                        Animation::ID => {
                            let animation = decoded(id, entity::decode_animation(body))?;
                            // The swing (byte 0) moves an arm the source's
                            // handler answers; byte 1 is the hurt the status
                            // packet already stages, byte 2 is the wake
                            // answered below, byte 3 is dropped on both
                            // sides, and the crits (bytes 4 and 5) run the
                            // shared emitter below
                            // (`NetHandlerPlayClient.handleAnimation:867-894`).
                            if animation.animation == 0 {
                                // The id names a tracked entity or nothing:
                                // the swing lands through the store's own
                                // lookup.
                                if let Some(entity) = entities.get_mut(animation.entity_id) {
                                    entity.swing();
                                }
                            } else if animation.animation == 2
                                && Some(animation.entity_id) == player.entity_id
                                && player.sleeping
                            {
                                // The wake: animation 2 on the sleeping own
                                // player (`handleAnimation:882-885` wakes it).
                                // A wake with no sleep behind it answers
                                // nothing.
                                player.sleeping = false;
                                report(events, ClientEvent::Woke);
                            } else if let Some(particle) =
                                crit_id_for_animation(animation.animation)
                            {
                                // The wire twin of the local crit path: three
                                // emitter rounds at the entity's mid-height —
                                // the constructor's own `onUpdate` plus one
                                // per tick until `age >= lifetime (3)`,
                                // ticked by `updateEffects:159-178`
                                // (`EntityParticleEmitter`) — through the
                                // engine's own dice, so the trigger spends
                                // three rounds' sixteen tries. The local
                                // attack path has no port yet
                                // (`EntityPlayerSP.onCriticalHit:671-674`
                                // awaits the attack action), so this arm is
                                // the only emitter.
                                match entities.get(animation.entity_id) {
                                    Some(entity) => match entity.kind.hitbox() {
                                        Some((width, height)) => {
                                            let centre = [
                                                entity.position[0],
                                                entity.position[1] + height / 2.0,
                                                entity.position[2],
                                            ];
                                            // The constructor round.
                                            let (_, spawns) = crit_burst(
                                                &mut particles,
                                                centre,
                                                width,
                                                height,
                                                particle,
                                            );
                                            for spawn in spawns {
                                                particles.spawn_local(spawn);
                                            }
                                            // The two tick rounds.
                                            for _ in 0..2 {
                                                let (_, spawns) = crit_burst(
                                                    &mut particles,
                                                    centre,
                                                    width,
                                                    height,
                                                    particle,
                                                );
                                                for spawn in spawns {
                                                    particles.spawn_local(spawn);
                                                }
                                            }
                                        }
                                        None => {
                                            debug!(
                                                entity_id = animation.entity_id,
                                                "a crit for an entity with no hitbox"
                                            );
                                        }
                                    },
                                    None => {
                                        debug!(
                                            entity_id = animation.entity_id,
                                            animation = animation.animation,
                                            "a crit for an unknown entity"
                                        );
                                    }
                                }
                            } else {
                                debug!(
                                    entity_id = animation.entity_id,
                                    animation = animation.animation,
                                    "an animation this client does not draw"
                                );
                            }
                        }
                        SpawnPlayer::ID => {
                            let spawn = decoded(id, entity::decode_spawn_player(body))?;
                            let mut entity = Entity::new(spawn.entity_id, EntityKind::Player);
                            entity.uuid = Some(spawn.uuid);
                            entity.position = [spawn.x, spawn.y, spawn.z];
                            entity.last_tick_position = entity.position;
                            entity.yaw = spawn.yaw;
                            entity.last_tick_yaw = spawn.yaw;
                            entity.pitch = spawn.pitch;
                            entity.last_tick_pitch = spawn.pitch;
                            // The head starts at the spawn's own yaw: the
                            // handler's fresh player stands at the packet's
                            // pose until a head-look packet names the head
                            // (`NetHandlerPlayClient.handleSpawnPlayer:539-541`).
                            entity.head_yaw = spawn.yaw;
                            entity.last_tick_head_yaw = spawn.yaw;
                            entities.insert(entity);
                        }
                        SpawnObject::ID => {
                            let spawn = decoded(id, entity::decode_spawn_object(body))?;
                            let mut entity =
                                Entity::new(spawn.entity_id, kind_from_object(spawn.kind));
                            entity.position = [spawn.x, spawn.y, spawn.z];
                            entity.last_tick_position = entity.position;
                            entity.pitch = spawn.pitch;
                            entity.last_tick_pitch = spawn.pitch;
                            entity.yaw = spawn.yaw;
                            entity.last_tick_yaw = spawn.yaw;
                            entity.velocity = spawn.velocity;
                            entity.data = kind_data_of(spawn.kind, spawn.data);
                            entities.insert(entity);
                        }
                        SpawnMob::ID => {
                            let spawn = decoded(id, entity::decode_spawn_mob(body))?;
                            let mut entity =
                                Entity::new(spawn.entity_id, kind_from_mob(spawn.kind));
                            entity.position = [spawn.x, spawn.y, spawn.z];
                            entity.last_tick_position = entity.position;
                            entity.yaw = spawn.yaw;
                            entity.last_tick_yaw = spawn.yaw;
                            entity.pitch = spawn.pitch;
                            entity.last_tick_pitch = spawn.pitch;
                            entity.head_yaw = spawn.head_yaw;
                            entity.last_tick_head_yaw = spawn.head_yaw;
                            // The body's render offset starts where the head
                            // does (`NetHandlerPlayClient.handleSpawnMob:925`).
                            entity.render_yaw_offset = spawn.head_yaw;
                            entity.prev_render_yaw_offset = spawn.head_yaw;
                            entity.velocity = spawn.velocity;
                            entity.metadata = spawn.metadata;
                            entities.insert(entity);
                        }
                        SpawnPainting::ID => {
                            let spawn = decoded(id, entity::decode_spawn_painting(body))?;
                            let mut entity = Entity::new(spawn.entity_id, EntityKind::Painting);
                            entity.position =
                                [f64::from(spawn.x), f64::from(spawn.y), f64::from(spawn.z)];
                            entity.last_tick_position = entity.position;
                            // The source reduces the wire's direction byte
                            // modulo 4 when it resolves the face
                            // (`EnumFacing.getHorizontal:273-276`, called from
                            // `S10PacketSpawnPainting.readPacketData:38`).
                            entity.data = KindData::Painting {
                                title: Arc::from(spawn.title.as_str()),
                                facing: spawn.facing & 0x03,
                            };
                            entities.insert(entity);
                        }
                        SpawnXpOrb::ID => {
                            let spawn = decoded(id, entity::decode_spawn_xp_orb(body))?;
                            let mut entity = Entity::new(spawn.entity_id, EntityKind::XpOrb);
                            entity.position = [spawn.x, spawn.y, spawn.z];
                            entity.last_tick_position = entity.position;
                            entity.data = KindData::XpOrb { count: spawn.count };
                            entities.insert(entity);
                        }
                        SpawnGlobalEntity::ID => {
                            let spawn = decoded(id, decode_spawn_global_entity(body))?;
                            let mut entity = Entity::new(spawn.entity_id, EntityKind::Global);
                            entity.position = [
                                entity::read_fixed_point(spawn.x),
                                entity::read_fixed_point(spawn.y),
                                entity::read_fixed_point(spawn.z),
                            ];
                            entity.last_tick_position = entity.position;
                            entities.insert(entity);
                            // One event per spawn with the raw kind and the
                            // raw fixed-point position: kind 1 is lightning
                            // and any other kind keeps the record and draws
                            // nothing
                            // (`NetHandlerPlayClient.handleSpawnGlobalEntity:463-486`).
                            report(
                                events,
                                ClientEvent::GlobalEntity {
                                    entity_id: spawn.entity_id,
                                    kind: spawn.kind,
                                    pos: [spawn.x, spawn.y, spawn.z],
                                },
                            );
                        }
                        CollectItem::ID => {
                            let collect = decoded(id, entity::decode_collect_item(body))?;
                            // The pickup sound: orbs orb and everything else
                            // pops, at the drop's own position with the
                            // pickup spread
                            // (`NetHandlerPlayClient.handleCollectItem:819-844`).
                            // The lookup runs ahead of the removal, which is
                            // what carries the position away.
                            if let Some(drop) = entities.get(collect.collected) {
                                let name = if drop.kind == EntityKind::XpOrb {
                                    ORB_SOUND
                                } else {
                                    PICKUP_SOUND
                                };
                                report(
                                    events,
                                    ClientEvent::LocalSound {
                                        name: name.to_string(),
                                        pos: [
                                            drop.position[0] as f32,
                                            drop.position[1] as f32,
                                            drop.position[2] as f32,
                                        ],
                                        volume: 0.2,
                                        pitch: pickup_pitch(sound_rng.random(), sound_rng.random()),
                                        category_hint: SoundCategory::Players,
                                    },
                                );
                            }
                            // The collected drop leaves the world
                            // (`NetHandlerPlayClient.handleCollectItem:819-844`).
                            entities.remove(&[collect.collected]);
                        }
                        EntityVelocity::ID => {
                            let velocity = decoded(id, entity::decode_entity_velocity(body))?;
                            entities.apply_velocity(velocity.entity_id, velocity.velocity);
                        }
                        DestroyEntities::ID => {
                            let destroy = decoded(id, entity::decode_destroy_entities(body))?;
                            entities.remove(&destroy.entity_ids);
                        }
                        entity::Entity::ID => {
                            // The packet names an entity and nothing else
                            // (`S14PacketEntity.readPacketData:33-36`); the
                            // source's handler applies its zero deltas as a
                            // position resync and the on-ground bit
                            // (`NetHandlerPlayClient.handleEntityMovement:613-631`).
                            // The session reads the id and drops it.
                            let packet = decoded(id, entity::decode_entity(body))?;
                            debug!(entity_id = packet.entity_id, "ignoring the entity packet");
                        }
                        EntityRelativeMove::ID => {
                            let movement = decoded(id, entity::decode_entity_relative_move(body))?;
                            entities.apply_relative_move(movement.entity_id, movement.delta);
                        }
                        EntityLook::ID => {
                            let look = decoded(id, entity::decode_entity_look(body))?;
                            entities.apply_look(look.entity_id, look.yaw, look.pitch);
                        }
                        EntityLookAndRelativeMove::ID => {
                            let both =
                                decoded(id, entity::decode_entity_look_and_relative_move(body))?;
                            entities.apply_relative_move(both.entity_id, both.delta);
                            entities.apply_look(both.entity_id, both.yaw, both.pitch);
                        }
                        EntityTeleport::ID => {
                            let teleport = decoded(id, entity::decode_entity_teleport(body))?;
                            entities.apply_teleport(
                                teleport.entity_id,
                                [teleport.x, teleport.y, teleport.z],
                                teleport.yaw,
                                teleport.pitch,
                                teleport.on_ground,
                            );
                        }
                        EntityHeadLook::ID => {
                            let head = decoded(id, entity::decode_entity_head_look(body))?;
                            entities.apply_head_look(head.entity_id, head.head_yaw);
                        }
                        AttachEntity::ID => {
                            let attach = decoded(id, entity::decode_attach_entity(body))?;
                            entities.set_attachment(attach.attached, attach.holder, attach.leash);
                        }
                        EntityMetadata::ID => {
                            let metadata = decoded(id, entity::decode_entity_metadata(body))?;
                            // The player's own block carries the air supply at
                            // index 1 (`Entity.java:286-287`); the own-id path
                            // reads it the way the status branch above reads
                            // the hurt flash, and the store takes the block as
                            // it stands.
                            if Some(metadata.entity_id) == player.entity_id {
                                for (index, value) in &metadata.metadata.entries {
                                    if *index == AIR_METADATA_INDEX {
                                        if let MetadataValue::Short(air) = value {
                                            report(events, ClientEvent::Air { air: *air });
                                        }
                                    }
                                    if *index == ABSORPTION_METADATA_INDEX {
                                        if let MetadataValue::Float(amount) = value {
                                            report(
                                                events,
                                                ClientEvent::Absorption { amount: *amount },
                                            );
                                        }
                                    }
                                    if *index == SCORE_METADATA_INDEX {
                                        if let MetadataValue::Int(score) = value {
                                            report(events, ClientEvent::Score { score: *score });
                                        }
                                    }
                                }
                            }
                            entities.apply_metadata(metadata.entity_id, metadata.metadata);
                        }
                        PlayerListItem::ID => {
                            // The decoder reads all five actions; each lands in
                            // the list's own merge rules below — an add
                            // replaces the record, the gamemode, latency and
                            // display-name updates reach the record they name,
                            // and a remove drops it. An update naming no
                            // record is refused by the list and logged.
                            let list = decoded(id, PlayerListItem::decode(body))?;
                            let before = player_list.clone();
                            match list.action {
                                PlayerListItem::ACTION_ADD => {
                                    for entry in list.entries {
                                        let Some(name) = entry.name else {
                                            continue;
                                        };
                                        player_list.insert(
                                            entry.uuid,
                                            PlayerListRecord {
                                                uuid: hyphenated(&entry.uuid),
                                                name,
                                                properties: entry.properties,
                                                // The wire's gamemode runs through
                                                // the same conversion the reach
                                                // reads; an unknown number is
                                                // survival there and here.
                                                gamemode: mode_from_value(
                                                    entry.gamemode.unwrap_or(0) as f32,
                                                ),
                                                latency: entry.ping.unwrap_or(0),
                                                display_name: entry.display_name,
                                            },
                                        );
                                    }
                                }
                                PlayerListItem::ACTION_UPDATE_GAME_MODE => {
                                    for entry in list.entries {
                                        if let Some(gamemode) = entry.gamemode {
                                            if !player_list.set_gamemode(
                                                entry.uuid,
                                                mode_from_value(gamemode as f32),
                                            ) {
                                                debug!(
                                                    uuid = ?entry.uuid,
                                                    "a gamemode update for a player the list does not hold"
                                                );
                                            }
                                        }
                                    }
                                }
                                PlayerListItem::ACTION_UPDATE_LATENCY => {
                                    for entry in list.entries {
                                        if let Some(latency) = entry.ping {
                                            if !player_list.set_latency(entry.uuid, latency) {
                                                debug!(
                                                    uuid = ?entry.uuid,
                                                    "a latency update for a player the list does not hold"
                                                );
                                            }
                                        }
                                    }
                                }
                                PlayerListItem::ACTION_UPDATE_DISPLAY_NAME => {
                                    for entry in list.entries {
                                        if !player_list
                                            .set_display_name(entry.uuid, entry.display_name)
                                        {
                                            debug!(
                                                uuid = ?entry.uuid,
                                                "a display-name update for a player the list does not hold"
                                            );
                                        }
                                    }
                                }
                                PlayerListItem::ACTION_REMOVE => {
                                    for entry in list.entries {
                                        player_list.remove(entry.uuid);
                                    }
                                }
                                _ => {}
                            }
                            report_list_change(&player_list, &before, events);
                        }
                        ScoreboardObjective::ID => {
                            let objective = decoded(id, ScoreboardObjective::decode(body))?;
                            let before = board.clone();
                            if objective.mode == ScoreboardObjective::MODE_REMOVE {
                                board.remove_objective(&objective.name);
                            } else {
                                // The create and update modes both carry the
                                // whole info block and both write it over the
                                // objective's name (`handleScoreboardObjective`,
                                // `NetHandlerPlayClient.java:1874-1892`); the
                                // decoder fills the two fields for exactly
                                // those modes.
                                board.set_objective(
                                    &objective.name,
                                    objective.value.as_deref().unwrap_or_default(),
                                    objective.kind.as_deref().unwrap_or_default(),
                                );
                            }
                            report_board_change(&board, &before, events);
                        }
                        ScoreboardScore::ID => {
                            let score = decoded(id, ScoreboardScore::decode(body))?;
                            let before = board.clone();
                            match (score.mode, score.objective.as_deref()) {
                                // The set mode carries the value (the decoder
                                // fills it for that mode alone).
                                (ScoreboardScore::MODE_SET, Some(objective)) => {
                                    board.set_score(
                                        &score.entry,
                                        objective,
                                        score.value.unwrap_or(0),
                                    );
                                }
                                // The remove mode drops one objective's
                                // score, or — the empty name — every
                                // objective of the entry
                                // (`handleUpdateScore`, `:1910-1919`).
                                (ScoreboardScore::MODE_REMOVE, Some(objective)) => {
                                    board.remove_score(&score.entry, objective);
                                }
                                (ScoreboardScore::MODE_REMOVE, None) => {
                                    board.remove_scores(&score.entry);
                                }
                                // A set naming no objective writes nowhere.
                                (_, None) => {}
                                // The decoder admits only the two modes.
                                _ => {}
                            }
                            report_board_change(&board, &before, events);
                        }
                        ScoreboardDisplay::ID => {
                            let display = decoded(id, ScoreboardDisplay::decode(body))?;
                            let before = board.clone();
                            // The empty name clears the slot; anything else
                            // names the objective it shows
                            // (`handleDisplayScoreboard`, `:1932-1939`).
                            board.set_display(display.slot as usize, display.objective.as_deref());
                            report_board_change(&board, &before, events);
                        }
                        ScoreboardTeam::ID => {
                            let team = decoded(id, ScoreboardTeam::decode(body))?;
                            let before = board.clone();
                            match team.mode {
                                ScoreboardTeam::MODE_CREATE | ScoreboardTeam::MODE_UPDATE => {
                                    // Both modes write the whole info block
                                    // (`handleTeams:1962-1975`), and the
                                    // colour byte runs through the sentinel's
                                    // own reading. The create replaces the
                                    // team registration; the update rewrites
                                    // the one it names and keeps its players.
                                    if team.mode == ScoreboardTeam::MODE_CREATE {
                                        board.remove_team(&team.name);
                                    }
                                    board.set_team(
                                        &team.name,
                                        team.display_name.as_deref().unwrap_or_default(),
                                        team.prefix.as_deref().unwrap_or_default(),
                                        team.suffix.as_deref().unwrap_or_default(),
                                        team.friendly_flags.unwrap_or(0),
                                        team.name_tag_visibility.as_deref().unwrap_or_default(),
                                        team.colour.and_then(colour_from_wire),
                                    );
                                    if let Some(players) = team.players.as_deref() {
                                        board.add_team_players(&team.name, players);
                                    }
                                }
                                ScoreboardTeam::MODE_ADD_PLAYERS => {
                                    if let Some(players) = team.players.as_deref() {
                                        board.add_team_players(&team.name, players);
                                    }
                                }
                                ScoreboardTeam::MODE_REMOVE_PLAYERS => {
                                    if let Some(players) = team.players.as_deref() {
                                        board.remove_team_players(&team.name, players);
                                    }
                                }
                                // The remove mode: the team and every
                                // membership with it (`handleTeams:1993-1996`).
                                _ => board.remove_team(&team.name),
                            }
                            report_board_change(&board, &before, events);
                        }
                        TabHeaderFooter::ID => {
                            let text = decoded(id, TabHeaderFooter::decode(body))?;
                            let next = (text.header, text.footer);
                            if tab_text.as_ref() != Some(&next) {
                                report(
                                    events,
                                    ClientEvent::TabText {
                                        header: next.0.clone(),
                                        footer: next.1.clone(),
                                    },
                                );
                            }
                            tab_text = Some(next);
                        }
                        OpenWindow::ID => {
                            let open = decoded(id, OpenWindow::decode(body))?;
                            // The server opens at most one window at a time;
                            // the fresh container replaces whatever stood
                            // (`handleOpenWindow:1092-1120`), and the slot
                            // count names the vector its Window Items will
                            // fill.
                            windows.apply_open(OpenWindowState {
                                window_id: open.window_id,
                                kind: open.kind,
                                title: open.title.clone(),
                                slots: vec![None; usize::from(open.slot_count)],
                                properties: Vec::new(),
                                entity_id: open.entity_id,
                            });
                            report(
                                events,
                                ClientEvent::WindowOpened {
                                    window_id: open.window_id,
                                    kind: open.kind,
                                    title: open.title,
                                    slot_count: open.slot_count,
                                    entity_id: open.entity_id,
                                },
                            );
                        }
                        CloseWindow::ID => {
                            let close = decoded(id, CloseWindow::decode(body))?;
                            // Only a close naming the live open window changes
                            // anything: it clears the window and drops the
                            // carried stack with it
                            // (`handleCloseWindow:1311-1315`).
                            if windows.apply_close(close.window_id) {
                                report(
                                    events,
                                    ClientEvent::WindowClosed {
                                        window_id: close.window_id,
                                    },
                                );
                            }
                        }
                        SetSlot::ID => {
                            let slot = decoded(id, SetSlot::decode(body))?;
                            // The state's own branches: window −1 is the
                            // carried stack, window 0 the player's layout and
                            // the rest the open window's
                            // (`handleSetSlot:1133-1168`). The cursor belongs
                            // to the shared player inventory every open
                            // screen draws (`GuiContainer.drawScreen:144-170`
                            // draws `inventoryplayer.getItemStack()`), so its
                            // write republishes the open window too — window
                            // 0's snapshot alone never reaches the standing
                            // screen.
                            if windows.apply_set_slot(slot.window_id, slot.slot, slot.item) {
                                report_window_snapshot(
                                    &windows,
                                    slot_window(slot.window_id),
                                    events,
                                );
                                if slot.window_id == CURSOR_WINDOW_ID {
                                    if let Some(open) =
                                        windows.open.as_ref().map(|open| open.window_id)
                                    {
                                        report_window_snapshot(&windows, open, events);
                                    }
                                }
                            }
                        }
                        WindowItems::ID => {
                            let items = decoded(id, WindowItems::decode(body))?;
                            if windows.apply_window_items(items.window_id, items.slots) {
                                report_window_snapshot(&windows, items.window_id, events);
                            }
                        }
                        WindowProperty::ID => {
                            let property = decoded(id, WindowProperty::decode(body))?;
                            if windows.apply_property(
                                property.window_id,
                                property.property,
                                property.value,
                            ) {
                                report_window_snapshot(&windows, property.window_id, events);
                            }
                        }
                        ConfirmTransaction::ID => {
                            let confirm = decoded(id, ConfirmTransaction::decode(body))?;
                            // The source's own guard
                            // (`handleConfirmTransaction:1174-1193`): a
                            // rejected transaction for a window the session
                            // holds — window 0, or the live open window — is
                            // answered in the same pass with the packet's own
                            // action and the accepted flag set. The accepted
                            // case and a window the state does not hold answer
                            // nothing.
                            if !confirm.accepted && windows.holds_window(confirm.window_id) {
                                let reply = payload_of(|out| {
                                    write_confirm_transaction(
                                        out,
                                        confirm.window_id,
                                        confirm.action,
                                        true,
                                    )
                                })?;
                                send_reply(&mut conn, &reply)?;
                            }
                        }
                        HELD_ITEM_CHANGE_ID => {
                            let slot = decoded(id, decode_held_item_change(body))?;
                            // The source's own guard
                            // (`handleHeldItemChange:598-606`): a slot inside
                            // the hotbar's nine selects it, and anything else
                            // is ignored.
                            if (0..HOTBAR_SIZE as i16).contains(&slot) {
                                windows.player.inventory.set_selected(slot);
                                report(events, ClientEvent::HeldItemSlot { slot });
                            } else {
                                debug!(
                                    slot = slot,
                                    "a held item change outside the hotbar is ignored"
                                );
                            }
                        }
                        SetExperience::ID => {
                            let experience = decoded(id, SetExperience::decode(body))?;
                            // The level-up sound: a gain that lands on five
                            // past the cooldown, staged at the player
                            // (`EntityPlayer.addExperienceLevel:2037-2054`).
                            // The level tracks every packet; the gate reads
                            // the session tick against the last sound.
                            if level_sounds(
                                xp_level,
                                experience.level,
                                player.tick,
                                last_xp_sound_tick,
                            ) {
                                last_xp_sound_tick = player.tick;
                                report(
                                    events,
                                    ClientEvent::LocalSound {
                                        name: LEVEL_SOUND.to_string(),
                                        pos: [
                                            player.position[0] as f32,
                                            player.position[1] as f32,
                                            player.position[2] as f32,
                                        ],
                                        volume: level_volume(experience.level),
                                        pitch: 1.0,
                                        category_hint: SoundCategory::Players,
                                    },
                                );
                            }
                            xp_level = experience.level;
                            report(
                                events,
                                ClientEvent::Experience {
                                    bar: experience.bar,
                                    level: experience.level,
                                    total: experience.total,
                                },
                            );
                        }
                        EntityEffect::ID => {
                            let effect = decoded(id, EntityEffect::decode(body))?;
                            // The store is the own player's alone
                            // (`handleEntityEffect:1502-1513`): a fresh id is
                            // held as it arrived, and one already held
                            // combines (`EntityLivingBase.addPotionEffect:745-760`
                            // reaching `PotionEffect.combine:63-85`).
                            if Some(effect.entity_id) == player.entity_id {
                                match effects.get_mut(&effect.effect_id) {
                                    Some(held) => {
                                        combine_effect(held, effect.amplifier, effect.duration)
                                    }
                                    None => {
                                        effects.insert(
                                            effect.effect_id,
                                            StatusEffect {
                                                effect_id: effect.effect_id,
                                                amplifier: effect.amplifier,
                                                duration: effect.duration,
                                            },
                                        );
                                    }
                                }
                                report(
                                    events,
                                    ClientEvent::Effects {
                                        effects: effect_list(&effects),
                                    },
                                );
                            }
                        }
                        RemoveEntityEffect::ID => {
                            let remove = decoded(id, RemoveEntityEffect::decode(body))?;
                            // A removal for the own id drops the effect the id
                            // names — `removePotionEffectClient:788-791` is a
                            // plain map removal — and reports the store only
                            // when something left it.
                            if Some(remove.entity_id) == player.entity_id
                                && effects.remove(&remove.effect_id).is_some()
                            {
                                report(
                                    events,
                                    ClientEvent::Effects {
                                        effects: effect_list(&effects),
                                    },
                                );
                            }
                        }
                        UpdateSign::ID => {
                            let sign = decoded(id, UpdateSign::decode(body))?;
                            // The last text per position is the view's to
                            // draw; the lines travel as sent
                            // (`handleUpdateSign:1234-1261`). The block the
                            // store holds rides along for the board's
                            // variant and orientation — air when the text
                            // arrived before its chunk.
                            let packed = world
                                .as_ref()
                                .map(|store| store.block(sign.x, sign.y, sign.z))
                                .unwrap_or(0);
                            let (block_id, metadata) = (block_id_of(packed), block_meta_of(packed));
                            signs.insert(
                                (sign.x, sign.y, sign.z),
                                SignText {
                                    lines: sign.lines.clone(),
                                    block_id,
                                    metadata,
                                },
                            );
                            report(
                                events,
                                ClientEvent::SignTextChanged {
                                    x: sign.x,
                                    y: sign.y,
                                    z: sign.z,
                                    lines: sign.lines,
                                    block_id,
                                    metadata,
                                },
                            );
                        }
                        SignEditorOpen::ID => {
                            let editor = decoded(id, SignEditorOpen::decode(body))?;
                            report(
                                events,
                                ClientEvent::SignEditorOpen {
                                    x: editor.x,
                                    y: editor.y,
                                    z: editor.z,
                                },
                            );
                        }
                        PluginMessage::ID => {
                            let message = decoded(id, PluginMessage::decode(body))?;
                            if message.channel == TRADE_LIST_CHANNEL {
                                let trade_list =
                                    decoded(id, MerchantOffers::decode(&message.data))?;
                                report(
                                    events,
                                    ClientEvent::MerchantOffers {
                                        offers: trade_list.offers,
                                    },
                                );
                            } else if message.channel == BOOK_OPEN_CHANNEL {
                                // The open carries no stack — the buffer is
                                // never read — so the held slot snapshots it:
                                // the own player's currently held item, opened
                                // only for the written-book identity
                                // (`NetHandlerPlayClient.handleCustomPayload:1855-1863`).
                                // Anything else is silently ignored.
                                if let Some(stack) = book_open_stack(
                                    &message.channel,
                                    windows.player.inventory.get_current_item(),
                                ) {
                                    report(events, ClientEvent::BookOpen { stack });
                                } else {
                                    debug!(
                                        channel = %message.channel,
                                        "a book open without a held written book is ignored"
                                    );
                                }
                            } else {
                                debug!(channel = %message.channel, bytes = message.data.len(), "plugin message");
                            }
                        }
                        PlayDisconnect::ID => {
                            let disconnect = decoded(id, PlayDisconnect::decode(body))?;
                            finish_meshes(
                                world.as_ref(),
                                &mut queue,
                                &pool,
                                &mesh,
                                &finished,
                                &results,
                                events,
                                graphics_fast,
                                smooth_lighting,
                            );
                            report(
                                events,
                                ClientEvent::Disconnected {
                                    reason: disconnect.reason,
                                },
                            );
                            return Ok(());
                        }
                        PLAY_SET_COMPRESSION_ID => {
                            warn!("ignoring a play-state Set Compression, which M1 does not apply");
                        }
                        other => {
                            debug!(
                                packet_id = other,
                                "skipping a play packet M1 does not handle"
                            );
                        }
                    }
                }
                Ok(RecvOutcome::Idle) => {
                    pump_meshes(
                        world.as_ref(),
                        &mut queue,
                        &pool,
                        &mesh,
                        &finished,
                        &results,
                        events,
                        graphics_fast,
                        smooth_lighting,
                    );
                }
                Err(error) if is_stream_end(&error) => {
                    debug!(error = %error, "the server closed the stream");
                    break;
                }
                Err(error) => return Err(SessionError::Frame(error)),
            }

            // The ticks that have come due. The frame above — or the idle wait
            // before this batch — was handled first, and the ticker caps a
            // catch-up, so a long pause cannot snowball into a long pass. A
            // tick's packets go out in the order the source sends them, each
            // as soon as the tick produced it.
            for _ in 0..ticker.due(Instant::now()) {
                // While dead the tick steps a neutral intent — the death
                // screen's own input state: the keys are unheld and the look
                // is not polled, so nothing the window does moves the player.
                // The movement model itself still runs, which is what lets the
                // body settle and the death clock advance.
                let dead = player.dead;
                let tick_intent = if dead { Intent::neutral() } else { intent };
                // The weather eases once per tick, before the step reads it: the sky's
                // re-report at the step's tail then carries the tick's own strengths, so a
                // frozen-clock scene still tracks the rain. The report below stays where it
                // was, behind the step's sounds, so the event stream keeps its order.
                let weather_moved = weather.tick();
                for payload in step_tick(
                    &mut player,
                    &tick_intent,
                    clock.as_mut(),
                    world.as_ref(),
                    &mut entities,
                    &player_list,
                    &board,
                    events,
                    !awaiting_respawn_position,
                    &weather,
                    weather_moved,
                ) {
                    send_reply(&mut conn, &payload)?;
                }
                // The step's own sounds: the splash on water entry and the
                // swim or footstep past the walk threshold, staged from the
                // move the step just ran. The edge it answers stages the
                // stride's own particles beside it.
                let entered_water = stage_step_sounds(
                    &player,
                    world.as_ref(),
                    &mut distance_walked,
                    &mut next_step_distance,
                    &mut was_in_water,
                    &mut sound_rng,
                    events,
                );
                stage_stride_particles(&player, world.as_ref(), entered_water, &mut particles);
                // The weather eases once per tick and reports while it moves:
                // one event per tick that changed a strength, none for a
                // settled tick. The ease ran before the step above, so this
                // reports the same movement the sky already re-reported on.
                if weather_moved {
                    report(
                        events,
                        ClientEvent::WeatherChanged {
                            rain: weather.rain,
                            thunder: weather.thunder,
                        },
                    );
                }
                // The particle pool steps once per tick beside the weather:
                // the engine ticks against the session's columns, the splash
                // sampler rains its drops into the pool, and the snapshot
                // and the columns ride to the window while either is live.
                // Without a world the pool holds still: there is no light or
                // ground to step against yet.
                particle_tick = particle_tick.wrapping_add(1);
                if let Some(world) = world.as_ref() {
                    particles.tick(&SessionParticleWorld::new(world));
                    let viewer_block = [
                        player.position[0].floor() as i32,
                        player.position[1].floor() as i32,
                        player.position[2].floor() as i32,
                    ];
                    for spawn in splash_spawns(
                        &mut splash_rng,
                        world,
                        viewer_block,
                        weather.rain,
                        !graphics_fast,
                        ParticleSetting::All,
                    ) {
                        particles.spawn_local(spawn);
                    }
                    let columns = weather_columns(
                        world,
                        player.position,
                        player.eye_height() as f32,
                        weather.rain,
                        !graphics_fast,
                    );
                    if !columns.is_empty() || columns_live {
                        columns_live = !columns.is_empty();
                        report(
                            events,
                            ClientEvent::WeatherColumns {
                                columns,
                                tick: particle_tick,
                            },
                        );
                    }
                }
                let frames = particles.snapshot();
                if !frames.is_empty() || particles_live {
                    particles_live = !frames.is_empty();
                    report(
                        events,
                        ClientEvent::Particles {
                            frames,
                            tick: particle_tick,
                            total: particles.stats().count,
                        },
                    );
                }
                // The aim follows the tick's own state: a step that moved the
                // player, and any world change behind it, is read here.
                update_aim(world.as_ref(), &player, gamemode, &mut aim, events);
                // The third-person occlusion reads the same state: in either
                // third-person mode the eight corner rays run from the pose
                // eye toward the wanted camera point, and only a changed
                // clipped distance is reported. First person resets the
                // read with no event.
                if camera_mode == 0 {
                    camera_clip = oxide_render::camera::THIRD_PERSON_DISTANCE;
                } else if let Some(world) = world.as_ref() {
                    let eye = [
                        player.position[0],
                        player.position[1] + player.eye_height(),
                        player.position[2],
                    ];
                    let clipped = camera_clip_distance(
                        &WorldView(world),
                        eye,
                        player.yaw,
                        player.pitch,
                        camera_mode == 2,
                        f64::from(oxide_render::camera::THIRD_PERSON_DISTANCE),
                    ) as f32;
                    if clipped != camera_clip {
                        camera_clip = clipped;
                        report(events, ClientEvent::CameraClip { distance: clipped });
                    }
                }
                // The hotbar's pop runs down with the player's own tick: the
                // source decrements every main-inventory stack once per tick
                // (`EntityPlayer.onLivingUpdate:617` through
                // `InventoryPlayer.decrementAnimations:352-361` and
                // `ItemStack.updateAnimation:486-491`). A tick that moved a
                // counter republishes window 0, which is where the counters
                // ride.
                if windows.tick_hotbar_pop() {
                    report_window_snapshot(&windows, 0, events);
                }
                // Nothing the window presses reaches the interaction while the
                // death view is up: a click is the respawn request then, and
                // the presses and the dig step run only while alive.
                if !dead {
                    // The use-entity presses run against that same aim before
                    // the placement presses: the source's `rightClickMouse`
                    // tries the `ENTITY` case first and an entity target
                    // consumes the click (`Minecraft.java:1583-1592`), so a
                    // press that became 0x02 never becomes 0x08.
                    step_use_entity(
                        &entities,
                        &player,
                        gamemode,
                        aim,
                        &mut right_presses,
                        dig.hitting(),
                        &mut conn,
                    )?;
                    // The placement presses run against that same aim, before the
                    // dig's step: the source's tick orders the use button's presses
                    // before `sendClickBlockToController` (`Minecraft.java:2153-2166`).
                    step_place(
                        world.as_mut(),
                        &mut queue,
                        aim,
                        windows.player.inventory.get_current_item(),
                        gamemode,
                        &mut right_presses,
                        dig.hitting(),
                        &mut conn,
                        events,
                    )?;
                    // The dig runs against that same aim, and its actions run in
                    // the order the machine produced them: the completion's finish
                    // goes out before the removal it predicts, and the local path
                    // waits on nothing (`PlayerControllerMP.java:324-325`).
                    for action in step_dig(
                        world.as_ref(),
                        gamemode,
                        &mut dig,
                        aim,
                        &mut left_presses,
                        left_held,
                    ) {
                        apply_dig_action(
                            &action,
                            world.as_mut(),
                            &mut queue,
                            &mut stages,
                            stage_counter,
                            player.entity_id,
                            &mut conn,
                            events,
                            &mut particles,
                            &mesh.tint_maps,
                        )?;
                    }
                }
                // The stages' own clock: the counter advances once per tick
                // and the sweep runs every twentieth, reporting the entries it
                // expired (`RenderGlobal.updateClouds`, `RenderGlobal.java:1138-1146`).
                stage_counter = stage_counter.wrapping_add(1);
                if stage_counter % 20 == 0 {
                    for (breaker, [x, y, z]) in stages.sweep(stage_counter) {
                        report(events, ClientEvent::BreakCleared { breaker, x, y, z });
                    }
                }
            }
        }
        finish_meshes(
            world.as_ref(),
            &mut queue,
            &pool,
            &mesh,
            &finished,
            &results,
            events,
            graphics_fast,
            smooth_lighting,
        );
        Ok(())
    }
}

impl Session<TcpStream> {
    /// Connects and runs; the entry point the client thread calls.
    ///
    /// The connection itself is [`Session::connect`]'s, so this runs a session
    /// that already exists, exactly as [`Session::run_over`] does, and takes
    /// the window's input the same way.
    pub fn run(
        self,
        events: &Sender<ClientEvent>,
        inputs: Receiver<InputEvent>,
    ) -> Result<(), SessionError> {
        self.run_over(events, inputs)
    }
}

/// How the login phase ended.
///
/// Public so the mock-server harness can settle the login on its own: the
/// happy path returns the accepted profile, and the refusal cases return
/// before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginOutcome {
    /// The server accepted the login.
    Accepted {
        /// The account UUID, hyphenated.
        uuid: String,
        /// The account name.
        username: String,
    },
    /// The server refused the login, or the stream ended before it settled.
    Ended,
}

/// Runs the login phase until the login settles.
///
/// Set Compression and Login Success arrive here in the order the capture
/// records: the threshold first, still in the plain framing, then the success
/// frame in the compressed framing it switched on. The loop leaves only on a
/// success, a refusal, or the stream ending, so the threshold may arrive at any
/// point in the login.
///
/// An Encryption Request is answered the way the source answers it
/// (`NetHandlerLoginClient.handleEncryptionRequest`): the server key parses,
/// a fresh shared secret generates, the server-id hash computes over the
/// request's server id, the secret and the request's own key bytes — then the
/// signed-in account's joiner posts the join with that hash, and only then the
/// secret and the token encrypt, the response sends, and the ciphers enable.
/// A join failure returns before any response byte is sent; without an account
/// there is nothing to join with, so the refusal stands.
fn login<S: Read + Write>(
    conn: &mut Conn<S>,
    events: &Sender<ClientEvent>,
    account: Option<&SessionAccount>,
) -> Result<LoginOutcome, SessionError> {
    loop {
        let Some(payload) = recv_frame(conn)? else {
            debug!("the stream ended during login");
            return Ok(LoginOutcome::Ended);
        };
        let (id, _) = match read_packet_id(&payload) {
            Ok(read) => read,
            Err(error) => {
                warn!(error = %error, packet = "login", "the packet id did not read");
                return Err(SessionError::Packet(error));
            }
        };
        if !LOGIN_PACKET_IDS.contains(&id) {
            warn!(packet_id = id, "skipping a login packet M1 does not know");
            continue;
        }
        match decoded(id, clientbound::decode_login(&payload))? {
            LoginPacket::SetCompression { threshold } => {
                conn.set_compression(Compression::from_server_threshold(threshold));
                info!(
                    threshold = threshold,
                    "the server set the compression threshold"
                );
            }
            LoginPacket::LoginSuccess { uuid, username } => {
                return Ok(LoginOutcome::Accepted { uuid, username });
            }
            LoginPacket::Disconnect { reason } => {
                report(events, ClientEvent::Disconnected { reason });
                return Ok(LoginOutcome::Ended);
            }
            LoginPacket::EncryptionRequest {
                server_id,
                public_key,
                verify_token,
            } => {
                // The key parses before anything else runs: a malformed key
                // is a typed error with no join and no response bytes.
                let key = parse_server_key(&public_key)?;
                let secret = new_secret_key();
                let hash = server_id_hash(&server_id, &secret, &public_key);
                // The join completes before the response is sent: a failure
                // returns here, so the wire sees nothing after the request.
                let account = match account {
                    Some(account) => account,
                    None => return Err(SessionError::EncryptionRequired),
                };
                if let Err(message) =
                    account
                        .joiner
                        .join(&account.uuid_hex, &account.access_token, &hash)
                {
                    return Err(SessionError::JoinFailed { message });
                }
                let encrypted_secret = encrypt_rsa(&key, &secret)?;
                let encrypted_token = encrypt_rsa(&key, &verify_token)?;
                let response = payload_of(|out| {
                    write_encryption_response(out, &encrypted_secret, &encrypted_token)
                })?;
                send_reply(conn, &response)?;
                // The source enables its ciphers when the response's send
                // completes, so the next frame read already decrypts: Set
                // Compression and Login Success arrive encrypted.
                conn.enable_encryption(&secret);
                info!("the login ciphers are enabled");
            }
        }
    }
}

/// The session's copy of the world's clock: what `WorldClient` keeps and ticks.
///
/// `WorldClient.tick` advances the total world time every tick and the time of
/// day only while the day-night cycle runs (`WorldClient.java:66-74`); the
/// frozen-sun convention puts the cycle's state on the wire as the time's sign,
/// which `WorldClient.setWorldTime` reads back, negating a negative time and
/// stopping the cycle (`:468-481`). [`received_time_of_day`] does the negation
/// here, and the sign it was read from is this clock's `frozen` flag, so the
/// tick's advance is gated exactly where the source gates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Clock {
    /// The world's age in ticks, advanced every tick (`WorldClient.tick`, `:69`).
    world_age: i64,
    /// The time of day in ticks, after the receive rule's negation.
    time_of_day: i64,
    /// Whether the day-night cycle is stopped: the wire's negative time.
    frozen: bool,
}

/// Recomputes the aim and reports it when it changed.
///
/// The aim is the block the interaction ray meets: from the pose eye —
/// `position + eye_height()`, with no displacement — along the look vector,
/// for the gamemode's reach. A session with no world yet aims at nothing.
fn update_aim(
    world: Option<&World>,
    player: &Player,
    gamemode: u8,
    aim: &mut Option<Aim>,
    events: &Sender<ClientEvent>,
) {
    let next = world.and_then(|world| {
        let eye = [
            player.position[0],
            player.position[1] + player.eye_height(),
            player.position[2],
        ];
        raycast(
            &WorldView(world),
            eye,
            look_vector(player.yaw, player.pitch),
            reach(gamemode),
        )
        .map(|aim| {
            let block = world.block(aim.x, aim.y, aim.z);
            (aim, block)
        })
    });
    let next_aim = next.map(|(aim, _)| aim);
    if next_aim != *aim {
        *aim = next_aim;
        report(
            events,
            ClientEvent::Aim {
                aim: next_aim,
                block: next.map(|(_, block)| block),
            },
        );
    }
}

/// The chat field's character cap: `GuiChat.initGui`'s own
/// `setMaxStringLength(100)` (`GuiChat.java`:59).
///
/// The field enforces it; the session keeps the same number for its guard
/// over a message that slipped past the field.
const CHAT_FIELD_CAP: usize = 100;

/// Whether a chat message is past the field's cap, counted in characters.
///
/// The field counts characters (`GuiTextField.setMaxStringLength` over
/// `String.length`, `GuiChat.java`:59) and so does this — a message of a
/// hundred multibyte characters is within the cap.
fn chat_over_cap(text: &str) -> bool {
    text.chars().count() > CHAT_FIELD_CAP
}

/// The packet one click owes, or `None` when the click names no container the
/// state holds.
///
/// The source's click path runs against the container the client holds
/// (`PlayerControllerMP.windowClick:515-543` runs `openContainer.slotClick`
/// and sends what it returned): a click naming any other window has nothing to
/// run and sends nothing — the window and its slot are checked before anything
/// is written — and a click that lands steps the container's own action number
/// (`Container.getNextTransactionID:561-565`, counting from one in
/// [`Windows`]), carries the echo the local prediction's branch produced, and
/// reports whether the prediction moved the state so the caller republishes
/// the window.
fn click_window_packet(
    windows: &mut Windows,
    window_id: i32,
    slot: i16,
    button: i8,
    mode: i8,
) -> Result<Option<(Vec<u8>, bool)>, SessionError> {
    let Ok(wire_id) = i8::try_from(window_id) else {
        debug!(window_id, "a click names a window the wire cannot carry");
        return Ok(None);
    };
    if !windows.holds_window(wire_id) {
        debug!(window_id, "a click names no window the state holds");
        return Ok(None);
    }
    let (echo, mutated) = windows.predict_click(wire_id, slot, button, mode);
    let action = windows.next_action_number();
    Ok(Some((
        payload_of(|out| {
            write_click_window(out, wire_id, slot, button, action, echo.as_ref(), mode)
        })?,
        mutated,
    )))
}

/// Whether one close the view sent writes its packet: the state must take the
/// window, or it must be window 0's own close.
///
/// The source's close path writes C0D for the container the screen stood on
/// (`EntityPlayerSP.closeScreen:330-334`) — window 0 included, which is why the
/// send cannot simply follow the state's rule — and drops the cursor with the
/// screen (`closeScreenAndDropStack:336-341`). Window 0 stays in the state but
/// its carried stack drops with the close; every other id is the state's own
/// call: [`Windows::apply_close`] clears the live open window and refuses —
/// counting it — a close that names nothing, so a second close of an
/// already-shut window sends nothing and changes nothing.
fn close_window(windows: &mut Windows, window_id: u8) -> bool {
    if window_id == 0 {
        windows.drop_cursor();
        return true;
    }
    windows.apply_close(window_id)
}

/// Applies one window event to the held input and the player's look, and
/// answers whether the look moved.
///
/// The mouse delta turns the player where the source turns it — when the
/// mouse is polled, in `EntityRenderer.updateMouse` (`:1094-1123`), not on the
/// tick — with the pitch clamped as `Entity.setAngles` clamps it
/// (`Entity.java:395`). A focus loss releases every held key, so a window that
/// stops receiving leaves nothing held. A moved look asks for the aim to be
/// recomputed before the next tick.
///
/// The chat send and the click machine's actions have no arm here to reach:
/// every one of them is applied — and its packet written — by the play loop's
/// drain before this sees it, and their arm below is match completeness only.
fn apply_input(
    event: InputEvent,
    intent: &mut Intent,
    player: &mut Player,
    sensitivity: f32,
    invert_mouse: bool,
) -> bool {
    match event {
        InputEvent::Key { key, pressed } => {
            intent.apply_key(key, pressed);
            false
        }
        // The F5 edge steps the camera mode session-side; the tick's own
        // occlusion pass reads the stored mode, so this arm only keeps the
        // match complete.
        InputEvent::Perspective { .. } => false,
        InputEvent::MouseDelta { dx, dy } => {
            let (d_yaw, d_pitch) = look_delta(dx, dy, sensitivity);
            // The invert-mouse option flips the pitch sign
            // (`EntityRenderer.java`:1103-1107).
            let d_pitch = if invert_mouse { -d_pitch } else { d_pitch };
            player.yaw += d_yaw;
            player.pitch = (player.pitch + d_pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
            true
        }
        // The buttons belong to interaction, not movement; the held intent
        // has no use for them. The left button is drained by the play loop
        // into the dig input ([`apply_button`]) before this sees it.
        InputEvent::MouseButton { .. } => false,
        InputEvent::FocusLost => {
            intent.release_all();
            false
        }
        // The chat send was already collected and written by the drain; it
        // carries no held key and no look, so nothing here applies. The click
        // machine's actions were dispatched and written there too.
        InputEvent::SendChat { .. }
        | InputEvent::ClickWindow { .. }
        | InputEvent::CloseWindow { .. }
        | InputEvent::CreativeAction { .. }
        | InputEvent::EnchantItem { .. }
        | InputEvent::CustomPayload { .. }
        | InputEvent::UpdateSign { .. }
        | InputEvent::HeldItemChange { .. }
        | InputEvent::OpenInventory
        | InputEvent::LeaveBed
        | InputEvent::SetViewDistance { .. }
        | InputEvent::PushSettings(_)
        | InputEvent::SetParticleSetting { .. }
        | InputEvent::RebuildMeshes
        | InputEvent::DropItem { .. } => false,
    }
}

/// Stages one tick's step sounds: the splash on water entry, then the swim
/// or the footstep past the walk threshold.
///
/// The splash is the dry-to-wet transition's `resetHeight`
/// (`Entity.handleWaterMovement:1111-1129`, `:1135-1144`): the volume off the
/// motion vector times `0.2`, capped at `1.0`, the pitch `1.0` plus the swim
/// spread. The footstep is the move's own accounting (`Entity.java:852-900`):
/// the horizontal walk — the vertical only on a climbable block — times `0.6`
/// joins the walked distance, and past the threshold the water swims (the
/// motion times `0.35`, capped) while the dry foot plays the stood-on block's
/// step event at `volume * 0.15` (`playStepSound:986-998`), snow when a snow
/// layer sits on top and nothing when the stood-on block is air or liquid.
/// The position is the player's own feet in both cases
/// (`playSoundAtEntity:1001-1005`).
#[allow(clippy::too_many_arguments)]
fn stage_step_sounds(
    player: &Player,
    world: Option<&World>,
    distance_walked: &mut f32,
    next_step_distance: &mut i32,
    was_in_water: &mut bool,
    sound_rng: &mut StdRng,
    events: &Sender<ClientEvent>,
) -> bool {
    let Some(world) = world else {
        *was_in_water = player.in_water;
        return false;
    };
    let feet = [
        player.position[0] as f32,
        player.position[1] as f32,
        player.position[2] as f32,
    ];
    let entered_water = !*was_in_water && player.in_water;
    if entered_water {
        report(
            events,
            ClientEvent::LocalSound {
                name: SPLASH_SOUND.to_string(),
                pos: feet,
                volume: splash_volume(player.motion),
                pitch: swim_pitch(sound_rng.random(), sound_rng.random()),
                category_hint: SoundCategory::Players,
            },
        );
    }
    *was_in_water = player.in_water;
    // The stood-on block: the feet cell two-tenths down (`Entity.java:820`).
    let stood = world.block(
        player.position[0].floor() as i32,
        (player.position[1] - 0.2).floor() as i32,
        player.position[2].floor() as i32,
    ) >> 4;
    let dx = player.position[0] - player.last_tick_position[0];
    let mut dy = player.position[1] - player.last_tick_position[1];
    let dz = player.position[2] - player.last_tick_position[2];
    if !behaviour(stood).is_some_and(|row| row.climbable) {
        dy = 0.0;
    }
    *distance_walked += ((dx * dx + dy * dy + dz * dz) as f32).sqrt() * 0.6;
    if *distance_walked <= *next_step_distance as f32 || stood == 0 {
        return entered_water;
    }
    *next_step_distance = *distance_walked as i32 + 1;
    if player.in_water {
        report(
            events,
            ClientEvent::LocalSound {
                name: SWIM_SOUND.to_string(),
                pos: feet,
                volume: swim_volume(player.motion),
                pitch: swim_pitch(sound_rng.random(), sound_rng.random()),
                category_hint: SoundCategory::Players,
            },
        );
    }
    // The snow layer on top lends its own step (`playStepSound:988-993`);
    // a liquid stood-on block stays silent (`:994-997`).
    let above = world.block(
        player.position[0].floor() as i32,
        (player.position[1] - 0.2).floor() as i32 + 1,
        player.position[2].floor() as i32,
    ) >> 4;
    let group = if above == SNOW_LAYER_ID {
        SoundGroup::Snow
    } else {
        SoundGroup::of_block(stood)
    };
    if behaviour(stood).is_some_and(|row| row.material == Material::Liquid) {
        return entered_water;
    }
    report(
        events,
        ClientEvent::LocalSound {
            name: group.step_sound().to_string(),
            pos: feet,
            volume: group.step_volume(),
            pitch: group.frequency(),
            category_hint: SoundCategory::Blocks,
        },
    );
    entered_water
}

/// One tick of the stride's local particles, beside the step sounds: the
/// water-entry fan-outs on the dry-to-wet edge the sounds just read, and the
/// sprint crumb every sprinting dry tick (`Entity.spawnRunningParticles:1165`
/// gates the same two flags before `createRunningParticles:1172-1188`).
fn stage_stride_particles(
    player: &Player,
    world: Option<&World>,
    entered_water: bool,
    particles: &mut ParticleEngine,
) {
    let Some(world) = world else {
        return;
    };
    for spawn in water_spawns(
        particles,
        player.position,
        player.motion,
        PLAYER_WIDTH,
        entered_water,
    ) {
        particles.spawn_local(spawn);
    }
    // The stood-on cell, read as the full state value: the crumb's data is
    // the state's own id, where the sounds above read only the block id.
    let stood = world.block(
        player.position[0].floor() as i32,
        (player.position[1] - 0.2).floor() as i32,
        player.position[2].floor() as i32,
    );
    if let Some(spawn) = sprint_spawn(
        particles,
        player.position,
        player.motion,
        PLAYER_WIDTH,
        stood,
        renderable_id(stood >> 4),
        player.sprinting,
        player.in_water,
    ) {
        particles.spawn_local(spawn);
    }
}

/// Applies one mouse button edge to the interaction input.
///
/// The left button is the source's attack key: a press is one `clickMouse`
/// call (`Minecraft.java:1522-1524`) and the held flag is
/// `sendClickBlockToController`'s `leftClick` (`Minecraft.java:1496-1498`). The right
/// button is the use key: a press is one `rightClickMouse` call
/// (`Minecraft.java:1570-1572`), consumed by the placement step.
fn apply_button(
    button: MouseButton,
    pressed: bool,
    left_held: &mut bool,
    left_presses: &mut u32,
    right_presses: &mut u32,
) {
    match button {
        MouseButton::Left => {
            if pressed {
                *left_presses += 1;
            }
            *left_held = pressed;
        }
        MouseButton::Right => {
            if pressed {
                *right_presses += 1;
            }
        }
    }
}

/// Applies a clientbound 0x08 teleport to the player.
///
/// A flagged axis is a delta on the state the session holds, an unflagged one
/// is already absolute — the rule the source's flags state (the wire's flag
/// byte through `S08PacketPlayerPosLook`, whose constants the codec carries).
/// The motion is kept: an ordinary correction moves the player without
/// zeroing velocity; a dimension respawn is what zeroes it.
fn apply_correction(player: &mut Player, teleport: &PlayerPositionAndLook) {
    let flags = teleport.flags;
    let axis = |held: f64, arrives: f64, flag: u8| {
        if flags & flag != 0 {
            held + arrives
        } else {
            arrives
        }
    };
    player.position = [
        axis(
            player.position[0],
            teleport.x,
            PlayerPositionAndLook::FLAG_X,
        ),
        axis(
            player.position[1],
            teleport.y,
            PlayerPositionAndLook::FLAG_Y,
        ),
        axis(
            player.position[2],
            teleport.z,
            PlayerPositionAndLook::FLAG_Z,
        ),
    ];
    player.yaw = axis(
        f64::from(player.yaw),
        f64::from(teleport.yaw),
        PlayerPositionAndLook::FLAG_YAW,
    ) as f32;
    player.pitch = axis(
        f64::from(player.pitch),
        f64::from(teleport.pitch),
        PlayerPositionAndLook::FLAG_PITCH,
    ) as f32;
}

/// The portal block's numeric id (`Blocks.portal`, `Blocks.java:358`): 90, the
/// nether-portal block the behaviour table carries no row for.
const PORTAL_BLOCK_ID: u16 = 90;

/// The player's width in blocks: the source's 0.6 (`EntityPlayer`'s own
/// width, carried by `Entity.width`), the span the probes read `0.8` of.
const PLAYER_WIDTH: f64 = 0.6;

/// The player's height in blocks for the portal scan: 1.8 standing (the
/// source's `EntityPlayer` height).
const PLAYER_HEIGHT: f64 = 1.8;

/// One gate probe's cell (`Entity.isEntityInsideOpaqueBlock`,
/// `Entity.java:1838-1840`): the Y cell from `(index >> 0)`, X from
/// `(index >> 1)`, Z from `(index >> 2)` — the `±0.05` vertical taps at the
/// eye, the `±0.4·width` horizontal ones.
fn gate_cell(index: u32, feet: [f64; 3], eye_height: f64) -> (i32, i32, i32) {
    let tap = |bit: u32| f64::from((index >> bit) % 2) - 0.5;
    (
        (feet[0] + tap(1) * PLAYER_WIDTH * 0.8).floor() as i32,
        (feet[1] + tap(0) * 0.1 + eye_height).floor() as i32,
        (feet[2] + tap(2) * PLAYER_WIDTH * 0.8).floor() as i32,
    )
}

/// One winner probe's cell (`ItemRenderer.renderOverlays`,
/// `ItemRenderer.java:429-431`): X from `(index >> 0)`, Y from `(index >>
/// 1)`, Z from `(index >> 2)` — the other swizzle, floored at the eye.
fn hand_cell(index: u32, feet: [f64; 3], eye_height: f64) -> (i32, i32, i32) {
    let tap = |bit: u32| f64::from((index >> bit) % 2) - 0.5;
    (
        (feet[0] + tap(0) * PLAYER_WIDTH * 0.8).floor() as i32,
        (feet[1] + tap(1) * 0.1 + eye_height).floor() as i32,
        (feet[2] + tap(2) * PLAYER_WIDTH * 0.8).floor() as i32,
    )
}

/// Whether a block id is visually opaque (`Block.isVisuallyOpaque`,
/// `Block.java:361-364`): the material's `blocksMovement` and a full cube.
/// An id the table carries no row for (air, the portal) is not opaque.
fn visually_opaque_id(id: u16) -> bool {
    behaviour(id).is_some_and(|row| row.material.blocks_movement() && row.full_cube)
}

/// Whether a block id draws in the hand (`getRenderType() != -1`,
/// `ItemRenderer.java:445`): air (`BlockAir.java:19-22`) and the barrier
/// (`BlockBarrier.java:22-25`) answer -1; every other id reads the
/// dispatcher's default 3 (`Block.java:379-382`), including the ids the
/// table carries no row for.
fn renderable_id(id: u16) -> bool {
    id != 0 && behaviour(id).is_none_or(|row| !matches!(row.render, RenderKind::Invisible))
}

/// A watcher byte off the tracked metadata: the last entry the block
/// carries for the index, if any is a byte.
fn watcher_byte(metadata: &entity::Metadata, index: u8) -> Option<i8> {
    metadata
        .entries
        .iter()
        .rev()
        .filter(|(at, _)| *at == index)
        .find_map(|(_, value)| match value {
            MetadataValue::Byte(byte) => Some(*byte),
            _ => None,
        })
}

/// A watcher int off the tracked metadata, same shape as [`watcher_byte`].
fn watcher_int(metadata: &entity::Metadata, index: u8) -> Option<i32> {
    metadata
        .entries
        .iter()
        .rev()
        .filter(|(at, _)| *at == index)
        .find_map(|(_, value)| match value {
            MetadataValue::Int(int) => Some(*int),
            _ => None,
        })
}

/// Whether the tick's box overlaps a portal block: the collision's
/// `setPortal` read as a bool (`BlockPortal.onEntityCollidedWithBlock`,
/// `BlockPortal.java:197-204` — the riding checks have no carrier here; the
/// port never rides). The scan covers the standing box; the cooldown
/// (`timeUntilPortal`) and the pattern teleport belong to the travel work,
/// not to this read.
fn portal_overlap(feet: [f64; 3], world: &World) -> bool {
    let min = [
        (feet[0] - PLAYER_WIDTH / 2.0).floor() as i32,
        feet[1].floor() as i32,
        (feet[2] - PLAYER_WIDTH / 2.0).floor() as i32,
    ];
    let max = [
        (feet[0] + PLAYER_WIDTH / 2.0).floor() as i32,
        (feet[1] + PLAYER_HEIGHT).floor() as i32,
        (feet[2] + PLAYER_WIDTH / 2.0).floor() as i32,
    ];
    for x in min[0]..=max[0] {
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                if world.block(x, y, z) >> 4 == PORTAL_BLOCK_ID {
                    return true;
                }
            }
        }
    }
    false
}

/// The eye-block read for one tick: the viewpoint cell, the portal overlap
/// and the block-in-hand probe's winner, from the world data the session
/// owns (`Entity.java:1829-1856`, `ItemRenderer.java:424-447`,
/// `BlockPortal.java:197-204`). Before Join Game there is no world to read
/// and the answer is empty: air at the eye, no portal, the gate shut.
fn eye_block_event(player: &Player, world: Option<&World>) -> ClientEvent {
    let Some(world) = world else {
        return ClientEvent::EyeBlock {
            eye: 0,
            in_portal: false,
            inside: None,
            block_loaded: false,
            biome_id: 0,
            sky_light: 0,
            block_light: 0,
        };
    };
    let feet = player.position;
    let eye_height = player.eye_height();
    let eye_y = feet[1] + eye_height;
    let eye_cell = (
        feet[0].floor() as i32,
        eye_y.floor() as i32,
        feet[2].floor() as i32,
    );
    let mut eye = world.block(eye_cell.0, eye_cell.1, eye_cell.2);
    // The liquid-surface plane (`ActiveRenderInfo.getBlockAtEntityViewpoint`,
    // `ActiveRenderInfo.java:84-107`): a liquid eye cell whose surface stands
    // below the viewpoint reads the block above. The plane sits a ninth past
    // the surface the level carries (`BlockLiquid.java:48-56`), which a source
    // (level 0) places exactly at the cell's top.
    if liquid_kind(eye >> 4).is_some() {
        let level = (eye & 15) as u8;
        let plane = f64::from(eye_cell.1) + f64::from(liquid_height_percent(level)) + 1.0 / 9.0;
        if eye_y >= plane {
            eye = world.block(eye_cell.0, eye_cell.1 + 1, eye_cell.2);
        }
    }
    let in_portal = portal_overlap(feet, world);
    // The gate (`Entity.isEntityInsideOpaqueBlock`, `Entity.java:1829-1856`):
    // any visually-opaque probe opens it.
    let gate_open = (0..8u32).any(|index| {
        let (x, y, z) = gate_cell(index, feet, eye_height);
        visually_opaque_id(world.block(x, y, z) >> 4)
    });
    // The winner probe (`ItemRenderer.java:424-447`): the last opaque probe's
    // packed state — or nothing when the gate is shut or its render type is
    // `-1` (`:445`).
    let mut inside = None;
    if gate_open {
        for index in 0..8u32 {
            let (x, y, z) = hand_cell(index, feet, eye_height);
            let state = world.block(x, y, z);
            if visually_opaque_id(state >> 4) && renderable_id(state >> 4) {
                inside = Some(state);
            }
        }
    }
    let (block_loaded, biome_id, sky_light, block_light) = view_block_read(world, feet);
    ClientEvent::EyeBlock {
        eye,
        in_portal,
        inside,
        block_loaded,
        biome_id,
        sky_light,
        block_light,
    }
}

/// The feet block's debug-overlay read: whether its column is loaded, its
/// biome id and its two light nibbles, from the world data the session owns.
///
/// An unloaded column — or a feet row outside 0..256 — answers the unload
/// half: not loaded, the ocean fallback id and zeroed nibbles.
fn view_block_read(world: &World, feet: [f64; 3]) -> (bool, u8, u8, u8) {
    let bx = feet[0].floor() as i32;
    let by = feet[1].floor() as i32;
    let bz = feet[2].floor() as i32;
    let Some(chunk) = (0..256)
        .contains(&by)
        .then(|| world.chunk(bx >> 4, bz >> 4))
        .flatten()
    else {
        return (false, 0, 0, 0);
    };
    let biome_id = chunk.biome((bx & 15) as usize, (bz & 15) as usize);
    let sky_light = world.sky_light(bx, by, bz);
    let block_light = world.block_light(bx, by, bz);
    (true, biome_id, sky_light, block_light)
}

/// The player's current state as the window's per-tick event.
fn player_tick(player: &Player, snapped: bool) -> ClientEvent {
    ClientEvent::PlayerTick {
        x: player.position[0],
        y: player.position[1],
        z: player.position[2],
        yaw: player.yaw,
        pitch: player.pitch,
        on_ground: player.on_ground,
        sprinting: player.sprinting,
        sneaking: player.sneaking,
        flying: player.flying,
        in_water: player.in_water,
        tick: player.tick,
        snapped,
        hurt_time: player.hurt_time,
        attacked_at_yaw: player.attacked_at_yaw,
    }
}

/// The tracked entities as the window's per-tick event.
///
/// The frames are built here, against the same light data the mesher reads
/// and the list and board the 0x38–0x3E arms maintain — the window holds no
/// world snapshot of its own.
fn entities_tick(
    entities: &Entities,
    world: Option<&World>,
    player_list: &PlayerList,
    board: &Scoreboard,
) -> ClientEvent {
    ClientEvent::EntitiesTick {
        entities: entity_view::snapshot(entities, world, player_list, board),
    }
}

/// The store kind a spawn mob's wire type names.
///
/// The wire's mob table and the store's kinds are one-to-one: every variant
/// the decoder accepts has a kind.
fn kind_from_mob(mob: MobType) -> EntityKind {
    match mob {
        MobType::Creeper => EntityKind::Creeper,
        MobType::Skeleton => EntityKind::Skeleton,
        MobType::Spider => EntityKind::Spider,
        MobType::Giant => EntityKind::Giant,
        MobType::Zombie => EntityKind::Zombie,
        MobType::Slime => EntityKind::Slime,
        MobType::Ghast => EntityKind::Ghast,
        MobType::PigZombie => EntityKind::PigZombie,
        MobType::Enderman => EntityKind::Enderman,
        MobType::CaveSpider => EntityKind::CaveSpider,
        MobType::Silverfish => EntityKind::Silverfish,
        MobType::Blaze => EntityKind::Blaze,
        MobType::LavaSlime => EntityKind::LavaSlime,
        MobType::EnderDragon => EntityKind::EnderDragon,
        MobType::WitherBoss => EntityKind::WitherBoss,
        MobType::Bat => EntityKind::Bat,
        MobType::Witch => EntityKind::Witch,
        MobType::Endermite => EntityKind::Endermite,
        MobType::Guardian => EntityKind::Guardian,
        MobType::Pig => EntityKind::Pig,
        MobType::Sheep => EntityKind::Sheep,
        MobType::Cow => EntityKind::Cow,
        MobType::Chicken => EntityKind::Chicken,
        MobType::Squid => EntityKind::Squid,
        MobType::Wolf => EntityKind::Wolf,
        MobType::MushroomCow => EntityKind::MushroomCow,
        MobType::SnowMan => EntityKind::SnowMan,
        MobType::Ozelot => EntityKind::Ozelot,
        MobType::VillagerGolem => EntityKind::VillagerGolem,
        MobType::EntityHorse => EntityKind::EntityHorse,
        MobType::Rabbit => EntityKind::Rabbit,
        MobType::Villager => EntityKind::Villager,
    }
}

/// The store kind a spawn object's wire type names.
///
/// Objects the store's kinds cannot name — the primed TNT, the crystal, the
/// falling blocks, the stands and hooks — arrive as [`EntityKind::Unknown`]:
/// tracked, drawn not at all.
fn kind_from_object(object: ObjectType) -> EntityKind {
    match object {
        ObjectType::Boat => EntityKind::Boat,
        ObjectType::Item => EntityKind::Item,
        ObjectType::Minecart | ObjectType::MinecartStorage | ObjectType::MinecartPowered => {
            EntityKind::Minecart
        }
        ObjectType::Arrow => EntityKind::Arrow,
        ObjectType::Snowball => EntityKind::Snowball,
        ObjectType::ThrownEgg => EntityKind::Egg,
        ObjectType::Fireball => EntityKind::Fireball,
        ObjectType::SmallFireball => EntityKind::SmallFireball,
        ObjectType::ThrownEnderpearl => EntityKind::EnderPearl,
        ObjectType::WitherSkull => EntityKind::WitherSkull,
        ObjectType::ItemFrame => EntityKind::ItemFrame,
        ObjectType::EyeOfEnderSignal => EntityKind::EyeOfEnder,
        ObjectType::ThrownPotion => EntityKind::Potion,
        ObjectType::ThrownExpBottle => EntityKind::XpBottle,
        ObjectType::FireworksRocketEntity => EntityKind::Firework,
        _ => EntityKind::Unknown,
    }
}

/// The spawn-given extras an object spawn carries.
///
/// Only the objects whose kinds hold extras take one; the rest keep
/// [`KindData::None`]. An item's `data` is the item id — its stack's count
/// and damage arrive with the entity's metadata (index 10) — and a negative
/// id, which no item has, is kept as the wire's own zero like every other
/// out-of-range item id.
fn kind_data_of(object: ObjectType, data: i32) -> KindData {
    match object {
        ObjectType::Boat => KindData::Boat,
        ObjectType::Item => KindData::Item {
            id: i16::try_from(data).unwrap_or(0),
            count: 1,
            damage: 0,
        },
        ObjectType::Minecart | ObjectType::MinecartStorage | ObjectType::MinecartPowered => {
            KindData::Minecart
        }
        _ => KindData::None,
    }
}

/// The respawn's dimension, checked against the width the session's world
/// carries.
///
/// The packet carries the dimension as an `i32` (`S07PacketRespawn:17-19`) and
/// Join Game names it as a byte, and the session's world model is the byte:
/// only `-1`, `0` and `1` are dimensions this client can build. The source
/// cannot carry any other value either — `WorldProvider.getProviderForDimension`
/// answers null for it and `WorldClient` dereferences that
/// (`WorldClient.java:52-56`) — so it is refused rather than narrowed, and the
/// refusal is the packet error the decoders raise for a value no model can
/// hold.
fn respawn_dimension(dimension: i32) -> Result<i8, SessionError> {
    i8::try_from(dimension).map_err(|_| {
        SessionError::Packet(PacketError::Codec(CodecError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("respawn dimension {dimension} does not fit the world model's byte"),
        ))))
    })
}

/// One 20 Hz step of the session's state.
///
/// The source's own per-tick order: an entity copies its previous position at
/// the top of its tick (`Entity.onEntityUpdate`, `Entity.java:420-423`); the
/// client tick reads the movement input into the sneak flag (`isSneaking`,
/// `EntityPlayerSP.java:684-688`) and runs the sprint rule once
/// (`EntityPlayerSP.onLivingUpdate`:801-821; its release's collision term
/// reads the previous step's flag); the clock advances as [`Clock`] describes.
/// The tick count is the session's own and is the cloud offset's source (the
/// source's `cloudTickCounter` advances once per tick, `RenderGlobal.updateClouds`,
/// `RenderGlobal.java:1138-1146`), reported on every tick either way.
///
/// A tick that moved the clock also reports the sky it moved: the sun should
/// travel at the tick rate, not at the Time Update rate. A frozen clock moves
/// nothing, so it reports nothing.
///
/// The movement model runs within the tick, against the world as it stands
/// ([`physics::step`]): the fluid probe, the jump cooldown, the drag and the
/// collision walk all happen here, and the tick's report carries where the
/// step left the player.
///
/// The tick's sends are the source's own, in its order: the flight toggle's
/// 0x13 (`EntityPlayerSP.onLivingUpdate:836-845`) and the landing cancel's
/// 0x13 (`:904-908`), then the sprint and sneak edges
/// (`onUpdateWalkingPlayer:189-221`) and the walking report (`:215-274`).
/// They return to the caller, which writes each one as the tick produced it.
/// `movement_reports` gates the edges and the report — a respawn holds them
/// until the 0x08 that places the fresh player arrives (the play loop's
/// `awaiting_respawn_position`); the movement model, the clock and the
/// per-tick event run either way.
///
/// The hurt countdown and the death clock advance at the top of the step,
/// where the source's own entity tick runs them before the movement
/// (`EntityLivingBase.onEntityUpdate:337-349`): the hurt flash falls by one
/// each tick and the death clock rises by one while the death state holds.
// One argument per owner of the tick's state; bundling them into a struct
// would only move the same list one level down.
#[allow(clippy::too_many_arguments)]
fn step_tick(
    player: &mut Player,
    input: &Intent,
    mut clock: Option<&mut Clock>,
    world: Option<&World>,
    entities: &mut Entities,
    player_list: &PlayerList,
    board: &Scoreboard,
    events: &Sender<ClientEvent>,
    movement_reports: bool,
    weather: &WeatherState,
    weather_moved: bool,
) -> Vec<Vec<u8>> {
    player.last_tick_position = player.position;
    player.tick += 1;
    // The entity store ticks with the player: its pose pairs are copied at
    // the top and the per-kind rules advance (`Entities::tick`).
    entities.tick();
    // The hurt flash's countdown (`EntityLivingBase.onEntityUpdate:337-340`)
    // and the death clock's advance while dead (`:344-349` reaching
    // `onDeathUpdate`, whose `++this.deathTime` is `:400`).
    if player.hurt_time > 0 {
        player.hurt_time -= 1;
    }
    if player.dead {
        player.death_time += 1;
    }
    // The sneak flag is the held key, and the sprint rule runs once per tick,
    // before the move — so its collision term reads the previous step's flag
    // (`EntityPlayerSP.java`:818-821; `Entity.java`:818).
    player.sneaking = input.sneak;
    let sprinting = player.sprinting;
    player.sprinting = player.sprint_tap.update(
        input,
        sprinting,
        player.on_ground,
        player.last_step.collided_horizontally,
    );

    // The double-tap flight toggle (`EntityPlayerSP.onLivingUpdate:823-845`)
    // runs before the movement model consumes `flying`, and its 0x13 goes out
    // at once — the source sends it inline.
    let mut sends = Vec::new();
    if player.update_flight(input.jump) {
        sends.push(abilities_payload(player));
    }

    // The movement model runs one step against the world as it stands: the
    // fluid probe, the jump cooldown, the drag and the collision walk move the
    // player, and the report below carries where the step left it. Before Join
    // Game there is no world to move against and the step is skipped.
    if let Some(world) = world {
        // The step's outcome is held for the next tick's sprint release: the
        // source's sprint rule runs before its move, so the release reads the
        // previous move's own flag (`EntityPlayerSP.java`:818-821;
        // `Entity.java`:818).
        let outcome = physics::step(player, input, &WorldView(world));
        player.last_step = outcome;
    }

    // Landing cancels flight (`EntityPlayerSP.onLivingUpdate:904-908`): the
    // source checks it after the move, at the end of its living tick, and
    // sends the abilities packet itself.
    if player.on_ground && player.flying {
        player.set_flying(false);
        sends.push(abilities_payload(player));
    }

    // The walking report and the action edges are the tick's own, and they
    // begin where this client enters a world — at Join Game, which also names
    // the entity id they carry. The source's tick is gated the same way, on
    // the block under the player being loaded (`EntityPlayerSP.onUpdate:170`),
    // and `movement_reports` is the respawn's own version of that gate: the
    // fresh player waits for the 0x08 that places it.
    if let Some(entity_id) = player.entity_id {
        // The respawn gate: the fresh player waits for the 0x08 that places
        // it before it reports anything of itself, exactly as the source's
        // tick waits for the world under the player to load
        // (`EntityPlayerSP.onUpdate:170`). The edges and the report are what
        // the gate holds; the movement model above ran either way.
        if movement_reports {
            // One packet per change (`EntityPlayerSP.onUpdateWalkingPlayer:189-221`):
            // a stable state sends nothing, so the server hears exactly the edges.
            if player.sprinting != player.server_sprint_state {
                let action = if player.sprinting {
                    EntityAction::StartSprinting
                } else {
                    EntityAction::StopSprinting
                };
                sends.push(tick_payload(|out| {
                    write_entity_action(out, entity_id, action, 0)
                }));
                player.server_sprint_state = player.sprinting;
            }
            if player.sneaking != player.server_sneak_state {
                let action = if player.sneaking {
                    EntityAction::StartSneaking
                } else {
                    EntityAction::StopSneaking
                };
                sends.push(tick_payload(|out| {
                    write_entity_action(out, entity_id, action, 0)
                }));
                player.server_sneak_state = player.sneaking;
            }
            // The walking report, built after the edges so a tick's packets keep
            // the source's order.
            let report_kind = walking_report(player);
            sends.push(match report_kind {
                WalkingReport::Player => tick_payload(|out| write_player(out, player.on_ground)),
                WalkingReport::Position => tick_payload(|out| {
                    write_player_position(
                        out,
                        player.position[0],
                        player.position[1],
                        player.position[2],
                        player.on_ground,
                    )
                }),
                WalkingReport::Look => tick_payload(|out| {
                    write_player_look(out, player.yaw, player.pitch, player.on_ground)
                }),
                WalkingReport::PositionAndLook => tick_payload(|out| {
                    write_player_position_and_look(
                        out,
                        player.position[0],
                        player.position[1],
                        player.position[2],
                        player.yaw,
                        player.pitch,
                        player.on_ground,
                    )
                }),
            });
        }
    }

    let mut advanced = false;
    if let Some(clock) = clock.as_mut() {
        clock.world_age += 1;
        if !clock.frozen {
            clock.time_of_day += 1;
            advanced = true;
        }
    }
    if advanced || weather_moved {
        report_sky(world, player.position, clock.as_deref(), weather, events);
    }
    report(events, player_tick(player, false));
    // The feed sits immediately behind the player's own event: one
    // `EntitiesTick` per tick, carrying every tracked entity's frame, and the
    // eye-block read behind that — the window keeps the values for the
    // overlay draws, the portal clock and the lava fog arm.
    report(events, entities_tick(entities, world, player_list, board));
    report(events, eye_block_event(player, world));
    sends
}

/// Which of the four player packets one walking tick reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkingReport {
    /// 0x03: the ground state alone — neither position nor rotation moved.
    Player,
    /// 0x04: the position alone.
    Position,
    /// 0x05: the rotation alone.
    Look,
    /// 0x06: both.
    PositionAndLook,
}

/// One tick of the source's walking report rule, quoted:
///
/// ```text
/// double d0 = this.posX - this.lastReportedPosX;
/// double d1 = this.getEntityBoundingBox().minY - this.lastReportedPosY;
/// double d2 = this.posZ - this.lastReportedPosZ;
/// double d3 = (double)(this.rotationYaw - this.lastReportedYaw);
/// double d4 = (double)(this.rotationPitch - this.lastReportedPitch);
/// boolean flag2 = d0 * d0 + d1 * d1 + d2 * d2 > 9.0E-4D || this.positionUpdateTicks >= 20;
/// boolean flag3 = d3 != 0.0D || d4 != 0.0D;
/// ```
///
/// The two flags then choose the packet (`EntityPlayerSP.java:225-274`): both
/// send the position and the rotation, the position alone, the rotation
/// alone, and neither sends the ground byte alone. The reporters update with
/// their own flag and the counter restarts with the position flag
/// (`:258-273`); a tick with no position report still counts, which is what
/// re-sends a quiet player's position once the counter reaches twenty. The
/// riding branch (`:255-258`) has no counterpart here: nothing rides in M3.
fn walking_report(player: &mut Player) -> WalkingReport {
    let dx = player.position[0] - player.last_reported_position[0];
    let dy = player.position[1] - player.last_reported_position[1];
    let dz = player.position[2] - player.last_reported_position[2];
    let moved = dx * dx + dy * dy + dz * dz > POSITION_EPSILON
        || player.position_update_ticks >= POSITION_STALE_TICKS;
    let looked =
        player.yaw != player.last_reported_yaw || player.pitch != player.last_reported_pitch;
    let report_kind = match (moved, looked) {
        (true, true) => WalkingReport::PositionAndLook,
        (true, false) => WalkingReport::Position,
        (false, true) => WalkingReport::Look,
        (false, false) => WalkingReport::Player,
    };
    player.position_update_ticks += 1;
    if moved {
        player.last_reported_position = player.position;
        player.position_update_ticks = 0;
    }
    if looked {
        player.last_reported_yaw = player.yaw;
        player.last_reported_pitch = player.pitch;
    }
    report_kind
}

/// Resets the walking reporters to where a correction settled the player.
///
/// The source's teleport handler echoes the corrected pose itself and leaves
/// its `lastReported*` fields alone
/// (`NetHandlerPlayClient.handlePlayerPosLook:669-707`), so its next tick
/// replays the corrected pose as a move. This client keeps the reporters at
/// the echo instead: the echo is the position report that reconciles the
/// correction, and the tick after it reports nothing of the pre-correction
/// displacement. `position_update_ticks` restarts with the echo, so the
/// staleness clock measures from the report that was sent.
fn reset_reporters(player: &mut Player) {
    player.last_reported_position = player.position;
    player.last_reported_yaw = player.yaw;
    player.last_reported_pitch = player.pitch;
    player.position_update_ticks = 0;
}

/// The 0x13 payload for the player's abilities.
fn abilities_payload(player: &Player) -> Vec<u8> {
    tick_payload(|out| {
        write_player_abilities(
            out,
            player.abilities.flags(),
            player.abilities.fly_speed,
            player.abilities.walk_speed,
        )
    })
}

/// Builds a payload whose writer cannot fail.
///
/// Writing to a `Vec` is total, and the fields the tick writes are bounded by
/// the packets' own layout, so the `expect` cannot fire.
fn tick_payload(build: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> Vec<u8> {
    payload_of(build).expect("writing to a Vec cannot fail")
}

/// Reads the next frame; `None` means the stream ended.
fn recv_frame<S: Read + Write>(conn: &mut Conn<S>) -> Result<Option<Vec<u8>>, SessionError> {
    match conn.recv() {
        Ok(payload) => Ok(Some(payload)),
        Err(error) if is_stream_end(&error) => {
            debug!(error = %error, "the stream ended");
            Ok(None)
        }
        Err(error) => Err(SessionError::Frame(error)),
    }
}

/// Whether a framing failure is the stream ending rather than corruption.
///
/// A stream that simply stops leaves a read at the end of the file, and a frame
/// cut off mid-write leaves `read_exact` with an unexpected end; both are how a
/// server closes a session, so both end the loop quietly. A server that exits
/// abruptly can reset or abort the connection instead of ending it tidily,
/// which is the same close seen from this end, so those end it quietly too.
fn is_stream_end(error: &FrameError) -> bool {
    match error {
        FrameError::Io(cause) => closed_by_peer(cause.kind()),
        FrameError::VarInt(VarIntError::UnexpectedEof) => true,
        FrameError::VarInt(VarIntError::Io(cause)) => closed_by_peer(cause.kind()),
        FrameError::VarInt(VarIntError::TooLong)
        | FrameError::TooLong(..)
        | FrameError::NegativeLength(..)
        | FrameError::BadCompression
        | FrameError::EmptyPayload => false,
    }
}

/// Whether an IO error kind is the peer closing the connection.
///
/// An unexpected end of file, a reset and an abort all mean the server is gone;
/// only the first is tidy, but none of them is corruption.
fn closed_by_peer(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
    )
}

/// Sends one reply frame, folding a stream write failure into [`SessionError::Io`].
fn send_reply<S: Read + Write>(conn: &mut Conn<S>, payload: &[u8]) -> Result<(), SessionError> {
    match conn.send(payload) {
        Ok(()) => Ok(()),
        Err(FrameError::Io(cause)) => Err(SessionError::Io(cause)),
        Err(error) => Err(SessionError::Frame(error)),
    }
}

/// Builds a packet payload with one of the codec's writers.
///
/// A writer's failure is an IO failure: the codecs refuse a malformed field
/// before anything reaches the stream.
fn payload_of(build: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> Result<Vec<u8>, SessionError> {
    let mut payload = Vec::new();
    build(&mut payload)?;
    Ok(payload)
}

/// Unwraps a known packet's decode result, leaving the id in the log line.
///
/// The id is one the client has a codec for: a packet it cannot name is skipped
/// before it gets here, so a failure here is corruption rather than a packet
/// M1 does not know.
fn decoded<T>(id: i32, result: Result<T, PacketError>) -> Result<T, SessionError> {
    result.map_err(|error| {
        warn!(packet_id = id, error = %error, "the packet did not decode");
        SessionError::Packet(error)
    })
}

/// Reports one event; a window that stopped listening is not an error here.
fn report(events: &Sender<ClientEvent>, event: ClientEvent) {
    let _ = events.send(event);
}

/// The window one landed slot write belongs to, for the snapshot it reports.
///
/// The wire's window −1 is the carried stack, which rides every window; the
/// write reports window 0, the projection that always exists. Window 0 reports
/// itself, and any other id the state admitted is the live open window's.
fn slot_window(window_id: i8) -> u8 {
    u8::try_from(window_id).unwrap_or(0)
}

/// Reports one window's snapshot, built from the state's own projection.
///
/// The projection is built here, per change — never per frame — and a window
/// the state does not hold reports nothing.
fn report_window_snapshot(windows: &Windows, window_id: u8, events: &Sender<ClientEvent>) {
    if let Some(snapshot) = windows.snapshot(window_id) {
        report(
            events,
            ClientEvent::WindowSnapshot {
                window_id: snapshot.window_id,
                slots: snapshot.slots,
                cursor: snapshot.cursor,
                properties: snapshot.properties,
                hotbar_pop: snapshot.hotbar_pop,
            },
        );
    }
}

/// Combines a fresh effect into the one held.
///
/// The source's own rule (`PotionEffect.combine:63-85`): a stronger amplifier
/// replaces the pair — duration and all — an equal one keeps the longer
/// duration, and anything else leaves the pair alone.
fn combine_effect(held: &mut StatusEffect, amplifier: u8, duration: i32) {
    if amplifier > held.amplifier {
        held.amplifier = amplifier;
        held.duration = duration;
    } else if amplifier == held.amplifier && held.duration < duration {
        held.duration = duration;
    }
}

/// The store's effects, ascending by effect id.
fn effect_list(effects: &BTreeMap<u8, StatusEffect>) -> Vec<StatusEffect> {
    effects.values().copied().collect()
}

/// Decodes the clientbound Held Item Change body: the slot byte alone.
fn decode_held_item_change(body: &[u8]) -> Result<i16, PacketError> {
    let [slot] = body else {
        return Err(PacketError::Codec(CodecError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "the held item change body is one slot byte",
        ))));
    };
    Ok(i16::from(*slot as i8))
}

/// Reports the player list when it changed against the set it started from.
///
/// The 0x38 arm hands its five actions a snapshot of the list: a packet that
/// leaves the list as it was reports nothing, however it spelled its entries.
fn report_list_change(player_list: &PlayerList, before: &PlayerList, events: &Sender<ClientEvent>) {
    if player_list == before {
        return;
    }
    report(
        events,
        ClientEvent::PlayerList {
            entries: player_list
                .iter()
                .map(|(_, record)| record.clone())
                .collect(),
        },
    );
}

/// Reports the scoreboard when it changed against the state it started from.
fn report_board_change(board: &Scoreboard, before: &Scoreboard, events: &Sender<ClientEvent>) {
    if board == before {
        return;
    }
    report(
        events,
        ClientEvent::ScoreboardChanged {
            board: board.clone(),
        },
    );
}

/// The client's receive rule for a Time Update's time of day: `WorldClient.setWorldTime`
/// (`WorldClient.java:468-481`) negates a negative time — the frozen-sun convention on the wire
/// (`S03PacketTimeUpdate.java:17-31`) — before the clock stores it, so the sky functions read
/// the positive value the source renders from: the wire's `-6000` is the `6000` of noon.
///
/// The source's `-time` is a two's-complement negation, and `wrapping_abs` is that rule on
/// every `i64`: the smallest one negates to itself rather than panicking the read loop. The
/// same branch also toggles the source's local `doDaylightCycle` rule — the sign it read is
/// kept on [`Clock`]'s `frozen` flag, which gates the tick's advance exactly as
/// `WorldClient.tick` gates it (`WorldClient.java:71-74`).
fn received_time_of_day(time_of_day: i64) -> i64 {
    time_of_day.wrapping_abs()
}

/// Reports the sky the clock and the view block produce, when both are known.
///
/// The angle and the colours come from the world clock; the block is the render-view entity's
/// own, floored exactly as `World.getSkyColor` floors it (`World.java:1437-1443`), and the
/// light level is that same block's (`EntityRenderer.java:362` reads
/// `World.getLightBrightness(new BlockPos(viewEntity))`). The frame fraction is the renderer's
/// own concern — the session reports whole ticks, and the window interpolates between them —
/// so the partial tick is zero, while the strengths are the session's live eased weather, and
/// the disc quad is the provider's answer for the frame's angle. A session with no clock yet —
/// or no world, as before Join Game — has no sky to report.
fn report_sky(
    world: Option<&World>,
    view: [f64; 3],
    clock: Option<&Clock>,
    weather: &WeatherState,
    events: &Sender<ClientEvent>,
) {
    let (Some(world), Some(clock)) = (world, clock) else {
        return;
    };
    let time_of_day = clock.time_of_day;
    /// The session reports whole ticks; interpolation is the renderer's.
    const PARTIAL_TICKS: f32 = 0.0;
    let angle = celestial_angle(time_of_day, PARTIAL_TICKS);
    let x = view[0].floor() as i32;
    let y = view[1].floor() as i32;
    let z = view[2].floor() as i32;
    let colour = sky_colour(world, x, y, z, angle, weather.rain);
    // The fog's brightness factor reads the view block's own light, with the source's
    // defaults for a position no column holds (`EntityRenderer.java:362-365`).
    let light_level = view_light_level(world, x, y, z);
    report(
        events,
        ClientEvent::Sky {
            celestial_angle: angle,
            colour,
            sun_brightness: sun_brightness(time_of_day, PARTIAL_TICKS, weather.rain),
            star_brightness: star_brightness(time_of_day, PARTIAL_TICKS),
            cloud_colour: cloud_colour(time_of_day, PARTIAL_TICKS, weather.rain),
            // `moon_phase` answers `0..8`, so the narrowing cannot lose a case.
            moon_phase: moon_phase(time_of_day) as u8,
            light_level,
            horizon: sunrise_sunset_colours(angle),
        },
    );
}

/// One finished build: the job it answers and the column's sixteen sections.
type MeshResult = (MeshJob, Vec<(usize, Option<ChunkMesh>)>);

/// Marks an applied column dirty together with each of its loaded neighbours.
///
/// A neighbour's snapshot collar reads the applied column, so its boundary
/// faces and light change when the column lands: the queue rebuilds it too,
/// which is M1's boundary refresh carried onto the queue and the rule the
/// plan's Task 13 interfaces state. A neighbour that is not loaded is not
/// marked: it has no mesh to refresh, and the burst's reported set stays the
/// columns the server sent. An unload is the reverse case and goes through
/// [`MeshQueue::mark_column_unloaded`], which marks all four neighbours.
fn mark_column_changed(store: &World, queue: &mut MeshQueue, cx: i32, cz: i32) {
    queue.mark_dirty(cx, cz);
    for (nx, nz) in [(cx + 1, cz), (cx - 1, cz), (cx, cz + 1), (cx, cz - 1)] {
        if store.chunk(nx, nz).is_some() {
            queue.mark_dirty(nx, nz);
        }
    }
}

/// Marks every loaded column a recompute over `(cx, cz)` rewrote dirty.
///
/// The region is the changed column and its eight neighbours, so this is
/// [`mark_column_changed`] plus the four diagonal columns: their light is
/// rewritten too, even though no collar reads the changed column, so their
/// meshes go stale as well.
fn mark_recompute_region(store: &World, queue: &mut MeshQueue, cx: i32, cz: i32) {
    mark_column_changed(store, queue, cx, cz);
    for (dx, dz) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
        if store.chunk(cx + dx, cz + dz).is_some() {
            queue.mark_dirty(cx + dx, cz + dz);
        }
    }
}

thread_local! {
    /// The light recompute passes the sessions on this thread have run.
    static RECOMPUTE_PASSES: Cell<u64> = const { Cell::new(0) };
}

/// The number of light recompute passes this thread's sessions have run.
///
/// A session runs its light work on the thread that drives it; the count is
/// per thread and monotonic, so a caller reads it around a run to see how many
/// passes that run cost, and sessions running in parallel do not see each
/// other's work. The replay tests read it to pin the block-change pipeline's
/// pass count.
pub fn recompute_passes() -> u64 {
    RECOMPUTE_PASSES.with(Cell::get)
}

/// Runs one light recompute pass over the changed column, counting it for
/// [`recompute_passes`].
///
/// This is the session's only route to [`light::recompute`], so the count and
/// the engine's own work cannot drift apart.
fn recompute_light(store: &mut World, x: i32, y: i32, z: i32) {
    RECOMPUTE_PASSES.with(|passes| passes.set(passes.get() + 1));
    light::recompute(store, x, y, z);
}

/// Air's block value: id 0 at metadata 0 — the packed `id << 4 | meta` zero
/// (`Section`'s default for a cell no section holds).
const AIR: u16 = 0;

/// The block value a placement predicts with until M5's inventory.
///
/// The packet carries an empty held item stack, and the server places from
/// the stack it holds for the account (`NetHandlerPlayServer
/// .processPlayerBlockPlacement`, `:582`), which this client cannot see — the
/// acceptance rig arranges a full cube with `/give`, so placements predict
/// the plain stone the rig gives, id 1 at metadata 0. A different held block
/// shows as the server's own 0x23 replacing the prediction.
pub const ASSUMED_HELD_BLOCK: u16 = 1 << 4;

/// The block facts one dig step reads at the aim: the aimed block, the face
/// and the hand's rate from the behaviour table.
///
/// The rate is [`hand_rate`] over the row's hardness and the material's own
/// harvest rule. An id outside the covered set has no row and is treated as
/// unbreakable — rate 0 — rather than guessed at.
fn dig_aim(world: &World, aim: Aim) -> DigAim {
    let id = world.block(aim.x, aim.y, aim.z) >> 4;
    let rate = match behaviour(id) {
        Some(row) => hand_rate(row.hardness, tool_not_required(row.material)),
        None => 0.0,
    };
    DigAim {
        x: aim.x,
        y: aim.y,
        z: aim.z,
        face: aim.face,
        rate,
    }
}

/// One tick of the digging input: the presses and the held state against the
/// current aim, in the source's order.
///
/// The source's tick runs `clickMouse` once per queued press and then
/// `sendClickBlockToController` (`Minecraft.java:1454-1460`); each step's
/// actions are the packets and local effects the machine produced, in their
/// own order. The presses are taken, so a press is delivered once. A held
/// button with no aim is the reset the source's else branch takes
/// (`:1515-1518`).
fn step_dig(
    world: Option<&World>,
    gamemode: u8,
    dig: &mut DigState,
    aim: Option<Aim>,
    presses: &mut u32,
    held: bool,
) -> Vec<DigAction> {
    let aimed = world.and_then(|world| aim.map(|aim| dig_aim(world, aim)));
    let creative = creative(gamemode);
    let mut actions = Vec::new();
    for _ in 0..std::mem::take(presses) {
        actions.extend(dig.click(aimed, creative));
    }
    if held {
        actions.extend(dig.on_player_damage_block(aimed, creative));
    } else {
        actions.extend(dig.reset_block_removing());
    }
    actions
}

/// The digging tint at a cell: the block's tint kind sampled over its own
/// nine-biome average, then the `setBlockPos` rule
/// (`EntityDiggingFX.setBlockPos` — the grass exception, not the
/// render-colour path).
fn dig_tint_at(store: &World, tint_maps: &TintMaps, x: i32, y: i32, z: i32, id: u16) -> [f32; 3] {
    let kind = behaviour(id).map(|row| row.tint).unwrap_or(TintKind::None);
    let sample = match kind {
        TintKind::None => [255, 255, 255],
        _ => tint_at_9_biome(tint_maps, x, y, z, kind, |bx, bz| {
            store
                .chunk(
                    bx.div_euclid(SECTION_SIZE as i32),
                    bz.div_euclid(SECTION_SIZE as i32),
                )
                .map(|column| {
                    column.biome(
                        bx.rem_euclid(SECTION_SIZE as i32) as usize,
                        bz.rem_euclid(SECTION_SIZE as i32) as usize,
                    )
                })
                .unwrap_or(0)
        }),
    };
    dig_tint(id, kind, sample)
}

/// Performs one dig action: its packet goes out, or its local effect lands,
/// in the order the machine produced them.
///
/// The completion's order is the source's own (`PlayerControllerMP.java:324-325`):
/// the finish is written before [`apply_block_change`] removes the block
/// locally, so the local path waits on no server round trip; the stage that
/// follows carries the reset progress's removal. An abort also clears the
/// breaker's entry: the source drops the breaker's damage entry as the abort
/// goes out (`sendBlockBreakProgress` with a negative progress,
/// `PlayerControllerMP.java`:263, `:281`). A stage with a negative index
/// clears the breaker's entry; a set reports only when the stored value
/// changed. The breaker is the session's own entity id, named at Join Game;
/// it is absent only before a join, when no dig can run.
#[allow(clippy::too_many_arguments)]
fn apply_dig_action<S: Read + Write>(
    action: &DigAction,
    world: Option<&mut World>,
    queue: &mut MeshQueue,
    stages: &mut BreakStages,
    stage_counter: u32,
    breaker: Option<i32>,
    conn: &mut Conn<S>,
    events: &Sender<ClientEvent>,
    particles: &mut ParticleEngine,
    tint_maps: &TintMaps,
) -> Result<(), SessionError> {
    match *action {
        DigAction::Start { x, y, z, face } => send_reply(
            conn,
            &tick_payload(|out| {
                write_player_digging(out, DiggingStatus::Start, x, y, z, face.wire())
            }),
        ),
        DigAction::Abort { x, y, z, face } => {
            send_reply(
                conn,
                &tick_payload(|out| {
                    write_player_digging(out, DiggingStatus::Abort, x, y, z, face.wire())
                }),
            )?;
            if let Some(breaker) = breaker {
                if stages.clear(breaker) {
                    report(events, ClientEvent::BreakCleared { breaker, x, y, z });
                }
            }
            Ok(())
        }
        DigAction::Finish { x, y, z, face } => send_reply(
            conn,
            &tick_payload(|out| {
                write_player_digging(out, DiggingStatus::Finish, x, y, z, face.wire())
            }),
        ),
        DigAction::Swing => send_reply(conn, &tick_payload(|out| write_animation(out))),
        DigAction::Hit { x, y, z, face } => {
            if let Some(store) = world {
                // The caller's own gates (`sendClickBlockToController:1506-1516`
                // over `addBlockHitEffects:395-442`): air never scatters, and
                // a block that draws nothing scatters nothing.
                let value = store.block(x, y, z);
                let id = value >> 4;
                if id != 0 && renderable_id(id) {
                    let tint = dig_tint_at(store, tint_maps, x, y, z, id);
                    let spawn = dig_hit(particles, [x, y, z], face, value, tint, FULL_CUBE_BOUNDS);
                    particles.spawn_local(spawn);
                }
            }
            Ok(())
        }
        DigAction::DamageClick { x, y, z } => {
            if let Some(store) = world {
                // The damage tick's click beside the hit particle
                // (`PlayerControllerMP.onPlayerDamageBlock:311-319`): the
                // block's own step event at its centre, at the click volume
                // and halved pitch, ahead of whatever the tick removes.
                let id = store.block(x, y, z) >> 4;
                if id != 0 && renderable_id(id) {
                    let group = SoundGroup::of_block(id);
                    report(
                        events,
                        ClientEvent::LocalSound {
                            name: group.step_sound().to_string(),
                            pos: [x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5],
                            volume: group.click_volume(),
                            pitch: group.click_pitch(),
                            category_hint: SoundCategory::Blocks,
                        },
                    );
                }
            }
            Ok(())
        }
        DigAction::Destroy { x, y, z } => {
            if let Some(store) = world {
                // The break sound: the broken block's own break event at its
                // centre, ahead of the removal that carries the id away
                // (`RenderGlobal.playAuxSFX 2001:2275-2282`).
                let value = store.block(x, y, z);
                let group = SoundGroup::of_block(value >> 4);
                report(
                    events,
                    ClientEvent::LocalSound {
                        name: group.break_sound().to_string(),
                        pos: [x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5],
                        volume: group.break_volume(),
                        pitch: group.break_pitch(),
                        category_hint: SoundCategory::Blocks,
                    },
                );
                // The destroy fan-out beside the staged sound half, from the
                // same pre-removal read (`addBlockDestroyEffects:369-393`).
                let tint = dig_tint_at(store, tint_maps, x, y, z, value >> 4);
                for spawn in dig_destroy([x, y, z], value, tint) {
                    particles.spawn_local(spawn);
                }
                apply_block_change(store, queue, x, y, z, AIR);
            }
            Ok(())
        }
        DigAction::Stage { x, y, z, index } => {
            if let Some(breaker) = breaker {
                if index >= 0 {
                    if stages.insert(breaker, [x, y, z], index as u8, stage_counter) {
                        report(
                            events,
                            ClientEvent::BreakStage {
                                breaker,
                                x,
                                y,
                                z,
                                stage: index as u8,
                            },
                        );
                    }
                } else if stages.clear(breaker) {
                    report(events, ClientEvent::BreakCleared { breaker, x, y, z });
                }
            }
            Ok(())
        }
    }
}

/// One tick of the use-entity input: when the frame's pick stops on an
/// entity nearer than the block aim, every queued press sends 0x02 and no
/// press reaches the block path.
///
/// The source's path is `rightClickMouse` (`Minecraft.java:1570-1603`): the
/// `ENTITY` case runs first (`:1583-1592`) — `isPlayerRightClickingOnEntity`
/// queues the `INTERACT_AT` send (`PlayerControllerMP.java:523-529`), else
/// `interactWithEntitySendPacket` queues the `INTERACT` send (`:509-514`) —
/// and either success sets `flag = false`, which skips the block case, so an
/// entity target consumes the click. This port sends the `INTERACT` form
/// (`C02PacketUseEntity.java:22-27`): the id plus the zero ordinal, no hit
/// vector (`writePacketData`, `:58-68` writes it for `INTERACT_AT` only).
/// The server opens the villager's merchant window and the horse's inventory
/// from `INTERACT` (`NetHandlerPlayServer.processUseEntity`, `:918-921`
/// calls `playerEntity.interactWith`), so the standing screen routes fire.
fn step_use_entity<S: Read + Write>(
    entities: &Entities,
    player: &Player,
    gamemode: u8,
    aim: Option<Aim>,
    presses: &mut u32,
    hitting: bool,
    conn: &mut Conn<S>,
) -> Result<(), SessionError> {
    if hitting || *presses == 0 {
        return Ok(());
    }
    let eye = [
        player.position[0],
        player.position[1] + player.eye_height(),
        player.position[2],
    ];
    let block_distance = aim.map(|aim| {
        let dx = aim.hit[0] - eye[0];
        let dy = aim.hit[1] - eye[1];
        let dz = aim.hit[2] - eye[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    });
    let Some(target) = pick_entity_target(
        entities,
        eye,
        look_vector(player.yaw, player.pitch),
        gamemode,
        block_distance,
    ) else {
        return Ok(());
    };
    for _ in 0..std::mem::take(presses) {
        send_reply(
            conn,
            &tick_payload(|out| write_use_entity(out, target, UseEntityAction::Interact, None)),
        )?;
    }
    Ok(())
}

/// One tick of the placement input: each queued press, when an aim exists,
/// runs the source's checks, sends 0x08 and predicts the block locally; a
/// press with no aim falls through to the source's use-item path and sends
/// the no-target 0x08.
///
/// The source's path is `rightClickMouse` (`Minecraft.java:1570-1638`): it
/// refuses while a dig runs (`:1572`'s `getIsHittingBlock`), warns on a null
/// hit result (`:1577-1581`), tries the `ENTITY` case first (`:1583-1592`,
/// run here by the use-entity step before this one), then the `BLOCK` case
/// (`:1594-1625`, the aimed send below). Whatever is still flagged — a miss,
/// an air cell, a failed activation — falls through to `sendUseItem`
/// (`:1628-1636`), which queues the no-target 0x08 for any non-null held
/// stack and refuses spectator alone (`PlayerControllerMP.sendUseItem`,
/// `:456-482`). The no-target composition is the single-argument constructor's
/// (`C08PacketPlayerBlockPlacement.java:24-27`): `field_179726_a`
/// (-1, -1, -1), direction 255, cursor 0,0,0 (`:13`).
///
/// That method refuses a target that cannot take the block before
/// the packet (`:417-421`), queues 0x08 (`:424`) and then lets
/// `onItemUse` write the block locally (`:436`, `:443`) — so the packet goes
/// out before the prediction lands, the order the two calls run here.
///
/// The prediction's value is [`ASSUMED_HELD_BLOCK`]: the packet's held stack
/// is empty, and the server places from its own copy of the held item
/// (`NetHandlerPlayServer.processPlayerBlockPlacement`, `:582`). The
/// no-target send keeps the same empty stack: the server's direction-255 arm
/// reads its own held copy too (`:585-591`), so the book still opens there.
#[allow(clippy::too_many_arguments)]
fn step_place<S: Read + Write>(
    mut world: Option<&mut World>,
    queue: &mut MeshQueue,
    aim: Option<Aim>,
    held: &Option<MetadataItem>,
    gamemode: u8,
    presses: &mut u32,
    hitting: bool,
    conn: &mut Conn<S>,
    events: &Sender<ClientEvent>,
) -> Result<(), SessionError> {
    let presses = std::mem::take(presses);
    if hitting {
        return Ok(());
    }
    for _ in 0..presses {
        let (Some(store), Some(aim)) = (world.as_deref_mut(), aim) else {
            // No block target: the source's `sendUseItem` fallthrough. The
            // held stack gates it (`rightClickMouse:1632`'s null check) and
            // spectator refuses it (`sendUseItem:456-460`); the cursor is
            // 0,0,0, the packet's own default.
            if !spectator(gamemode) && held.is_some() {
                send_reply(
                    conn,
                    &tick_payload(|out| {
                        write_player_block_placement(out, -1, -1, -1, 255, [0, 0, 0])
                    }),
                )?;
            }
            continue;
        };
        let Some(placed) = placement(&WorldView(store), &aim) else {
            continue;
        };
        send_reply(
            conn,
            &tick_payload(|out| {
                write_player_block_placement(
                    out,
                    placed.x,
                    placed.y,
                    placed.z,
                    placed.face.wire(),
                    placed.cursor,
                )
            }),
        )?;
        apply_block_change(
            store,
            queue,
            placed.target[0],
            placed.target[1],
            placed.target[2],
            ASSUMED_HELD_BLOCK,
        );
        // The place sound: the predicted block's own place event at its
        // centre (`ItemBlock.onItemUse:71`).
        let group = SoundGroup::of_block(ASSUMED_HELD_BLOCK >> 4);
        report(
            events,
            ClientEvent::LocalSound {
                name: group.place_sound().to_string(),
                pos: [
                    placed.target[0] as f32 + 0.5,
                    placed.target[1] as f32 + 0.5,
                    placed.target[2] as f32 + 0.5,
                ],
                volume: group.break_volume(),
                pitch: group.break_pitch(),
                category_hint: SoundCategory::Blocks,
            },
        );
    }
    Ok(())
}

/// Applies one block change locally: the world write, the light it needs and
/// the mesh invalidation of every column the recompute rewrote.
///
/// This is the pipeline clientbound 0x23 and every record of 0x22 land
/// through, and the one Tasks 8 and 9's prediction paths reuse verbatim: the
/// write (which materialises a section the column does not hold as its air
/// defaults), the light recompute over the changed column and its eight
/// neighbours ([`light::recompute`], the region the source's `checkLightFor`
/// neighbourhood derives), and the invalidation of every loaded column of
/// that region ([`mark_recompute_region`] — the light of all nine can change,
/// and the changed column's four orthogonal neighbours' meshes also read its
/// blocks through their collar).
///
/// A write that does not land — an unloaded column, a y outside the build
/// range — changes nothing, so there is no light to recompute and no mesh to
/// invalidate, and the change is dropped.
fn apply_block_change(
    store: &mut World,
    queue: &mut MeshQueue,
    x: i32,
    y: i32,
    z: i32,
    value: u16,
) {
    if store.set_block(x, y, z, value).is_none() {
        return;
    }
    recompute_light(store, x, y, z);
    let cx = x.div_euclid(SECTION_SIZE as i32);
    let cz = z.div_euclid(SECTION_SIZE as i32);
    mark_recompute_region(store, queue, cx, cz);
}

/// Re-reads the block at a mapped sign position after the store moved: a
/// sign block refreshes the entry's variant and orientation (re-reported so
/// the view's copy follows), anything else drops the entry and reports the
/// clear. Unmapped positions, and refreshes that change nothing, answer
/// false.
fn refresh_sign_at(
    store: &World,
    signs: &mut BTreeMap<(i32, i32, i32), SignText>,
    events: &Sender<ClientEvent>,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    let (lines, old_id, old_meta) = match signs.get(&(x, y, z)) {
        Some(entry) => (entry.lines.clone(), entry.block_id, entry.metadata),
        None => return false,
    };
    let packed = store.block(x, y, z);
    let (block_id, metadata) = (block_id_of(packed), block_meta_of(packed));
    if !is_sign_block(block_id) {
        signs.remove(&(x, y, z));
        report(events, ClientEvent::SignTextCleared { x, y, z });
        return true;
    }
    if (old_id, old_meta) == (block_id, metadata) {
        return false;
    }
    signs.insert(
        (x, y, z),
        SignText {
            lines: lines.clone(),
            block_id,
            metadata,
        },
    );
    report(
        events,
        ClientEvent::SignTextChanged {
            x,
            y,
            z,
            lines,
            block_id,
            metadata,
        },
    );
    true
}

/// Re-reads every mapped sign in the column after a chunk or multi-block
/// write landed.
fn refresh_sign_column(
    store: &World,
    signs: &mut BTreeMap<(i32, i32, i32), SignText>,
    events: &Sender<ClientEvent>,
    cx: i32,
    cz: i32,
) {
    let positions: Vec<(i32, i32, i32)> = signs
        .keys()
        .filter(|(x, _, z)| {
            x.div_euclid(SECTION_SIZE as i32) == cx && z.div_euclid(SECTION_SIZE as i32) == cz
        })
        .copied()
        .collect();
    for (x, y, z) in positions {
        refresh_sign_at(store, signs, events, x, y, z);
    }
}

/// Drops every mapped sign in the column, reporting each clear: the column
/// unloaded (or arrived empty), and the server re-sends the tile entities
/// with the next load.
fn drop_sign_column(
    signs: &mut BTreeMap<(i32, i32, i32), SignText>,
    events: &Sender<ClientEvent>,
    cx: i32,
    cz: i32,
) {
    let positions: Vec<(i32, i32, i32)> = signs
        .keys()
        .filter(|(x, _, z)| {
            x.div_euclid(SECTION_SIZE as i32) == cx && z.div_euclid(SECTION_SIZE as i32) == cz
        })
        .copied()
        .collect();
    for (x, y, z) in positions {
        signs.remove(&(x, y, z));
        report(events, ClientEvent::SignTextCleared { x, y, z });
    }
}

/// The refusal a Multi Block Change coordinate that cannot compose a block
/// position is answered with.
///
/// The chunk coordinate is a raw `i32` off the wire and the composition
/// `chunk * 16 + local` must not wrap (release) or panic (a checked build), so
/// a value outside the composition's range refuses the packet as a malformed
/// one, on the error shape the decoders refuse their own fields with.
fn coordinate_refusal(chunk_x: i32, chunk_z: i32) -> SessionError {
    warn!(
        chunk_x = chunk_x,
        chunk_z = chunk_z,
        "a multi block change's chunk coordinate cannot compose a block position"
    );
    SessionError::Packet(PacketError::Codec(CodecError::Io(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("chunk coordinate ({chunk_x}, {chunk_z}) cannot compose a block position"),
    ))))
}

/// Refuses a column whose chunk coordinate is outside the range the world
/// carries ([`World::accepts_chunk_coordinate`]) before anything of it
/// applies.
///
/// The coordinate is a raw `i32` off the wire, and the store and its light
/// engine only compose positions within that range: a loaded column outside
/// it would drive the engine's region arithmetic past `i32` on its next
/// block change, which a checked build panics on. The refusal is the
/// decoders' own malformed-field shape, and it precedes every write the
/// column feeds, so a refused packet applies nothing.
fn check_column_coordinate(cx: i32, cz: i32) -> Result<(), SessionError> {
    if World::accepts_chunk_coordinate(cx, cz) {
        return Ok(());
    }
    warn!(
        cx = cx,
        cz = cz,
        "a chunk coordinate is outside the range the world's light region carries"
    );
    Err(SessionError::Packet(PacketError::Codec(CodecError::Io(
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("chunk coordinate ({cx}, {cz}) is outside the supported range"),
        ),
    ))))
}

/// Applies one Multi Block Change: every record's write, then a single light
/// pass over the union of the changed cells' recompute regions, then the mesh
/// invalidation that pass forces.
///
/// Every record composes into the packet's own chunk — the decoder masks each
/// record's local coordinates to 0..16 — so the regions the records' changes
/// own coincide, and one pass over the packet's region is the union of them. A
/// from-scratch pass depends only on the final blocks, so the batch leaves the
/// light a pass sequence per record would leave, at one pass instead of one
/// per record. The invalidation is the same union: [`mark_recompute_region`]
/// for the changed column, its loaded orthogonal neighbours and the region's
/// loaded diagonals.
///
/// The composition is checked: a record whose coordinate cannot compose a
/// block position refuses the packet ([`coordinate_refusal`]) rather than
/// wrapping or panicking. A record whose write does not land — an unloaded
/// column, a y outside the build range — changes nothing, exactly as it does
/// through [`apply_block_change`].
fn apply_multi_block_change(
    store: &mut World,
    queue: &mut MeshQueue,
    chunk_x: i32,
    chunk_z: i32,
    updates: &[BlockUpdate],
) -> Result<(), SessionError> {
    let Some(base_x) = chunk_x.checked_mul(SECTION_SIZE as i32) else {
        return Err(coordinate_refusal(chunk_x, chunk_z));
    };
    let Some(base_z) = chunk_z.checked_mul(SECTION_SIZE as i32) else {
        return Err(coordinate_refusal(chunk_x, chunk_z));
    };
    // The writes first: the one pass below reads the batch's final blocks,
    // which is what lets it cover every record at once.
    let mut changed: Option<(i32, i32, i32)> = None;
    for update in updates {
        let (Some(x), Some(z)) = (base_x.checked_add(update.x), base_z.checked_add(update.z))
        else {
            return Err(coordinate_refusal(chunk_x, chunk_z));
        };
        if store.set_block(x, update.y, z, update.value).is_some() && changed.is_none() {
            changed = Some((x, update.y, z));
        }
    }
    let Some((x, y, z)) = changed else {
        return Ok(());
    };
    recompute_light(store, x, y, z);
    let cx = x.div_euclid(SECTION_SIZE as i32);
    let cz = z.div_euclid(SECTION_SIZE as i32);
    mark_recompute_region(store, queue, cx, cz);
    Ok(())
}

/// Builds the session's mesh pool: one worker fewer than the machine's
/// parallelism, at least one.
///
/// The session thread keeps a core for reading and for copying snapshots; the
/// rest build meshes. A machine that reports a single processor gets a
/// one-thread pool rather than none, and a failed query falls back to one.
fn mesh_pool() -> Result<ThreadPool, ThreadPoolBuildError> {
    let threads = std::thread::available_parallelism()
        .map(|available| available.get().saturating_sub(1).max(1))
        .unwrap_or(1);
    ThreadPoolBuilder::new().num_threads(threads).build()
}

/// Hands the queue's next columns to the pool and reports every finished build.
///
/// Called when the read found nothing for a whole [`MESH_TICK`] — and by
/// [`finish_meshes`], which drains the queue on a clean stop — so the work
/// here never delays a frame that was already readable. One pass spawns at
/// most [`PENDING_JOBS_CAP`] builds: the snapshot copy happens here, on the
/// session's thread — the world never leaves it — and the cap is what bounds
/// this thread's own work for one idle wait. The build itself reads only the
/// snapshot and the shared assets. The results are then taken with a
/// non-blocking receive: a build that is not finished yet is picked up by a
/// later pass, never waited for.
// One argument per owner of the pump's state; bundling them into a struct
// would only move the same list one level down.
#[allow(clippy::too_many_arguments)]
fn pump_meshes(
    world: Option<&World>,
    queue: &mut MeshQueue,
    pool: &ThreadPool,
    mesh: &Arc<MeshAssets>,
    finished: &Sender<MeshResult>,
    results: &Receiver<MeshResult>,
    events: &Sender<ClientEvent>,
    graphics_fast: bool,
    smooth_lighting: SmoothLighting,
) {
    if let Some(world) = world {
        for _ in 0..PENDING_JOBS_CAP {
            let Some(job) = queue.next_job() else {
                break;
            };
            queue.mark_running(job);
            let snapshot = ColumnSnapshot::from_world(world, job.cx, job.cz);
            let assets = Arc::clone(mesh);
            let finished = finished.clone();
            pool.spawn(move || {
                let ctx = mesh_context(&assets, graphics_fast, smooth_lighting);
                let sections = build_column_meshes(&snapshot, &ctx);
                let _ = finished.send((job, sections));
            });
        }
    }
    drain_meshes(queue, results, events);
}

/// Reports every finished build that is waiting; a stale one is discarded.
///
/// The receive never blocks, so a pass over the channel is bounded by what has
/// already arrived.
fn drain_meshes(
    queue: &mut MeshQueue,
    results: &Receiver<MeshResult>,
    events: &Sender<ClientEvent>,
) {
    while let Ok((job, sections)) = results.try_recv() {
        if queue.complete(job) {
            report(
                events,
                ClientEvent::ChunkUpdated {
                    cx: job.cx,
                    cz: job.cz,
                    sections,
                },
            );
        }
    }
}

/// Finishes the outstanding builds at the end of a session, within a bound.
///
/// A clean stop still reports its last meshes: the queue's columns go to the
/// pool and the results are drained until nothing is outstanding or
/// [`END_OF_SESSION_WAIT`] passes. The bound keeps a wedged build from holding
/// the session's end forever; the pool is dropped when the session returns.
/// One argument per owner of the finish's state; bundling them into a struct
/// would only move the same list one level down.
#[allow(clippy::too_many_arguments)]
fn finish_meshes(
    world: Option<&World>,
    queue: &mut MeshQueue,
    pool: &ThreadPool,
    mesh: &Arc<MeshAssets>,
    finished: &Sender<MeshResult>,
    results: &Receiver<MeshResult>,
    events: &Sender<ClientEvent>,
    graphics_fast: bool,
    smooth_lighting: SmoothLighting,
) {
    if world.is_none() {
        // No world was ever joined, so nothing can be queued for it.
        return;
    }
    let deadline = Instant::now() + END_OF_SESSION_WAIT;
    loop {
        pump_meshes(
            world,
            queue,
            pool,
            mesh,
            finished,
            results,
            events,
            graphics_fast,
            smooth_lighting,
        );
        if queue.pending() == 0 || Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    //! The tick period and the frozen clock gate, against synthetic state.

    use std::time::{Duration, Instant};

    use oxide_world::entity::Entities;
    use oxide_world::world::World;

    use super::{
        ASSUMED_HELD_BLOCK, CHAT_FIELD_CAP, ClientEvent, Clock, END_OF_SESSION_WAIT, GRAPHICS_FAST,
        INPUTS_PER_PASS, MOUSE_SENSITIVITY, MeshAssets, MeshQueue, PORTAL_BLOCK_ID,
        SMOOTH_LIGHTING, Session, SessionConfig, SmoothLighting, TICK_PERIOD, WEATHER_EASE_RATE,
        WeatherState, apply_input, apply_session_push, chat_over_cap, eye_block_event,
        finish_meshes, gate_cell, hand_cell, mesh_pool, renderable_id, smooth_lighting_of,
        step_tick, visually_opaque_id,
    };
    use crate::entity_view::PlayerList;
    use crate::input::{InputEvent, Intent, SessionPush};
    use crate::particles::ParticleEngine;
    use crate::player::{MAX_HURT_TIME, Player};
    use crate::scoreboard::Scoreboard;
    use crate::settings::{CHAT_MODE_RANGE, ParticleSetting, VIEW_DISTANCE_RANGE};
    use crate::ticker::Ticker;
    use oxide_proto_v47::serverbound::ClientSettings;
    use std::io::{Cursor, Read, Write};
    use std::sync::Arc;
    use std::sync::Mutex;

    #[test]
    fn the_input_backlog_drains_thirty_two_events_per_pass() {
        // The bound is port-only: the source's input arrives from the window
        // it already owns, so no source pass caps it. The backlog still
        // drains across passes and nothing is dropped; the bound only keeps
        // one pass' work finite when the window outruns the session.
        assert_eq!(
            INPUTS_PER_PASS, 32,
            "one pass drains thirty-two inputs before yielding"
        );
    }

    #[test]
    fn the_tick_period_is_fifty_milliseconds() {
        // `new Timer(20.0F)` (`Minecraft.java:223`): twenty steps per second.
        assert_eq!(
            TICK_PERIOD,
            Duration::from_millis(50),
            "one tick every 50 ms is the source's 20 Hz"
        );
        // The value drives the scheduler: a synthetic instant inside a period
        // reports nothing, and a hair past it reports the one whole step.
        let start = Instant::now();
        let mut ticker = Ticker::new(TICK_PERIOD);
        assert_eq!(ticker.due(start + Duration::from_millis(49)), 0);
        assert_eq!(
            ticker.due(start + TICK_PERIOD + Duration::from_millis(5)),
            1
        );
    }

    /// A held settings push and the mapping it runs through.
    fn held_push() -> (
        ClientSettings,
        bool,
        SmoothLighting,
        f32,
        bool,
        ParticleEngine,
    ) {
        (
            ClientSettings::default(),
            GRAPHICS_FAST,
            SMOOTH_LIGHTING,
            MOUSE_SENSITIVITY,
            false,
            ParticleEngine::seeded(0x9E37_79B9_7F4A_7C15, 8),
        )
    }

    fn full_push() -> SessionPush {
        SessionPush {
            settings: ClientSettings {
                locale: "de_DE".to_string(),
                view_distance: 10,
                chat_mode: 1,
                chat_colors: false,
                skin_parts: 0x01,
            },
            graphics_fast: false,
            smooth_lighting: 0,
            sensitivity: 1.0,
            invert_mouse: true,
            particles: ParticleSetting::Minimal,
        }
    }

    #[test]
    fn the_smooth_lighting_mapping_matches_the_option_order() {
        // `AMBIENT_OCCLUSIONS` (`GameSettings.java:59`): off, minimum,
        // maximum; past-the-end values clamp to Maximum, the renderer's own
        // `set_smooth_lighting` rule.
        assert_eq!(smooth_lighting_of(0), SmoothLighting::Off);
        assert_eq!(smooth_lighting_of(1), SmoothLighting::Minimum);
        assert_eq!(smooth_lighting_of(2), SmoothLighting::Maximum);
        assert_eq!(smooth_lighting_of(9), SmoothLighting::Maximum);
    }

    #[test]
    fn a_settings_push_moves_every_session_held_value() {
        let (
            mut settings,
            mut graphics_fast,
            mut smooth_lighting,
            mut sensitivity,
            mut invert_mouse,
            mut particles,
        ) = held_push();
        let changed = apply_session_push(
            &mut settings,
            &mut graphics_fast,
            &mut smooth_lighting,
            &mut sensitivity,
            &mut invert_mouse,
            &mut particles,
            &full_push(),
        );
        assert!(
            changed,
            "a changed wire payload reports back for the C15 re-send"
        );
        assert_eq!(settings.view_distance, 10);
        assert_eq!(settings.locale, "de_DE");
        assert_eq!(settings.chat_mode, 1);
        assert!(!settings.chat_colors);
        assert_eq!(settings.skin_parts, 0x01);
        assert!(!graphics_fast, "the mesher leaves the Fast path");
        assert_eq!(smooth_lighting, SmoothLighting::Off);
        assert_eq!(sensitivity, 1.0, "the look mapping runs the pushed value");
        assert!(invert_mouse, "the look mapping runs the pushed invert flag");
        assert_eq!(particles.setting(), ParticleSetting::Minimal);
    }

    #[test]
    fn an_identical_settings_push_sends_nothing_but_still_applies() {
        let (
            mut settings,
            mut graphics_fast,
            mut smooth_lighting,
            mut sensitivity,
            mut invert_mouse,
            mut particles,
        ) = held_push();
        let push = SessionPush {
            settings: settings.clone(),
            graphics_fast,
            smooth_lighting: 2,
            sensitivity,
            invert_mouse,
            particles: ParticleSetting::All,
        };
        let changed = apply_session_push(
            &mut settings,
            &mut graphics_fast,
            &mut smooth_lighting,
            &mut sensitivity,
            &mut invert_mouse,
            &mut particles,
            &push,
        );
        assert!(!changed, "an identical wire payload sends no C15");
    }

    #[test]
    fn a_settings_push_clamps_into_the_store_ranges() {
        let (
            mut settings,
            mut graphics_fast,
            mut smooth_lighting,
            mut sensitivity,
            mut invert_mouse,
            mut particles,
        ) = held_push();
        let mut push = full_push();
        push.settings.view_distance = 99;
        push.settings.chat_mode = 7;
        push.smooth_lighting = 9;
        push.sensitivity = 9.0;
        let changed = apply_session_push(
            &mut settings,
            &mut graphics_fast,
            &mut smooth_lighting,
            &mut sensitivity,
            &mut invert_mouse,
            &mut particles,
            &push,
        );
        assert!(changed);
        assert_eq!(settings.view_distance, VIEW_DISTANCE_RANGE.1);
        assert_eq!(settings.chat_mode, CHAT_MODE_RANGE.1);
        assert_eq!(smooth_lighting, SmoothLighting::Maximum);
        assert_eq!(sensitivity, 1.0);
    }

    #[test]
    fn the_look_runs_the_pushed_sensitivity() {
        // The `f³ × 8` law (`input.rs:524+`): at the 0.5 default ten pixels
        // turn 1.5°, at 1.0 they turn 6.144°.
        let mut intent = Intent::neutral();
        let mut player = Player::new();
        let looked = apply_input(
            InputEvent::MouseDelta { dx: 10.0, dy: 0.0 },
            &mut intent,
            &mut player,
            1.0,
            false,
        );
        assert!(looked, "a look asks for the aim to be recomputed");
        assert!((player.yaw - 6.144).abs() < 1e-4, "yaw {}", player.yaw);
        let mut intent = Intent::neutral();
        let mut player = Player::new();
        apply_input(
            InputEvent::MouseDelta { dx: 10.0, dy: 0.0 },
            &mut intent,
            &mut player,
            0.5,
            false,
        );
        assert!((player.yaw - 1.5).abs() < 1e-4, "yaw {}", player.yaw);
    }

    #[test]
    fn the_inverted_look_flips_the_pitch_sign() {
        // The invert-mouse option flips the pitch term
        // (`EntityRenderer.java`:1103-1107): a downward delta pitches down
        // straight and up inverted, while the yaw never moves.
        let mut intent = Intent::neutral();
        let mut player = Player::new();
        apply_input(
            InputEvent::MouseDelta { dx: 0.0, dy: 10.0 },
            &mut intent,
            &mut player,
            0.5,
            false,
        );
        assert!((player.pitch - 1.5).abs() < 1e-4, "pitch {}", player.pitch);
        assert_eq!(player.yaw, 0.0);
        let mut intent = Intent::neutral();
        let mut player = Player::new();
        apply_input(
            InputEvent::MouseDelta { dx: 0.0, dy: 10.0 },
            &mut intent,
            &mut player,
            0.5,
            true,
        );
        assert!((player.pitch + 1.5).abs() < 1e-4, "pitch {}", player.pitch);
        assert_eq!(player.yaw, 0.0);
    }

    #[test]
    fn a_settings_push_carries_the_invert_flag() {
        let (
            mut settings,
            mut graphics_fast,
            mut smooth_lighting,
            mut sensitivity,
            mut invert_mouse,
            mut particles,
        ) = held_push();
        assert!(!invert_mouse);
        let mut push = full_push();
        assert!(push.invert_mouse);
        let changed = apply_session_push(
            &mut settings,
            &mut graphics_fast,
            &mut smooth_lighting,
            &mut sensitivity,
            &mut invert_mouse,
            &mut particles,
            &push,
        );
        assert!(changed);
        assert!(invert_mouse, "the session holds the pushed invert flag");
        push.invert_mouse = false;
        apply_session_push(
            &mut settings,
            &mut graphics_fast,
            &mut smooth_lighting,
            &mut sensitivity,
            &mut invert_mouse,
            &mut particles,
            &push,
        );
        assert!(!invert_mouse, "clearing the option clears the held flag");
    }

    #[test]
    fn the_assumed_held_block_is_stone() {
        // The prediction's own value until M5's inventory: the packet's held
        // stack is empty and the server places from the stack it holds for
        // the account (`NetHandlerPlayServer.processPlayerBlockPlacement`,
        // `:582`), which the acceptance rig arranges as plain stone — id 1,
        // metadata 0 (`Blocks.stone`'s one variant).
        assert_eq!(ASSUMED_HELD_BLOCK, 1 << 4, "stone: id 1 at metadata 0");
    }

    /// One column holding exactly `blocks` (`(x, y, z, packed)` in world
    /// coordinates) for the eye-block probe tests.
    fn probe_world(blocks: &[(i32, i32, i32, u16)]) -> World {
        use oxide_proto_v47::column::{ColumnData, SectionData, block_index};
        let mut sections: [Option<SectionData>; 16] = std::array::from_fn(|_| None);
        for (x, y, z, value) in blocks {
            let section = (y >> 4) as usize;
            let slot = sections[section].get_or_insert(SectionData {
                blocks: Box::new([0u16; 4096]),
                block_light: Box::new([0u8; 2048]),
                sky_light: Some(Box::new([0u8; 2048])),
            });
            let local = block_index(
                x.rem_euclid(16) as usize,
                (y & 15) as usize,
                z.rem_euclid(16) as usize,
            );
            slot.blocks[local] = *value;
        }
        let mut world = World::new(true);
        assert!(
            world.apply_column(
                0,
                0,
                &ColumnData {
                    mask: 0xFFFF,
                    sections,
                    biomes: None,
                },
                true,
            ),
            "the probe column loads"
        );
        world
    }

    #[test]
    fn the_gate_probe_reads_y_from_the_low_bit() {
        // `Entity.isEntityInsideOpaqueBlock` (`Entity.java:1838-1840`): the Y
        // cell from `(i >> 0)`, X from `(i >> 1)`, Z from `(i >> 2)`. Feet
        // (8.0, 63.38, 8.0) with the standing eye (1.62) puts the low-bit
        // pair either side of cell 65, and the ±0.24 X/Z taps either side of
        // cell 8.
        let feet = [8.0, 63.38, 8.0];
        assert_eq!(
            gate_cell(0, feet, 1.62),
            (7, 64, 7),
            "index 0 taps the low corner"
        );
        assert_eq!(
            gate_cell(1, feet, 1.62),
            (7, 65, 7),
            "the low bit moves Y alone"
        );
        assert_eq!(gate_cell(2, feet, 1.62), (8, 64, 7), "bit 1 moves X alone");
        assert_eq!(gate_cell(4, feet, 1.62), (7, 64, 8), "bit 2 moves Z alone");
    }

    #[test]
    fn the_hand_probe_reads_x_from_the_low_bit() {
        // `ItemRenderer.renderOverlays` (`ItemRenderer.java:429-431`) swizzles
        // the other way: X from `(i >> 0)`, Y from `(i >> 1)`, Z from
        // `(i >> 2)`. Index 1 lands (8, 64, 7) here where the gate's lands
        // (7, 65, 7) — the two sites differ, and the port pins both.
        let feet = [8.0, 63.38, 8.0];
        assert_eq!(
            hand_cell(0, feet, 1.62),
            (7, 64, 7),
            "index 0 taps the low corner"
        );
        assert_eq!(
            hand_cell(1, feet, 1.62),
            (8, 64, 7),
            "the low bit moves X alone"
        );
        assert_eq!(hand_cell(2, feet, 1.62), (7, 65, 7), "bit 1 moves Y alone");
        assert_eq!(hand_cell(4, feet, 1.62), (7, 64, 8), "bit 2 moves Z alone");
    }

    #[test]
    fn the_opaque_rule_is_movement_and_a_full_cube() {
        // `Block.isVisuallyOpaque` (`Block.java:361-364`): the material's
        // `blocksMovement` and `isFullCube`. Stone (1) passes; air (0) and
        // the portal (90, no table row) fail; glass (20) fails on the cube.
        assert!(visually_opaque_id(1), "stone is visually opaque");
        assert!(!visually_opaque_id(0), "air is not");
        assert!(!visually_opaque_id(90), "the portal is not");
        assert!(!visually_opaque_id(20), "glass is not a full cube");
    }

    #[test]
    fn the_render_type_skip_is_air_and_the_barrier() {
        // `getRenderType() != -1` (`ItemRenderer.java:445`): air
        // (`BlockAir.java:19-22`) and the barrier (`BlockBarrier.java:22-25`)
        // answer -1; stone draws, and an untabled id like the portal (90)
        // reads the default 3 (`Block.java:379-382`).
        assert!(!renderable_id(0), "air is skipped");
        assert!(!renderable_id(166), "the barrier is skipped");
        assert!(renderable_id(1), "stone draws");
        assert!(renderable_id(90), "the portal reads the default type");
    }

    #[test]
    fn the_eye_read_reports_the_viewpoint_cell() {
        // The eye at feet (8.5, 65.0, 8.5) plus the standing 1.62 reads cell
        // (8, 66, 8); water there answers packed 128 with the gate shut and
        // no portal overlap.
        let mut player = Player::new();
        player.position = [8.5, 65.0, 8.5];
        let world = probe_world(&[(8, 66, 8, 8 << 4)]);
        let ClientEvent::EyeBlock {
            eye,
            in_portal,
            inside,
            ..
        } = eye_block_event(&player, Some(&world))
        else {
            panic!("the read reports EyeBlock");
        };
        assert_eq!(eye, 8 << 4, "the viewpoint cell's packed water");
        assert!(!in_portal, "no portal overlaps the box");
        assert_eq!(inside, None, "water is not visually opaque");
    }

    #[test]
    fn the_eye_read_steps_above_a_liquid_surface() {
        // The surface-plane rule (`ActiveRenderInfo.getBlockAtEntityViewpoint`,
        // `ActiveRenderInfo.java:84-107`): a level-6 water cell stands its
        // surface a third of the way up, so an eye just below the plane reads
        // the water and an eye just above it reads the air above.
        let mut player = Player::new();
        player.position = [8.5, 64.68, 8.5];
        let world = probe_world(&[(8, 66, 8, (8 << 4) | 6)]);
        let ClientEvent::EyeBlock { eye, .. } = eye_block_event(&player, Some(&world)) else {
            panic!("the read reports EyeBlock");
        };
        assert_eq!(eye, (8 << 4) | 6, "the eye below the plane reads the water");
        player.position = [8.5, 64.78, 8.5];
        let ClientEvent::EyeBlock { eye: above, .. } = eye_block_event(&player, Some(&world))
        else {
            panic!("the read reports EyeBlock");
        };
        assert_eq!(above, 0, "the eye past the plane reads the block above");
    }

    #[test]
    fn the_portal_overlap_reads_the_tick_box() {
        // A portal cell at the feet (8, 65, 8) overlaps the standing box, so
        // the tick reports `in_portal`; one cell over it does not.
        let mut player = Player::new();
        player.position = [8.5, 65.0, 8.5];
        let world = probe_world(&[(8, 65, 8, PORTAL_BLOCK_ID << 4)]);
        let ClientEvent::EyeBlock { in_portal, .. } = eye_block_event(&player, Some(&world)) else {
            panic!("the read reports EyeBlock");
        };
        assert!(in_portal, "the feet portal cell overlaps the box");
        let clear = probe_world(&[(9, 65, 8, PORTAL_BLOCK_ID << 4)]);
        let ClientEvent::EyeBlock {
            in_portal: absent, ..
        } = eye_block_event(&player, Some(&clear))
        else {
            panic!("the read reports EyeBlock");
        };
        assert!(!absent, "the neighbouring portal cell misses the box");
    }

    #[test]
    fn the_hand_probe_answers_the_last_opaque_state() {
        // Feet air, the eye-height ring stone: the gate opens and the last
        // opaque probe wins, packed 16.
        let mut player = Player::new();
        player.position = [8.5, 65.0, 8.5];
        let world = probe_world(&[(8, 66, 8, 1 << 4), (7, 66, 8, 1 << 4), (8, 66, 7, 1 << 4)]);
        let ClientEvent::EyeBlock { inside, .. } = eye_block_event(&player, Some(&world)) else {
            panic!("the read reports EyeBlock");
        };
        assert_eq!(inside, Some(1 << 4), "the last opaque state wins");
    }

    #[test]
    fn a_frozen_tick_advances_the_world_age_and_holds_the_time_of_day() {
        // `WorldClient.tick` advances the world age every tick and the time of
        // day only while the cycle runs (`WorldClient.java:66-74`); the frozen
        // flag is the sign the receive rule read (`:468-481`). One frozen
        // step: the age and the player's tick move, the time of day holds, and
        // no sky is reported — a step reports a sky exactly when it moved the
        // time of day.
        let world = World::new(true);
        let intent = Intent::neutral();
        let (sender, receiver) = crossbeam_channel::unbounded::<ClientEvent>();

        let mut player = Player::new();
        let mut frozen = Clock {
            world_age: 48_000,
            time_of_day: 6000,
            frozen: true,
        };
        step_tick(
            &mut player,
            &intent,
            Some(&mut frozen),
            Some(&world),
            &mut Entities::new(),
            &PlayerList::new(),
            &Scoreboard::new(),
            &sender,
            true,
            &WeatherState::default(),
            false,
        );
        assert_eq!(frozen.world_age, 48_001, "the age advances every tick");
        assert_eq!(frozen.time_of_day, 6000, "the frozen time of day holds");
        assert_eq!(player.tick, 1, "the step ran");
        let frozen_events: Vec<ClientEvent> = receiver.try_iter().collect();
        assert!(
            frozen_events.iter().any(|event| matches!(
                event,
                ClientEvent::PlayerTick {
                    tick: 1,
                    snapped: false,
                    ..
                }
            )),
            "the frozen step reports its tick: {frozen_events:?}"
        );
        assert!(
            !frozen_events
                .iter()
                .any(|event| matches!(event, ClientEvent::Sky { .. })),
            "a frozen step reports no sky: {frozen_events:?}"
        );

        // The running contrast: the same step with the cycle running reports
        // the sky it moved.
        let mut player = Player::new();
        let mut running = Clock {
            world_age: 48_000,
            time_of_day: 6000,
            frozen: false,
        };
        step_tick(
            &mut player,
            &intent,
            Some(&mut running),
            Some(&world),
            &mut Entities::new(),
            &PlayerList::new(),
            &Scoreboard::new(),
            &sender,
            true,
            &WeatherState::default(),
            false,
        );
        assert_eq!(running.world_age, 48_001);
        assert_eq!(running.time_of_day, 6001, "a running time of day advances");
        let running_events: Vec<ClientEvent> = receiver.try_iter().collect();
        assert_eq!(
            running_events
                .iter()
                .filter(|event| matches!(event, ClientEvent::Sky { .. }))
                .count(),
            1,
            "the running step reports the sky it moved: {running_events:?}"
        );
    }

    #[test]
    fn a_frozen_step_re_reports_the_sky_while_the_weather_moves() {
        // The Sky re-fires when the weather eases even though the clock stands still, so a
        // frozen-clock scene still tracks the rain — and the re-report carries the tick's own
        // strengths: the sun dims by `1 - rain * 5 / 16` and the clouds take the rain blend,
        // while the stars stay pure (the pass owns the dim).
        let intent = Intent::neutral();
        let (sender, receiver) = crossbeam_channel::unbounded::<ClientEvent>();
        let world = World::new(true);
        let mut player = Player::new();
        let mut frozen = Clock {
            world_age: 48_000,
            time_of_day: 6000,
            frozen: true,
        };
        let weather = WeatherState {
            rain: 0.5,
            thunder: 0.0,
            target_rain: 1.0,
            target_thunder: 0.0,
        };
        step_tick(
            &mut player,
            &intent,
            Some(&mut frozen),
            Some(&world),
            &mut Entities::new(),
            &PlayerList::new(),
            &Scoreboard::new(),
            &sender,
            true,
            &weather,
            true,
        );
        assert_eq!(frozen.time_of_day, 6000, "the clock never moved");
        let events: Vec<ClientEvent> = receiver.try_iter().collect();
        let skies: Vec<&ClientEvent> = events
            .iter()
            .filter(|event| matches!(event, ClientEvent::Sky { .. }))
            .collect();
        assert_eq!(
            skies.len(),
            1,
            "the moving weather re-reports the sky on a frozen step: {events:?}"
        );
        let &ClientEvent::Sky {
            sun_brightness,
            star_brightness,
            cloud_colour,
            horizon,
            ..
        } = skies[0]
        else {
            unreachable!("filtered above");
        };
        assert!(
            (sun_brightness - 0.875).abs() < 1e-6,
            "the noon sun dims by half rain's 1 - 0.5 * 5 / 16: {sun_brightness}"
        );
        assert_eq!(
            star_brightness, 0.0,
            "noon stars stay dark and pure: {star_brightness}"
        );
        assert!(
            cloud_colour[0] < 1.0,
            "the clouds take the rain blend: {cloud_colour:?}"
        );
        assert_eq!(
            horizon, None,
            "noon is outside the sunset band: {horizon:?}"
        );
    }

    #[test]
    fn a_hurt_flash_counts_down_and_the_death_clock_advances_per_tick() {
        // `EntityLivingBase.onEntityUpdate:337-340` drops the hurt flash by
        // one every tick and never below zero; while the entity is dead its
        // clock rises by one per tick (`:344-349` reaching `onDeathUpdate`,
        // whose `++this.deathTime` is `:400`).
        let intent = Intent::neutral();
        let (sender, _receiver) = crossbeam_channel::unbounded::<ClientEvent>();
        let mut player = Player::new();
        player.apply_hurt_status();
        assert_eq!(
            player.hurt_time, MAX_HURT_TIME,
            "the status filled the flash"
        );
        player.dead = true;
        for expected in 1u32..=3 {
            step_tick(
                &mut player,
                &intent,
                None,
                None,
                &mut Entities::new(),
                &PlayerList::new(),
                &Scoreboard::new(),
                &sender,
                true,
                &WeatherState::default(),
                false,
            );
            assert_eq!(
                player.death_time, expected,
                "the death clock counts the step's ticks"
            );
        }
        assert_eq!(
            player.hurt_time,
            MAX_HURT_TIME - 3,
            "the flash falls by one each step"
        );
        for _ in 0..MAX_HURT_TIME {
            step_tick(
                &mut player,
                &intent,
                None,
                None,
                &mut Entities::new(),
                &PlayerList::new(),
                &Scoreboard::new(),
                &sender,
                true,
                &WeatherState::default(),
                false,
            );
        }
        assert_eq!(player.hurt_time, 0, "the flash stops at zero");
        assert_eq!(
            player.death_time,
            3 + MAX_HURT_TIME,
            "the clock keeps going"
        );
    }

    #[test]
    fn a_gated_step_holds_its_reports_but_still_steps() {
        // The respawn hold: between the 0x07 and the 0x08 that places the
        // fresh player, the step runs its model and its own event while the
        // action edges and the walking report stay unsent — the source's own
        // gate waits for the world under the player to load
        // (`EntityPlayerSP.onUpdate:170`).
        let intent = Intent::neutral();
        let (sender, receiver) = crossbeam_channel::unbounded::<ClientEvent>();
        let mut player = Player::new();
        player.entity_id = Some(7);
        player.yaw = 30.0;

        let held = step_tick(
            &mut player,
            &intent,
            None,
            None,
            &mut Entities::new(),
            &PlayerList::new(),
            &Scoreboard::new(),
            &sender,
            false,
            &WeatherState::default(),
            false,
        );
        assert!(held.is_empty(), "a held step sends nothing: {held:?}");
        assert_eq!(player.tick, 1, "the step still ran");
        assert!(
            receiver
                .try_iter()
                .any(|event| matches!(event, ClientEvent::PlayerTick { tick: 1, .. })),
            "the step's own event still goes out"
        );

        // The contrast: the same state with the reporters armed sends its one
        // walking report.
        let sent = step_tick(
            &mut player,
            &intent,
            None,
            None,
            &mut Entities::new(),
            &PlayerList::new(),
            &Scoreboard::new(),
            &sender,
            true,
            &WeatherState::default(),
            false,
        );
        assert_eq!(sent.len(), 1, "one walking report per step: {sent:?}");
    }

    #[test]
    fn the_end_of_session_drain_gives_up_at_its_bound() {
        // The drain's expiry path (backlog item 31(8)): an outstanding build
        // cannot hold the session's end forever — [`finish_meshes`] returns
        // at [`END_OF_SESSION_WAIT`] with the build still outstanding and
        // reports nothing of it. The queue's own rules make the case
        // synthesisable: a job recorded as running but never completed stays
        // outstanding, and nothing re-queues it, so the deadline is the
        // drain's only exit.
        let mut queue = MeshQueue::new();
        queue.mark_dirty(4, -2);
        let job = queue.next_job().expect("the column is waiting");
        queue.mark_running(job);
        let pool = mesh_pool().expect("the pool starts");
        let (finished, results) = crossbeam_channel::unbounded();
        let (events, reported) = crossbeam_channel::unbounded::<ClientEvent>();
        let mesh = Arc::new(MeshAssets::fallback());
        let world = World::new(true);

        let start = Instant::now();
        finish_meshes(
            Some(&world),
            &mut queue,
            &pool,
            &mesh,
            &finished,
            &results,
            &events,
            GRAPHICS_FAST,
            SMOOTH_LIGHTING,
        );
        let elapsed = start.elapsed();
        assert!(
            elapsed >= END_OF_SESSION_WAIT,
            "the drain waited its whole bound: {elapsed:?}"
        );
        assert!(
            elapsed < END_OF_SESSION_WAIT + Duration::from_secs(5),
            "and no longer than the bound plus slack: {elapsed:?}"
        );
        assert_eq!(queue.pending(), 1, "the wedged build is still outstanding");
        assert!(
            reported.try_iter().next().is_none(),
            "nothing was reported of a build that never finished"
        );
    }

    #[test]
    fn the_chat_guard_reads_the_fields_hundred_character_cap() {
        // `GuiChat.initGui` sets the field's cap to 100 (`GuiChat.java`:59
        // over `GuiTextField.setMaxStringLength`:639-647); the session's
        // guard is the backstop for a message that slipped past the field,
        // so its boundary is 100 clear and 101 flagged, counted in
        // characters rather than bytes.
        assert_eq!(CHAT_FIELD_CAP, 100);
        assert!(!chat_over_cap(&"x".repeat(100)));
        assert!(chat_over_cap(&"x".repeat(101)));
        assert!(!chat_over_cap(&"é".repeat(100)), "characters, not bytes");
        assert!(chat_over_cap(&"é".repeat(101)));
    }

    #[test]
    fn a_use_click_on_a_villager_sends_the_interact_packet() {
        // The entity-use dispatch pin: a use-click with a villager in reach
        // sends serverbound 0x02 with the interact composition — the id plus
        // the zero ordinal, no hit vector — and consumes the press, so the
        // block path never sees it.
        use std::io::Cursor;

        use oxide_proto::conn::Conn;
        use oxide_world::entity::{Entity, EntityKind};

        use super::step_use_entity;

        let mut entities = Entities::new();
        let mut villager = Entity::new(7, EntityKind::Villager);
        villager.position = [0.5, 64.0, 2.0];
        villager.last_tick_position = [0.5, 64.0, 2.0];
        entities.insert(villager);
        let mut player = Player::new();
        player.position = [0.5, 64.0, 0.5];
        player.yaw = 0.0;
        player.pitch = 0.0;
        let mut presses = 1;
        let cursor = Cursor::new(Vec::new());
        let mut conn = Conn::new(cursor);
        step_use_entity(&entities, &player, 0, None, &mut presses, false, &mut conn)
            .expect("a cursor send cannot fail");
        assert_eq!(presses, 0, "the entity target consumes the press");
        let written = conn.into_inner().into_inner();
        assert!(
            written
                .windows(3)
                .any(|window| window == [0x02, 0x07, 0x00]),
            "the wire carries the 0x02 interact send for entity 7: {written:02x?}"
        );
    }

    #[test]
    fn a_use_press_with_a_written_book_held_and_no_target_sends_the_air_use_packet() {
        // Fix6 (a): the no-target use press with a written book held emits
        // the source's no-target use packet (`PlayerControllerMP.sendUseItem:456-482`
        // queues `new C08PacketPlayerBlockPlacement(held)` whose composition
        // is `field_179726_a` (-1, -1, -1), direction 255, cursor 0,0,0
        // (`C08PacketPlayerBlockPlacement.java:13,24-27`)) instead of being
        // dropped (`Minecraft.rightClickMouse:1628-1636` falls through to
        // `sendUseItem` for any non-null held stack). The reader opens on
        // the server's `MC|BOpen` echo of this send (pinned below); the send
        // count goes 0 to 1 — exactly one 16-byte frame.
        use std::io::Cursor;

        use oxide_proto::conn::Conn;
        use oxide_proto_v47::entity::MetadataItem;

        use super::step_place;
        use crate::mesh_queue::MeshQueue;

        // The frame on the wire: the length prefix (15 payload bytes), the
        // 0x08 id, the packed (-1,-1,-1) position (eight 0xFF), the 255
        // direction byte, the empty held stack (the short -1, two 0xFF —
        // this writer's established convention), the (0,0,0) cursor.
        let air_use_frame = vec![
            0x0F, 0x08, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
            0x00, 0x00,
        ];
        let held = Some(MetadataItem {
            id: 387,
            count: 1,
            damage: 0,
            nbt: None,
        });
        let mut presses = 1;
        let cursor = Cursor::new(Vec::new());
        let mut conn = Conn::new(cursor);
        let mut queue = MeshQueue::new();
        let (events, _reported) = crossbeam_channel::unbounded();
        step_place(
            None,
            &mut queue,
            None,
            &held,
            0,
            &mut presses,
            false,
            &mut conn,
            &events,
        )
        .expect("a cursor send cannot fail");
        assert_eq!(presses, 0, "the press is consumed");
        let written = conn.into_inner().into_inner();
        assert_eq!(
            written.len(),
            air_use_frame.len(),
            "one press sends exactly one frame: {written:02x?}"
        );
        assert_eq!(
            written, air_use_frame,
            "a written-book use press with no target sends the air-use C08: {written:02x?}"
        );
    }

    #[test]
    fn a_use_press_with_a_non_book_held_item_and_no_target_sends_the_same_air_use_packet() {
        // Fix6 (c): the source's fallthrough is NOT book-gated — `sendUseItem`
        // runs for any non-null held stack (`rightClickMouse:1632`), so a
        // non-book held item with no target emits the same air-use packet.
        use std::io::Cursor;

        use oxide_proto::conn::Conn;
        use oxide_proto_v47::entity::MetadataItem;

        use super::step_place;
        use crate::mesh_queue::MeshQueue;

        let air_use_frame = vec![
            0x0F, 0x08, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
            0x00, 0x00,
        ];
        let held = Some(MetadataItem {
            id: 1,
            count: 64,
            damage: 0,
            nbt: None,
        });
        let mut presses = 1;
        let cursor = Cursor::new(Vec::new());
        let mut conn = Conn::new(cursor);
        let mut queue = MeshQueue::new();
        let (events, _reported) = crossbeam_channel::unbounded();
        step_place(
            None,
            &mut queue,
            None,
            &held,
            0,
            &mut presses,
            false,
            &mut conn,
            &events,
        )
        .expect("a cursor send cannot fail");
        assert_eq!(presses, 0, "the press is consumed");
        let written = conn.into_inner().into_inner();
        assert_eq!(
            written, air_use_frame,
            "a stone use press with no target sends the same air-use C08: {written:02x?}"
        );
    }

    #[test]
    fn a_use_press_with_an_empty_hand_sends_nothing() {
        // The fallthrough's own gate (`rightClickMouse:1632`'s null check):
        // an empty hand sends no packet, and the press is still consumed.
        use std::io::Cursor;

        use oxide_proto::conn::Conn;
        use oxide_proto_v47::entity::MetadataItem;

        use super::step_place;
        use crate::mesh_queue::MeshQueue;

        let held: Option<MetadataItem> = None;
        let mut presses = 1;
        let cursor = Cursor::new(Vec::new());
        let mut conn = Conn::new(cursor);
        let mut queue = MeshQueue::new();
        let (events, _reported) = crossbeam_channel::unbounded();
        step_place(
            None,
            &mut queue,
            None,
            &held,
            0,
            &mut presses,
            false,
            &mut conn,
            &events,
        )
        .expect("a cursor send cannot fail");
        assert_eq!(presses, 0, "the press is consumed");
        let written = conn.into_inner().into_inner();
        assert!(
            written.is_empty(),
            "an empty hand sends nothing: {written:02x?}"
        );
    }

    #[test]
    fn a_use_press_in_spectator_sends_nothing() {
        // `sendUseItem` refuses at once in spectator (`:456-460`), so a
        // no-target press in gamemode 3 sends nothing even with a book held.
        use std::io::Cursor;

        use oxide_proto::conn::Conn;
        use oxide_proto_v47::entity::MetadataItem;

        use super::step_place;
        use crate::mesh_queue::MeshQueue;

        let held = Some(MetadataItem {
            id: 387,
            count: 1,
            damage: 0,
            nbt: None,
        });
        let mut presses = 1;
        let cursor = Cursor::new(Vec::new());
        let mut conn = Conn::new(cursor);
        let mut queue = MeshQueue::new();
        let (events, _reported) = crossbeam_channel::unbounded();
        step_place(
            None,
            &mut queue,
            None,
            &held,
            3,
            &mut presses,
            false,
            &mut conn,
            &events,
        )
        .expect("a cursor send cannot fail");
        assert_eq!(presses, 0, "the press is consumed");
        let written = conn.into_inner().into_inner();
        assert!(
            written.is_empty(),
            "a spectator use press sends nothing: {written:02x?}"
        );
    }

    #[test]
    fn the_book_open_stands_on_the_held_written_book_alone() {
        // Fix6 (a/b, reader half): the `MC|BOpen` open carries no stack —
        // the payload is EMPTY (`EntityPlayerMP.displayGUIBook:840-848`
        // sends an empty buffer) — so the client opens its OWN held stack,
        // guarded by the written-book identity
        // (`NetHandlerPlayClient.handleCustomPayload:1855-1863`). The helper
        // takes no payload bytes by construction: there is nothing to read.
        use oxide_proto_v47::entity::MetadataItem;

        use super::{BOOK_OPEN_CHANNEL, WRITTEN_BOOK_ID, book_open_stack};

        assert_eq!(BOOK_OPEN_CHANNEL, "MC|BOpen");
        assert_eq!(WRITTEN_BOOK_ID, 387);
        let written = Some(MetadataItem {
            id: 387,
            count: 1,
            damage: 0,
            nbt: None,
        });
        let stone = Some(MetadataItem {
            id: 1,
            count: 64,
            damage: 0,
            nbt: None,
        });
        assert_eq!(
            book_open_stack("MC|BOpen", &written),
            written,
            "BOpen with a held written book opens that stack"
        );
        assert_eq!(
            book_open_stack("MC|BOpen", &stone),
            None,
            "BOpen with a non-written held item opens nothing"
        );
        assert_eq!(
            book_open_stack("MC|BOpen", &None),
            None,
            "BOpen with an empty hand opens nothing"
        );
        assert_eq!(
            book_open_stack("MC|Brand", &written),
            None,
            "another channel never opens the reader"
        );
    }

    #[test]
    fn weather_eases_clear_to_full_at_the_pinned_rate() {
        // The source's own per-tick step (`World.updateWeather`,
        // `World.java:2576-2587`): 0.01 a tick, so clear-to-full is exactly
        // 100 ticks and the halfway point reads 0.5.
        assert_eq!(
            WEATHER_EASE_RATE, 0.01,
            "the pinned per-tick rate: {WEATHER_EASE_RATE}"
        );
        let mut weather = WeatherState {
            target_rain: 1.0,
            ..WeatherState::default()
        };
        for _ in 0..50 {
            assert!(weather.tick(), "every easing tick moves");
        }
        assert!(
            (weather.rain - 0.5).abs() < 1e-6,
            "fifty ticks ease halfway: {}",
            weather.rain
        );
        for _ in 0..50 {
            assert!(weather.tick(), "the second half moves too");
        }
        assert_eq!(weather.rain, 1.0, "a hundred ticks reach full");
        assert_eq!(weather.thunder, 0.0, "the thunder side never moved");
        assert!(!weather.tick(), "a settled tick reports no change");
    }

    #[test]
    fn weather_thunder_fade_folds_down_and_snaps_on_arrival() {
        // A fade pair below the target eases down at the same rate, and the
        // arrival tick snaps exactly onto a target the rate would overshoot.
        let mut weather = WeatherState {
            thunder: 1.0,
            target_thunder: 0.97,
            ..WeatherState::default()
        };
        assert!(weather.tick(), "the first tick moves");
        assert!(
            (weather.thunder - 0.99).abs() < 1e-6,
            "one tick down at the rate: {}",
            weather.thunder
        );
        for _ in 0..2 {
            assert!(weather.tick(), "the easing keeps moving");
        }
        assert_eq!(
            weather.thunder, 0.97,
            "the arrival snaps onto the target instead of overshooting"
        );
        assert!(!weather.tick(), "a settled tick reports no change");
        assert_eq!(weather.rain, 0.0, "the rain side never moved");
    }

    #[test]
    fn weather_end_rain_falls_while_a_thunder_fade_rises() {
        // The two strengths ease independently: one falls from full while the
        // other climbs toward its fade value.
        let mut weather = WeatherState {
            rain: 1.0,
            target_rain: 0.0,
            target_thunder: 0.05,
            ..WeatherState::default()
        };
        for _ in 0..5 {
            assert!(weather.tick(), "both sides move together");
        }
        assert!(
            (weather.rain - 0.95).abs() < 1e-6,
            "five ticks down at the rate: {}",
            weather.rain
        );
        assert_eq!(
            weather.thunder, 0.05,
            "the thunder fade arrives exactly on its fifth tick"
        );
        assert!(weather.tick(), "the rain still falls");
        assert!(
            (weather.rain - 0.94).abs() < 1e-6,
            "the rain keeps easing alone: {}",
            weather.rain
        );
    }

    /// The framing a scripted server uses once Set Compression has gone out.
    const SCRIPT_FRAMING: oxide_proto::frame::Compression =
        oxide_proto::frame::Compression::Enabled { threshold: 256 };

    /// A scripted in-memory stream for the drain-ordering and report-order
    /// pins below: scripted bytes read back, client writes accumulate for
    /// inspection, and each idle wait counts up so inputs inject mid-run at
    /// a chosen wait — after the script's death or mode frames, never
    /// before them.
    struct ScriptStream {
        /// Frames served first.
        head: Cursor<Vec<u8>>,
        /// Frames served after `stalls` idle waits.
        tail: Cursor<Vec<u8>>,
        /// Idle waits before the tail.
        stalls: usize,
        /// Idle waits after the tail before the stream ends.
        tail_stalls: usize,
        /// Idle waits served so far.
        waits: usize,
        /// Inputs to queue at `(wait, event)`.
        flips: Vec<(usize, InputEvent)>,
        /// The session's input channel.
        input_tx: crossbeam_channel::Sender<InputEvent>,
        /// Every byte the client wrote.
        outgoing: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for ScriptStream {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.head.position() < self.head.get_ref().len() as u64 {
                self.head.read(out)
            } else {
                self.tail.read(out)
            }
        }
    }

    impl Write for ScriptStream {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.outgoing.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl oxide_proto::conn::DeadlineStream for ScriptStream {
        fn wait_readable(&mut self, timeout: Duration) -> std::io::Result<bool> {
            if self.head.position() < self.head.get_ref().len() as u64 {
                return Ok(true);
            }
            if self.stalls > 0 {
                self.stalls -= 1;
                self.fire_flips();
                // A real deadline blocks for the timeout before it reports
                // nothing; the script waits the same span, so ticks the wall
                // clock owes still run.
                std::thread::sleep(timeout);
                return Ok(false);
            }
            if self.tail.position() < self.tail.get_ref().len() as u64 {
                return Ok(true);
            }
            if self.tail_stalls > 0 {
                self.tail_stalls -= 1;
                self.fire_flips();
                std::thread::sleep(timeout);
                return Ok(false);
            }
            Ok(true)
        }
    }

    impl ScriptStream {
        /// Queues every flip whose wait has come due.
        fn fire_flips(&mut self) {
            self.waits += 1;
            for (after, event) in &self.flips {
                if *after == self.waits {
                    let _ = self.input_tx.send(event.clone());
                }
            }
        }
    }

    /// The session config the scripts run against.
    fn script_config() -> SessionConfig {
        SessionConfig {
            host: "127.0.0.1".into(),
            port: 25565,
            username: "OxideDev".into(),
            settings: ClientSettings::default(),
            mesh: None,
            account: None,
        }
    }

    /// Appends one scripted server frame, framed as the server frames it.
    fn script_frame(
        out: &mut Vec<u8>,
        payload: &[u8],
        compression: oxide_proto::frame::Compression,
    ) {
        oxide_proto::frame::write_frame(out, payload, compression).expect("a scripted frame");
    }

    /// The login every script starts with: Set Compression in the plain
    /// framing, then Login Success in the compressed framing it switched on.
    fn script_login(out: &mut Vec<u8>) {
        script_frame(
            out,
            &[0x03, 0x80, 0x02],
            oxide_proto::frame::Compression::Disabled,
        );
        let mut success = vec![0x02, 0x24];
        success.extend_from_slice(b"069a79f4-44e9-4726-a5be-fca90e38aaf5");
        success.push(0x08);
        success.extend_from_slice(b"OxideDev");
        script_frame(out, &success, SCRIPT_FRAMING);
    }

    /// One Join Game payload (0x01): entity 20, `gamemode`, the overworld,
    /// difficulty 1, 20 players, the `default` level type.
    fn script_join(gamemode: u8) -> Vec<u8> {
        let mut join = vec![0x01];
        join.extend_from_slice(&20i32.to_be_bytes());
        join.extend_from_slice(&[gamemode, 0, 1, 20, 7]);
        join.extend_from_slice(b"default");
        join.push(0);
        join
    }

    /// One ground-up Chunk Data column (0x21) of solid stone at `(cx, cz)`.
    fn script_column(cx: i32, cz: i32) -> Vec<u8> {
        let mut section = vec![0u8; 8192];
        for cell in section.chunks_exact_mut(2) {
            cell.copy_from_slice(&0x0010u16.to_le_bytes());
        }
        let mut data = section;
        data.extend_from_slice(&[0u8; 2048]); // block light
        data.extend_from_slice(&[0xFFu8; 2048]); // sky light
        data.extend_from_slice(&[1u8; 256]); // biomes
        let mut chunk = vec![0x21];
        chunk.extend_from_slice(&cx.to_be_bytes());
        chunk.extend_from_slice(&cz.to_be_bytes());
        chunk.push(1); // ground-up
        chunk.extend_from_slice(&0x0008u16.to_be_bytes());
        oxide_proto::varint::write_varint(&mut chunk, data.len() as i32)
            .expect("writing to a Vec cannot fail");
        chunk.extend_from_slice(&data);
        chunk
    }

    /// One Player Position And Look payload (0x08).
    fn script_position(x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> Vec<u8> {
        let mut position = vec![0x08];
        position.extend_from_slice(&x.to_be_bytes());
        position.extend_from_slice(&y.to_be_bytes());
        position.extend_from_slice(&z.to_be_bytes());
        position.extend_from_slice(&yaw.to_be_bytes());
        position.extend_from_slice(&pitch.to_be_bytes());
        position.push(0);
        position
    }

    /// One Block Change payload (0x23): the packed position and the block
    /// value.
    fn script_block_change(x: i32, y: i32, z: i32, value: u16) -> Vec<u8> {
        let mut payload = vec![0x23];
        let packed =
            ((x as i64 & 0x3FFFFFF) << 38) | ((y as i64 & 0xFFF) << 26) | (z as i64 & 0x3FFFFFF);
        payload.extend_from_slice(&packed.to_be_bytes());
        oxide_proto::varint::write_varint(&mut payload, value as i32)
            .expect("writing to a Vec cannot fail");
        payload
    }

    /// One Change Game State payload (0x2B): the reason byte and the value
    /// float.
    fn script_game_state(reason: u8, value: f32) -> Vec<u8> {
        let mut payload = vec![0x2B, reason];
        payload.extend_from_slice(&value.to_be_bytes());
        payload
    }

    /// One Update Health payload (0x06): the health float, the food VarInt
    /// and the saturation float.
    fn script_health(health: f32) -> Vec<u8> {
        let mut payload = vec![0x06];
        payload.extend_from_slice(&health.to_be_bytes());
        oxide_proto::varint::write_varint(&mut payload, 20).expect("writing to a Vec cannot fail");
        payload.extend_from_slice(&5.0f32.to_be_bytes());
        payload
    }

    /// Runs one scripted session with `flips` injected at idle-wait counts,
    /// and returns every event it reported and every frame it wrote.
    fn drive_script(
        head: Vec<u8>,
        tail: Vec<u8>,
        stalls: usize,
        tail_stalls: usize,
        flips: Vec<(usize, InputEvent)>,
    ) -> (Vec<ClientEvent>, Vec<Vec<u8>>) {
        let outgoing = Arc::new(Mutex::new(Vec::new()));
        let (input_tx, input_rx) = crossbeam_channel::unbounded();
        let stream = ScriptStream {
            head: Cursor::new(head),
            tail: Cursor::new(tail),
            stalls,
            tail_stalls,
            waits: 0,
            flips,
            input_tx,
            outgoing: Arc::clone(&outgoing),
        };
        let (sender, receiver) = crossbeam_channel::unbounded();
        Session::new(oxide_proto::conn::Conn::new(stream), script_config())
            .run_over(&sender, input_rx)
            .expect("the scripted session runs to the end of the stream");
        let events: Vec<ClientEvent> = receiver.try_iter().collect();
        let written = outgoing.lock().unwrap().clone();
        let mut cursor = &written[..];
        let mut frames = Vec::new();
        frames.push(
            oxide_proto::frame::read_frame(&mut cursor, oxide_proto::frame::Compression::Disabled)
                .expect("the handshake"),
        );
        frames.push(
            oxide_proto::frame::read_frame(&mut cursor, oxide_proto::frame::Compression::Disabled)
                .expect("Login Start"),
        );
        while !cursor.is_empty() {
            frames.push(
                oxide_proto::frame::read_frame(&mut cursor, SCRIPT_FRAMING)
                    .expect("a client frame"),
            );
        }
        (events, frames)
    }

    /// A settings push that moves every session-held value off its default.
    fn moved_push() -> SessionPush {
        SessionPush {
            settings: ClientSettings {
                locale: "de_DE".to_string(),
                view_distance: 10,
                chat_mode: 1,
                chat_colors: false,
                skin_parts: 0x01,
            },
            graphics_fast: false,
            smooth_lighting: 0,
            sensitivity: 1.0,
            invert_mouse: true,
            particles: ParticleSetting::Minimal,
        }
    }

    #[test]
    fn a_settings_push_applies_while_dead() {
        // M6 T14 M-c: the drain's `PushSettings` arm precedes the dead check,
        // so a pause-menu change lands while dead instead of being dropped.
        // The script kills the player before any idle wait, and the push
        // injects at wait 2 — dead already — so a gated arm would send no
        // C15 echo. (The sibling `SetParticleSetting` arm shares the same
        // pre-check block; its regime mapping is pinned at function level.)
        let mut head = Vec::new();
        script_login(&mut head);
        script_frame(&mut head, &script_health(0.0), SCRIPT_FRAMING);
        let push = moved_push();
        let (events, frames) = drive_script(
            head,
            Vec::new(),
            8,
            0,
            vec![(2, InputEvent::PushSettings(push))],
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ClientEvent::Died)),
            "the player died before the push: {events:?}"
        );
        let mut expected = Vec::new();
        script_frame(
            &mut expected,
            &oxide_proto_v47::serverbound::client_settings_payload(&moved_push().settings),
            SCRIPT_FRAMING,
        );
        let mut cursor = &expected[..];
        let echo = oxide_proto::frame::read_frame(&mut cursor, SCRIPT_FRAMING).expect("the echo");
        assert_eq!(
            frames.len(),
            3,
            "only the handshake, Login Start and the echo left: {frames:?}"
        );
        assert_eq!(
            frames[2], echo,
            "the push's values reached the wire while dead"
        );
    }

    /// The `GameType` subsequence a run reported, in order.
    fn reported_game_types(events: &[ClientEvent]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|event| match event {
                ClientEvent::GameType { gamemode } => Some(*gamemode),
                _ => None,
            })
            .collect()
    }

    /// The `CameraClip` distances a run reported, in order.
    fn reported_clips(events: &[ClientEvent]) -> Vec<f32> {
        events
            .iter()
            .filter_map(|event| match event {
                ClientEvent::CameraClip { distance } => Some(*distance),
                _ => None,
            })
            .collect()
    }

    /// The F3 scene: a join, a stone-floor column, a teleport facing north
    /// (so the third-person camera stands south, inside the loaded column),
    /// and — when asked — a stone wall at (0, 65, 3) behind the player that
    /// the camera's corner rays must cross.
    fn clip_head(wall: bool) -> Vec<u8> {
        let mut head = Vec::new();
        script_login(&mut head);
        script_frame(&mut head, &script_join(0), SCRIPT_FRAMING);
        script_frame(&mut head, &script_column(0, 0), SCRIPT_FRAMING);
        script_frame(
            &mut head,
            &script_position(0.5, 64.0, 0.389, 180.0, 0.0),
            SCRIPT_FRAMING,
        );
        if wall {
            script_frame(
                &mut head,
                &script_block_change(0, 65, 3, 0x0010),
                SCRIPT_FRAMING,
            );
        }
        head
    }

    /// The F3 tail: a switch to spectator, then to an unknown id that reads
    /// survival.
    fn clip_tail() -> Vec<u8> {
        let mut tail = Vec::new();
        script_frame(&mut tail, &script_game_state(3, 3.0), SCRIPT_FRAMING);
        script_frame(&mut tail, &script_game_state(3, 9.0), SCRIPT_FRAMING);
        tail
    }

    #[test]
    fn the_game_mode_switch_reports_in_order_and_clips_behind_walls() {
        // M6 T27 F3: reason-3 switches report `GameType` with the floored
        // byte — an unknown id answering survival — in script order, and a
        // third-person tick behind a wall reports the shortened `CameraClip`.
        // The perspective flip injects after the tail's switches, so the
        // clip must sort after both mode reports.
        let (events, _) = drive_script(
            clip_head(true),
            clip_tail(),
            6,
            48,
            vec![(8, InputEvent::Perspective { mode: 1 })],
        );
        assert_eq!(
            reported_game_types(&events),
            vec![3, 0],
            "spectator, then the unknown id back at survival: {events:?}"
        );
        let clips = reported_clips(&events);
        assert!(
            !clips.is_empty(),
            "the wall shortened the third-person distance: {events:?}"
        );
        assert!(
            clips.iter().all(|&distance| distance > 0.0
                && distance < oxide_render::camera::THIRD_PERSON_DISTANCE),
            "every clip sits inside the wall: {clips:?}"
        );
        let first_mode = events
            .iter()
            .position(|event| matches!(event, ClientEvent::GameType { .. }))
            .expect("a mode report");
        let first_clip = events
            .iter()
            .position(|event| matches!(event, ClientEvent::CameraClip { .. }))
            .expect("a clip report");
        assert!(
            first_mode < first_clip,
            "the switch reports before the later tick's clip"
        );
    }

    #[test]
    fn first_person_ticks_report_no_camera_clip() {
        // The same switches with no perspective flip: the modes still report
        // in order, and first person resets the read with no event.
        let (events, _) = drive_script(clip_head(true), clip_tail(), 6, 48, Vec::new());
        assert_eq!(
            reported_game_types(&events),
            vec![3, 0],
            "the modes report without any camera move: {events:?}"
        );
        assert!(
            reported_clips(&events).is_empty(),
            "first person reports no clip: {events:?}"
        );
    }
}
