//! Serverbound packets: the login state now, the play state from Task 4 on.

use std::io::{self, Write};

use oxide_proto::codec::write_string;

/// Serverbound Login Start (id 0x00).
pub const LOGIN_START_ID: i32 = 0x00;

/// Writes Login Start: the packet id, then the name (16 characters at most).
pub fn write_login_start(mut out: impl Write, username: &str) -> io::Result<()> {
    oxide_proto::varint::write_varint(&mut out, LOGIN_START_ID)?;
    write_string(&mut out, username)
}
