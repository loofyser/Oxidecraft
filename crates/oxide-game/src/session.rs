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
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use oxide_assets::atlas::Atlas;
use oxide_proto::codec::CodecError;
use oxide_proto::conn::{Conn, DeadlineStream, RecvOutcome};
use oxide_proto::frame::{Compression, FrameError};
use oxide_proto::varint::VarIntError;
use oxide_proto_v47::PacketError;
use oxide_proto_v47::clientbound::{
    self, BlockBreakAnimation, BlockChange, BlockUpdate, ChangeGameState, ChunkData, EntityStatus,
    JoinGame, KeepAlive, LoginPacket, MapChunkBulk, MultiBlockChange, PlayDisconnect,
    PlayerAbilities, PlayerListItem, PlayerPositionAndLook, PluginMessage, Respawn, TimeUpdate,
    UpdateHealth, read_packet_id,
};
use oxide_proto_v47::entity::{
    self, Animation, AttachEntity, CollectItem, DestroyEntities, EntityEquipment, EntityHeadLook,
    EntityLook, EntityLookAndRelativeMove, EntityMetadata, EntityRelativeMove, EntityTeleport,
    EntityVelocity, MobType, ObjectType, SpawnGlobal, SpawnMob, SpawnObject, SpawnPainting,
    SpawnPlayer, SpawnXpOrb,
};
use oxide_proto_v47::handshake::write_handshake;
use oxide_proto_v47::serverbound::{
    ClientSettings, ClientStatusAction, DiggingStatus, EntityAction, write_animation,
    write_client_settings, write_client_status, write_entity_action, write_keep_alive,
    write_login_start, write_player, write_player_abilities, write_player_block_placement,
    write_player_digging, write_player_look, write_player_position, write_player_position_and_look,
    write_plugin_message,
};
use oxide_proto_v47::ui::{
    ChatMessage, ScoreboardDisplay, ScoreboardObjective, ScoreboardScore, ScoreboardTeam,
    TabHeaderFooter, write_chat,
};
use oxide_proto_v47::{NEXT_STATE_LOGIN, PROTOCOL};
use oxide_render::terrain::ChunkMesh;
use oxide_world::behaviour::behaviour;
use oxide_world::biome::{ColorMap, TintMaps};
use oxide_world::chunk::SECTION_SIZE;
use oxide_world::entity::{Entities, Entity, EntityKind, KindData};
use oxide_world::light::{self, view_light_level};
use oxide_world::sky::{
    celestial_angle, cloud_colour, moon_phase, sky_colour, star_brightness, sun_brightness,
};
use oxide_world::world::World;
use rayon::{ThreadPool, ThreadPoolBuildError, ThreadPoolBuilder};
use tracing::{debug, info, warn};

use crate::entity_view::{self, EntityFrame, PlayerList, PlayerListRecord, hyphenated};
use crate::input::{InputEvent, Intent, Key, MouseButton, look_delta};
use crate::interaction::{
    Aim, BreakStages, DigAction, DigAim, DigState, creative, hand_rate, look_vector,
    mode_from_value, placement, raycast, reach, tool_not_required,
};
use crate::mesh_queue::{MeshJob, MeshQueue};
use crate::mesher::{
    BlockModelSet, ColumnSnapshot, MeshContext, SmoothLighting, build_column_meshes,
};
use crate::physics;
use crate::player::{Abilities, Player};
use crate::scoreboard::{Scoreboard, colour_from_wire};
use crate::ticker::Ticker;
use crate::world_view::WorldView;

/// The plugin channel a vanilla client announces itself on.
const BRAND_CHANNEL: &str = "MC|Brand";

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

/// How many of the window's input events one pass drains.
///
/// The bound has no source counterpart: the source's input arrives from the
/// window it already owns. The backlog drains across passes and nothing is
/// dropped; the bound only keeps one pass' work finite when the window
/// outruns the session.
const INPUTS_PER_PASS: usize = 32;

/// The mouse sensitivity the look mapping runs with.
///
/// `GameSettings.mouseSensitivity`'s own default, 0.5
/// (`GameSettings.java:65`); the options screen that would change it arrives
/// with M6.
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

