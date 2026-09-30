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

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use oxide_assets::atlas::Atlas;
use oxide_proto::conn::{Conn, DeadlineStream, RecvOutcome};
use oxide_proto::frame::{Compression, FrameError};
use oxide_proto::varint::{VarIntError, read_varint};
use oxide_proto_v47::PacketError;
use oxide_proto_v47::clientbound::{
    self, ChunkData, JoinGame, KeepAlive, LoginPacket, MapChunkBulk, PlayDisconnect,
    PlayerListItem, PlayerPositionAndLook, PluginMessage, TimeUpdate, read_packet_id,
};
use oxide_proto_v47::handshake::write_handshake;
use oxide_proto_v47::serverbound::{
    ClientSettings, write_client_settings, write_keep_alive, write_login_start,
    write_player_position_and_look, write_plugin_message,
};
use oxide_proto_v47::{NEXT_STATE_LOGIN, PROTOCOL};
use oxide_render::terrain::ChunkMesh;
use oxide_world::biome::{ColorMap, TintMaps};
use oxide_world::sky::{
    celestial_angle, cloud_colour, moon_phase, sky_colour, star_brightness, sun_brightness,
};
use oxide_world::world::World;
use rayon::{ThreadPool, ThreadPoolBuildError, ThreadPoolBuilder};
use tracing::{debug, info, warn};

