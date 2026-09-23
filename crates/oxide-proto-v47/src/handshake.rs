//! Serverbound Handshake (state: handshake, id 0x00).

use std::io::{self, Write};

use oxide_proto::varint::write_varint;

/// Writes the handshake packet body: packet id, protocol, host, port, next state.
///
/// `next_state` is 1 to ask for a status response and 2 to begin a login; the
/// host is the length-prefixed server address the client dialled, and the port
/// follows big endian, as the wire format requires.
pub fn write_handshake(
    mut out: impl Write,
    protocol: i32,
    host: &str,
    port: u16,
    next_state: i32,
) -> io::Result<()> {
    out.write_all(&[0x00])?;
    write_varint(&mut out, protocol)?;
    write_string(&mut out, host)?;
    out.write_all(&port.to_be_bytes())?;
    write_varint(&mut out, next_state)
}

/// Builds the handshake packet body for a caller that wants the raw bytes.
pub fn handshake_payload(protocol: i32, host: &str, port: u16, next_state: i32) -> Vec<u8> {
    let mut out = Vec::new();
    write_handshake(&mut out, protocol, host, port, next_state)
        .expect("writing to a Vec cannot fail");
    out
}

/// Writes a length-prefixed string: a VarInt byte count, then the UTF-8 bytes.
fn write_string(mut out: impl Write, value: &str) -> io::Result<()> {
    write_varint(&mut out, value.len() as i32)?;
    out.write_all(value.as_bytes())
}