/// What the session needs to connect and behave like a vanilla client.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Server host.
    pub host: String,
    /// Server port.
    pub port: u16,
    /// The offline-mode player name.
    pub username: String,
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

/// The context a column build reads: the session's assets and the settings
/// above.
fn mesh_context(assets: &MeshAssets) -> MeshContext<'_> {
    MeshContext {
        models: &assets.models,
        atlas: &assets.atlas,
        tint_maps: &assets.tint_maps,
        graphics_fast: GRAPHICS_FAST,
        smooth_lighting: SMOOTH_LIGHTING,
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
    },
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
    /// (`S03PacketTimeUpdate.java:17-31`, `WorldClient.java:468-483`): a stopped cycle's frame
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
    /// travel with the event. `partial_ticks` is zero (M2 has no tick loop) and the rain
    /// strength is zero (no weather packets are decoded yet).
    Sky {
        /// The celestial angle in `0..1`, from the world time.
        celestial_angle: f32,
        /// The sky's colour at the view block's biome.
        colour: [f32; 3],
        /// The sun's brightness.
        sun_brightness: f32,
        /// The stars' brightness, the rain already folded in.
        star_brightness: f32,
        /// The clouds' tint.
        cloud_colour: [f32; 3],
        /// The moon's phase in `0..8`, from the world time.
        moon_phase: u8,
        /// The light level at the view block, 0..15: the fog colour's brightness factor reads
        /// it (`EntityRenderer.java:362`), and the client's renderer builds the table
        /// (`oxide-world`'s `light::view_light_level`).
        light_level: u8,
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
    /// The server asked for encryption; M1 does not implement it.
    #[error("the server requires an encrypted session, which M1 does not implement")]
    EncryptionRequired,
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
        let Session {
            mut conn,
            config,
            mesh,
        } = self;

        // The pool and the results channel live for the whole play loop; the
        // handshake and the login above mesh nothing.
        let pool = mesh_pool()?;
        let (finished, results) = crossbeam_channel::unbounded::<MeshResult>();
        let mut queue = MeshQueue::new();

        // Nothing has touched the framing yet: the handshake and Login Start go
        // out plain, and the server answers by naming a compression threshold.
        let handshake = payload_of(|out| {
            write_handshake(out, PROTOCOL, &config.host, config.port, NEXT_STATE_LOGIN)
        })?;
        send_reply(&mut conn, &handshake)?;
        let login_start = payload_of(|out| write_login_start(out, &config.username))?;
        send_reply(&mut conn, &login_start)?;

        match login(&mut conn, events)? {
            LoginOutcome::Accepted { uuid, username } => {
                report(events, ClientEvent::LoggedIn { uuid, username });
            }
            LoginOutcome::Ended => return Ok(()),
        }

        let mut world: Option<World> = None;
        let mut player = Player::new();
        let mut clock: Option<Clock> = None;
        let mut ticker = Ticker::new(TICK_PERIOD);
        let mut intent = Intent::neutral();
        // The gamemode the reach reads, from the last Join Game
        // (`PlayerControllerMP.getBlockReachDistance`, `:344-346`); the
        // controller's own default until one arrives is survival
        // (`PlayerControllerMP.java:59`).
        let mut gamemode: u8 = 0;
        // The aim the last recompute left; only a change is reported.
        let mut aim: Option<Aim> = None;
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
                                other => looked |= apply_input(other, &mut intent, &mut player),
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
                            let settings =
                                payload_of(|out| write_client_settings(out, &config.settings))?;
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
                                },
                            );
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
                                } else {
                                    debug!(
                                        status = status.status,
                                        "a status this client has no use for"
                                    );
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
                                let list_before = std::mem::take(&mut player_list);
                                // The rebuilt world reports the emptied list
                                // at once.
                                report_list_change(&player_list, &list_before, events);
                                report(events, ClientEvent::WorldCleared);
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
                                )?;
                            }
                            if aim.take().is_some() {
                                report(events, ClientEvent::Aim { aim: None });
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
                            // The view block moved: the sky's colour is sampled at the player's own
                            // block, so a correction can change it without a new clock.
                            report_sky(world.as_ref(), player.position, clock.as_ref(), events);
                        }
                        ChangeGameState::ID => {
                            let change = decoded(id, ChangeGameState::decode(body))?;
                            if change.reason == ChangeGameState::REASON_CHANGE_GAME_MODE {
                                // The mode the reach and the dig machine's creative
                                // branch read. Both read it live — the aim recomputes
                                // against this byte each tick, and the dig step takes
                                // it per step — so the next recompute already uses the
                                // new reach (`PlayerControllerMP.java:344-346`). The
                                // value maps as the source's handler maps it: floored
                                // and matched against the enum, everything unknown
                                // answering survival (`NetHandlerPlayClient.java:1364`,
                                // `:1383`).
                                gamemode = mode_from_value(change.value);
                                debug!(
                                    gamemode,
                                    value = change.value,
                                    "the server changed the game mode"
                                );
                            }
                        }
                        TimeUpdate::ID => {
                            let update = decoded(id, TimeUpdate::decode(body))?;
                            // The frozen-sun convention: a stopped day-night cycle puts a negative
                            // time on the wire (`S03PacketTimeUpdate.java:17-31`), and the client
                            // negates it back before it becomes the world clock
                            // (`WorldClient.setWorldTime`, `WorldClient.java:468-482`), so the
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
                            report_sky(world.as_ref(), player.position, clock.as_ref(), events);
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
                                } else {
                                    queue.mark_column_unloaded(column.chunk_x, column.chunk_z);
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
                                    } else {
                                        queue.mark_column_unloaded(column.chunk_x, column.chunk_z);
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
                        Animation::ID => {
                            let animation = decoded(id, entity::decode_animation(body))?;
                            // Only the swing (byte 0) moves an arm the source's
                            // handler answers (`NetHandlerPlayClient.handleAnimation`);
                            // the other animations are dropped there and here.
                            if animation.animation == 0 {
                                // The id names a tracked entity or nothing:
                                // the swing lands through the store's own
                                // lookup.
                                if let Some(entity) = entities.get_mut(animation.entity_id) {
                                    entity.swing();
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
                        SpawnGlobal::ID => {
                            let spawn = decoded(id, entity::decode_spawn_global(body))?;
                            let mut entity = Entity::new(spawn.entity_id, EntityKind::Global);
                            entity.position = [spawn.x, spawn.y, spawn.z];
                            entity.last_tick_position = entity.position;
                            entities.insert(entity);
                        }
                        CollectItem::ID => {
                            let collect = decoded(id, entity::decode_collect_item(body))?;
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
                        PluginMessage::ID => {
                            let message = decoded(id, PluginMessage::decode(body))?;
                            debug!(channel = %message.channel, bytes = message.data.len(), "plugin message");
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
                ) {
                    send_reply(&mut conn, &payload)?;
                }
                // The aim follows the tick's own state: a step that moved the
                // player, and any world change behind it, is read here.
                update_aim(world.as_ref(), &player, gamemode, &mut aim, events);
                // Nothing the window presses reaches the interaction while the
                // death view is up: a click is the respawn request then, and
                // the presses and the dig step run only while alive.
                if !dead {
                    // The placement presses run against that same aim, before the
                    // dig's step: the source's tick orders the use button's presses
                    // before `sendClickBlockToController` (`Minecraft.java:2153-2166`).
                    step_place(
                        world.as_mut(),
                        &mut queue,
                        aim,
                        &mut right_presses,
                        dig.hitting(),
                        &mut conn,
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
enum LoginOutcome {
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
fn login<S: Read + Write>(
    conn: &mut Conn<S>,
    events: &Sender<ClientEvent>,
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
            LoginPacket::EncryptionRequest { .. } => {
                return Err(SessionError::EncryptionRequired);
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
/// stopping the cycle (`:468-482`). [`received_time_of_day`] does the negation
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
    });
    if next != *aim {
        *aim = next;
        report(events, ClientEvent::Aim { aim: next });
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
/// The chat send has no arm here to reach: every [`InputEvent::SendChat`] is
/// collected by the play loop's drain — and written there — before this sees
/// it, and its arm below is match completeness only.
fn apply_input(event: InputEvent, intent: &mut Intent, player: &mut Player) -> bool {
    match event {
        InputEvent::Key { key, pressed } => {
            intent.apply_key(key, pressed);
            false
        }
        InputEvent::MouseDelta { dx, dy } => {
            let (d_yaw, d_pitch) = look_delta(dx, dy, MOUSE_SENSITIVITY);
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
        // carries no held key and no look, so nothing here applies.
        InputEvent::SendChat { .. } => false,
    }
}

/// Applies one mouse button edge to the interaction input.
///
/// The left button is the source's attack key: a press is one `clickMouse`
/// call (`Minecraft.java:1454-1458`) and the held flag is
/// `sendClickBlockToController`'s `leftClick` (`:1496-1505`). The right
/// button is the use key: a press is one `rightClickMouse` call
/// (`:2153-2156`), consumed by the placement step.
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
/// (`onLivingUpdate`, `:801-820`); the clock advances as [`Clock`] describes.
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
    // The sneak flag is the held key, and the sprint rule runs once per tick.
    player.sneaking = input.sneak;
    let sprinting = player.sprinting;
    player.sprinting = player.sprint_tap.update(input, sprinting, player.on_ground);

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
        physics::step(player, input, &WorldView(world));
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
    if advanced {
        report_sky(world, player.position, clock.as_deref(), events);
    }
    report(events, player_tick(player, false));
    // The feed sits immediately behind the player's own event: one
    // `EntitiesTick` per tick, carrying every tracked entity's frame.
    report(events, entities_tick(entities, world, player_list, board));
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
/// (`WorldClient.java:468-483`) negates a negative time — the frozen-sun convention on the wire
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
/// so the partial tick is zero, and no weather is decoded, so the rain strength is zero; both
/// are recorded in the module docs and the report. A session with no clock yet — or no world,
/// as before Join Game — has no sky to report.
fn report_sky(
    world: Option<&World>,
    view: [f64; 3],
    clock: Option<&Clock>,
    events: &Sender<ClientEvent>,
) {
    let (Some(world), Some(clock)) = (world, clock) else {
        return;
    };
    let time_of_day = clock.time_of_day;
    /// The session reports whole ticks; interpolation is the renderer's.
    const PARTIAL_TICKS: f32 = 0.0;
    /// No weather packets are decoded yet, so the rain strength is zero.
    const RAIN_STRENGTH: f32 = 0.0;
    let angle = celestial_angle(time_of_day, PARTIAL_TICKS);
    let x = view[0].floor() as i32;
    let y = view[1].floor() as i32;
    let z = view[2].floor() as i32;
    let colour = sky_colour(world, x, y, z, angle);
    // The fog's brightness factor reads the view block's own light, with the source's
    // defaults for a position no column holds (`EntityRenderer.java:362-365`).
    let light_level = view_light_level(world, x, y, z);
    report(
        events,
        ClientEvent::Sky {
            celestial_angle: angle,
            colour,
            sun_brightness: sun_brightness(time_of_day, PARTIAL_TICKS, RAIN_STRENGTH),
            star_brightness: star_brightness(time_of_day, PARTIAL_TICKS, RAIN_STRENGTH),
            cloud_colour: cloud_colour(time_of_day, PARTIAL_TICKS, RAIN_STRENGTH),
            // `moon_phase` answers `0..8`, so the narrowing cannot lose a case.
            moon_phase: moon_phase(time_of_day) as u8,
            light_level,
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
        DigAction::Destroy { x, y, z } => {
            if let Some(store) = world {
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

/// One tick of the placement input: each queued press, when an aim exists,
/// runs the source's checks, sends 0x08 and predicts the block locally.
///
/// The source's path is `rightClickMouse` (`Minecraft.java:1570-1603`): it
/// refuses while a dig runs (`:1572`'s `getIsHittingBlock`), drops a press
/// with nothing aimed (`:1577-1581`), and hands the frame's hit result to
/// `PlayerControllerMP.onPlayerRightClick` (`:395-396`'s `hitPos` and
/// `side`). That method refuses a target that cannot take the block before
/// the packet (`:417-421`), queues 0x08 (`:424`) and then lets
/// `onItemUse` write the block locally (`:436`, `:443`) — so the packet goes
/// out before the prediction lands, the order the two calls run here.
///
/// The prediction's value is [`ASSUMED_HELD_BLOCK`]: the packet's held stack
/// is empty, and the server places from its own copy of the held item
/// (`NetHandlerPlayServer.processPlayerBlockPlacement`, `:582`).
fn step_place<S: Read + Write>(
    mut world: Option<&mut World>,
    queue: &mut MeshQueue,
    aim: Option<Aim>,
    presses: &mut u32,
    hitting: bool,
    conn: &mut Conn<S>,
) -> Result<(), SessionError> {
    let presses = std::mem::take(presses);
    if hitting {
        return Ok(());
    }
    for _ in 0..presses {
        let (Some(store), Some(aim)) = (world.as_deref_mut(), aim) else {
            break;
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
/// Called when the read found nothing for a whole [`MESH_TICK`], so the work
/// here never delays a frame that was already readable. One pass spawns at
/// most [`PENDING_JOBS_CAP`] builds: the snapshot copy happens here, on the
/// session's thread — the world never leaves it — and the cap is what bounds
/// this thread's own work for one idle wait. The build itself reads only the
/// snapshot and the shared assets. The results are then taken with a
/// non-blocking receive: a build that is not finished yet is picked up by a
/// later pass, never waited for.
fn pump_meshes(
    world: Option<&World>,
    queue: &mut MeshQueue,
    pool: &ThreadPool,
    mesh: &Arc<MeshAssets>,
    finished: &Sender<MeshResult>,
    results: &Receiver<MeshResult>,
    events: &Sender<ClientEvent>,
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
                let ctx = mesh_context(&assets);
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
fn finish_meshes(
    world: Option<&World>,
    queue: &mut MeshQueue,
    pool: &ThreadPool,
    mesh: &Arc<MeshAssets>,
    finished: &Sender<MeshResult>,
    results: &Receiver<MeshResult>,
    events: &Sender<ClientEvent>,
) {
    if world.is_none() {
        // No world was ever joined, so nothing can be queued for it.
        return;
    }
    let deadline = Instant::now() + END_OF_SESSION_WAIT;
    loop {
        pump_meshes(world, queue, pool, mesh, finished, results, events);
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
        ASSUMED_HELD_BLOCK, CHAT_FIELD_CAP, ClientEvent, Clock, END_OF_SESSION_WAIT, MeshAssets,
        MeshQueue, TICK_PERIOD, chat_over_cap, finish_meshes, mesh_pool, step_tick,
    };
    use crate::entity_view::PlayerList;
    use crate::input::Intent;
    use crate::player::{MAX_HURT_TIME, Player};
    use crate::scoreboard::Scoreboard;
    use crate::ticker::Ticker;
    use std::sync::Arc;

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

    #[test]
    fn the_assumed_held_block_is_stone() {
        // The prediction's own value until M5's inventory: the packet's held
        // stack is empty and the server places from the stack it holds for
        // the account (`NetHandlerPlayServer.processPlayerBlockPlacement`,
        // `:582`), which the acceptance rig arranges as plain stone — id 1,
        // metadata 0 (`Blocks.stone`'s one variant).
        assert_eq!(ASSUMED_HELD_BLOCK, 1 << 4, "stone: id 1 at metadata 0");
    }

    #[test]
    fn a_frozen_tick_advances_the_world_age_and_holds_the_time_of_day() {
        // `WorldClient.tick` advances the world age every tick and the time of
        // day only while the cycle runs (`WorldClient.java:66-74`); the frozen
        // flag is the sign the receive rule read (`:468-483`). One frozen
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
}
