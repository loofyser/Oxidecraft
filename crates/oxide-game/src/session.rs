//! The client session: one framed connection, driven to completion.
//!
//! The session owns what the window cannot: the connection, the version 47
//! world store, and the column meshes (plan Decision 3). It logs in, answers
//! everything a vanilla server expects of a client — the keepalive echo, the
//! teleport echo, Client Settings and the brand plugin message — and reports
//! what the window draws through a [`ClientEvent`] channel.
//!
//! A packet the client has no use for is skipped, and one whose id it cannot
//! even name is not fatal (spec S2), so a proxy or a modded server cannot end
//! the session by sending something unexpected.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::TcpStream;

use crossbeam_channel::Sender;
use oxide_proto::conn::Conn;
use oxide_proto::frame::{Compression, FrameError};
use oxide_proto::varint::{VarIntError, read_varint};
use oxide_proto_v47::PacketError;
use oxide_proto_v47::clientbound::{
    self, ChunkData, JoinGame, KeepAlive, LoginPacket, MapChunkBulk, PlayDisconnect,
    PlayerListItem, PlayerPositionAndLook, PluginMessage, read_packet_id,
};
use oxide_proto_v47::handshake::write_handshake;
use oxide_proto_v47::serverbound::{
    ClientSettings, write_client_settings, write_keep_alive, write_login_start,
    write_player_position_and_look, write_plugin_message,
};
use oxide_proto_v47::{NEXT_STATE_LOGIN, PROTOCOL};
use oxide_render::terrain::ChunkMesh;
use oxide_world::world::World;
use tracing::{debug, info, warn};

use crate::mesher::build_column_meshes;

/// The plugin channel a vanilla client announces itself on.
const BRAND_CHANNEL: &str = "MC|Brand";

/// The brand payload, as the capture records the vanilla client sending it: a
/// length-prefixed `vanilla`.
const BRAND_PAYLOAD: &[u8] = b"\x07vanilla";

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
}

/// A client session over one framed connection.
pub struct Session<S> {
    /// The framed connection to the server.
    conn: Conn<S>,
    /// What the session connects with and sends.
    config: SessionConfig,
}

impl Session<TcpStream> {
    /// Connects and returns the session; nothing is sent until [`Session::run`].
    pub fn connect(config: &SessionConfig) -> Result<Self, SessionError> {
        let stream = TcpStream::connect((config.host.as_str(), config.port))?;
        Ok(Self::new(Conn::new(stream), config.clone()))
    }
}

impl<S: Read + Write> Session<S> {
    /// Wraps an existing connection.
    pub fn new(conn: Conn<S>, config: SessionConfig) -> Self {
        Self { conn, config }
    }

    /// Runs the session to completion: handshake, login, then the play loop.
    ///
    /// Returns `Ok(())` when the server closed the connection or the stream
    /// ended. Malformed packets are reported as an error, never a panic.
    pub fn run_over(self, events: &Sender<ClientEvent>) -> Result<(), SessionError> {
        let Session { mut conn, config } = self;

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
        let mut players: HashMap<[u8; 16], String> = HashMap::new();

        loop {
            let Some(payload) = recv_frame(&mut conn)? else {
                debug!("the server closed the stream");
                return Ok(());
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
                    // re-sent Join Game starts the column set over.
                    world = Some(World::new(join.dimension == 0));
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
                }
                ChunkData::ID => {
                    let Some(store) = world.as_mut() else {
                        warn!("a column arrived before Join Game built the world");
                        continue;
                    };
                    let column = decoded(id, ChunkData::decode(body, store.has_sky()))?;
                    if store.apply_chunk_data(&column) {
                        remesh(store, column.chunk_x, column.chunk_z, events);
                    } else {
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
                            remesh(store, column.chunk_x, column.chunk_z, events);
                        } else {
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
/// server closes a session, so both end the loop quietly.
fn is_stream_end(error: &FrameError) -> bool {
    match error {
        FrameError::Io(cause) => cause.kind() == io::ErrorKind::UnexpectedEof,
        FrameError::VarInt(VarIntError::UnexpectedEof) => true,
        FrameError::VarInt(VarIntError::Io(cause)) => cause.kind() == io::ErrorKind::UnexpectedEof,
        FrameError::VarInt(VarIntError::TooLong)
        | FrameError::TooLong(..)
        | FrameError::NegativeLength(..)
        | FrameError::BadCompression
        | FrameError::EmptyPayload => false,
    }
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

/// Re-meshes a column and its four neighbours (plan Decision 4) and reports
/// each of them.
///
/// The applied column leads its neighbours in a fixed order. All five report
/// every section, `None` for one that draws nothing, which is how the window
/// learns to drop a mesh it still holds; a neighbour that is not loaded reports
/// sixteen empty sections to the same end.
fn remesh(world: &World, cx: i32, cz: i32, events: &Sender<ClientEvent>) {
    report_chunk(world, cx, cz, events);
    for (nx, nz) in [(cx + 1, cz), (cx - 1, cz), (cx, cz + 1), (cx, cz - 1)] {
        report_chunk(world, nx, nz, events);
    }
}

/// Builds one column's meshes and reports them.
fn report_chunk(world: &World, cx: i32, cz: i32, events: &Sender<ClientEvent>) {
    report(
        events,
        ClientEvent::ChunkUpdated {
            cx,
            cz,
            sections: build_column_meshes(world, cx, cz),
        },
    );
}