use crate::mesh_queue::{MeshJob, MeshQueue};
use crate::mesher::{
    BlockModelSet, ColumnSnapshot, MeshContext, SmoothLighting, build_column_meshes,
};

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
    /// The server placed or moved the player.
    PlayerPosition {
        /// Absolute x, after the relative flags were resolved.
        x: f64,
        /// Absolute y, after the relative flags were resolved.
        y: f64,
        /// Absolute z, after the relative flags were resolved.
        z: f64,
        /// Absolute yaw in degrees.
        yaw: f32,
        /// Absolute pitch in degrees.
        pitch: f32,
    },
    /// Time Update arrived: the world's clock.
    ///
    /// The time of day is the value the celestial angle and the moon phase are read from, and a
    /// negative one is the frozen-sun convention (`S03PacketTimeUpdate.java:17-31`): the sign is
    /// reported as received. The age is carried for later work.
    Time {
        /// The world's age in ticks.
        world_age: i64,
        /// The world's time of day in ticks; negative while the sun is frozen.
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
    },
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
    /// that is already readable is always read before any mesh work, and a
    /// build never runs on this thread, which is what keeps the keepalive echo
    /// — and the teleport echo — answerable while a burst of columns is
    /// arriving.
    pub fn run_over(self, events: &Sender<ClientEvent>) -> Result<(), SessionError> {
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
        let mut position = Position::default();
        let mut clock: Option<i64> = None;
        let mut players: HashMap<[u8; 16], String> = HashMap::new();

        loop {
            let payload = match conn.recv_or_idle(MESH_TICK) {
                Ok(RecvOutcome::Frame(payload)) => payload,
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
                    continue;
                }
                Err(error) if is_stream_end(&error) => {
                    debug!(error = %error, "the server closed the stream");
                    break;
                }
                Err(error) => return Err(SessionError::Frame(error)),
            };
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
                    // columns belong to the old world, and any job still out
                    // completes stale and builds against the new one.
                    world = Some(World::new(join.dimension == 0));
                    queue = MeshQueue::new();
                    let settings = payload_of(|out| write_client_settings(out, &config.settings))?;
                    send_reply(&mut conn, &settings)?;
                    let brand =
                        payload_of(|out| write_plugin_message(out, BRAND_CHANNEL, BRAND_PAYLOAD))?;
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
                PlayerPositionAndLook::ID => {
                    let teleport = decoded(id, PlayerPositionAndLook::decode(body))?;
                    position = position.resolved(&teleport);
                    let echo = payload_of(|out| {
                        write_player_position_and_look(
                            out,
                            position.x,
                            position.y,
                            position.z,
                            position.yaw,
                            position.pitch,
                            false,
                        )
                    })?;
                    send_reply(&mut conn, &echo)?;
                    report(
                        events,
                        ClientEvent::PlayerPosition {
                            x: position.x,
                            y: position.y,
                            z: position.z,
                            yaw: position.yaw,
                            pitch: position.pitch,
                        },
                    );
                    // The view block moved: the sky's colour is sampled at the player's own
                    // block, so a teleport can change it without a new clock.
                    report_sky(world.as_ref(), position, clock, events);
                }
                TimeUpdate::ID => {
                    let update = decoded(id, TimeUpdate::decode(body))?;
                    clock = Some(update.time_of_day);
                    report(
                        events,
                        ClientEvent::Time {
                            world_age: update.world_age,
                            time_of_day: update.time_of_day,
                        },
                    );
                    report_sky(world.as_ref(), position, clock, events);
                }
                ChunkData::ID => {
                    let Some(store) = world.as_mut() else {
                        warn!("a column arrived before Join Game built the world");
                        continue;
                    };
                    let column = decoded(id, ChunkData::decode(body, store.has_sky()))?;
                    if store.apply_chunk_data(&column) {
                        mark_column_changed(store, &mut queue, column.chunk_x, column.chunk_z);
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
                MapChunkBulk::ID => {
                    let Some(store) = world.as_mut() else {
                        warn!("a bulk column arrived before Join Game built the world");
                        continue;
                    };
                    let bulk = decoded(id, MapChunkBulk::decode(body))?;
                    store.apply_bulk(&bulk);
                    for column in &bulk.columns {
                        if store.chunk(column.chunk_x, column.chunk_z).is_some() {
                            mark_column_changed(store, &mut queue, column.chunk_x, column.chunk_z);
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
                PlayerListItem::ID => {
                    // The decoder takes the add action only, while a live
                    // server sends latency, gamemode and display-name updates
                    // as a matter of course. Reading the action first keeps
                    // those out of the error path without guessing at the
                    // decoder's fields.
                    match read_varint(body) {
                        Ok(PlayerListItem::ACTION_ADD) => {
                            let list = decoded(id, PlayerListItem::decode(body))?;
                            for entry in list.entries {
                                if let Some(name) = entry.name {
                                    players.insert(entry.uuid, name);
                                }
                            }
                        }
                        Ok(action) => {
                            debug!(
                                action = action,
                                "skipping a Player List Item M1 has no use for"
                            );
                        }
                        Err(error) => {
                            warn!(packet_id = id, error = %error, "the player list action did not read");
                            return Err(SessionError::Packet(error.into()));
                        }
                    }
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
    /// that already exists, exactly as [`Session::run_over`] does.
    pub fn run(self, events: &Sender<ClientEvent>) -> Result<(), SessionError> {
        self.run_over(events)
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

/// The absolute position the last teleport settled on.
///
/// It is what the relative flags of the next teleport are applied to, and it is
/// the camera as far as M1 is concerned: there is no movement of our own.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Position {
    /// World x.
    x: f64,
    /// World y.
    y: f64,
    /// World z.
    z: f64,
    /// Look yaw in degrees.
    yaw: f32,
    /// Look pitch in degrees.
    pitch: f32,
}

impl Position {
    /// Resolves a teleport into absolute values: a flagged axis is a delta on
    /// the position the session holds, an unflagged one is already absolute.
    fn resolved(self, teleport: &PlayerPositionAndLook) -> Self {
        let flags = teleport.flags;
        Self {
            x: if flags & PlayerPositionAndLook::FLAG_X != 0 {
                self.x + teleport.x
            } else {
                teleport.x
            },
            y: if flags & PlayerPositionAndLook::FLAG_Y != 0 {
                self.y + teleport.y
            } else {
                teleport.y
            },
            z: if flags & PlayerPositionAndLook::FLAG_Z != 0 {
                self.z + teleport.z
            } else {
                teleport.z
            },
            yaw: if flags & PlayerPositionAndLook::FLAG_YAW != 0 {
                self.yaw + teleport.yaw
            } else {
                teleport.yaw
            },
            pitch: if flags & PlayerPositionAndLook::FLAG_PITCH != 0 {
                self.pitch + teleport.pitch
            } else {
                teleport.pitch
            },
        }
    }
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

/// Reports the sky the clock and the view block produce, when both are known.
///
/// The angle and the colours come from the world clock; the block is the render-view entity's
/// own, floored exactly as `World.getSkyColor` floors it (`World.java:1437-1443`). M2 has no
/// client tick loop, so the partial tick is zero, and no weather is decoded, so the rain
/// strength is zero; both are recorded in the module docs and the report. A session with no
/// clock yet — or no world, as before Join Game — has no sky to report.
fn report_sky(
    world: Option<&World>,
    position: Position,
    clock: Option<i64>,
    events: &Sender<ClientEvent>,
) {
    let (Some(world), Some(time_of_day)) = (world, clock) else {
        return;
    };
    /// M2 renders from the last update with no tick to interpolate over.
    const PARTIAL_TICKS: f32 = 0.0;
    /// No weather packets are decoded yet, so the rain strength is zero.
    const RAIN_STRENGTH: f32 = 0.0;
    let angle = celestial_angle(time_of_day, PARTIAL_TICKS);
    let colour = sky_colour(
        world,
        position.x.floor() as i32,
        position.y.floor() as i32,
        position.z.floor() as i32,
        angle,
    );
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
