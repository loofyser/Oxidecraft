//! Tests for the buffered framed connection: write coalescing, buffered reads,
//! and the compression switch.

use std::io::{self, Read, Write};

use oxide_proto::conn::Conn;
use oxide_proto::frame::{Compression, FrameError, write_frame};
use oxide_proto::varint::VarIntError;

/// An in-memory stream that records how the connection calls the underlying
/// reader and writer: one entry per `read` call and one byte count per `write`
/// call, plus everything written.
#[derive(Default)]
struct CountingStream {
    to_read: std::collections::VecDeque<u8>,
    written: Vec<u8>,
    write_calls: Vec<usize>,
    read_calls: usize,
}

impl CountingStream {
    fn with_input(bytes: &[u8]) -> Self {
        Self {
            to_read: bytes.iter().copied().collect(),
            ..Self::default()
        }
    }
}

impl Read for CountingStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.read_calls += 1;
        let mut n = 0;
        while n < out.len() {
            match self.to_read.pop_front() {
                Some(byte) => {
                    out[n] = byte;
                    n += 1;
                }
                None => break,
            }
        }
        Ok(n)
    }
}

impl Write for CountingStream {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.write_calls.push(data.len());
        self.written.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One scripted read result: bytes to hand back, or an error kind to raise.
enum Script {
    /// Serve these bytes.
    Bytes(Vec<u8>),
    /// Fail the read with this error kind.
    Fail(std::io::ErrorKind),
}

/// A stream that replays a fixed list of read results, one entry per call, so a
/// test can place a failure at an exact read. Writes are discarded.
struct ScriptedStream {
    script: std::collections::VecDeque<Script>,
}

impl ScriptedStream {
    fn new(script: Vec<Script>) -> Self {
        Self {
            script: script.into(),
        }
    }
}

impl Read for ScriptedStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        match self.script.pop_front() {
            Some(Script::Bytes(bytes)) => {
                assert!(
                    bytes.len() <= out.len(),
                    "scripted read of {} bytes into a {} byte buffer",
                    bytes.len(),
                    out.len()
                );
                out[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }
            Some(Script::Fail(kind)) => Err(std::io::Error::new(kind, "the scripted read fails")),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the script is exhausted",
            )),
        }
    }
}

impl Write for ScriptedStream {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn one_send_reaches_the_stream_as_one_write() {
    // The M0 carry-over: the VarInt path issued one write per byte. A buffered
    // writer must hand the whole frame to the stream in a single call.
    let stream = CountingStream::default();
    let mut conn = Conn::new(stream);
    conn.send(&[0x00, 0x7f]).expect("send");
    let writes = conn.into_inner().write_calls;
    assert_eq!(writes.len(), 1, "writes: {writes:?}");
}

#[test]
fn frames_round_trip_through_the_connection() {
    let stream = CountingStream::with_input(&[]);
    let mut conn = Conn::new(stream);
    conn.send(b"first").expect("send");
    conn.send(b"second").expect("send");
    let stream = conn.into_inner();

    let mut reading = Conn::new(CountingStream::with_input(&stream.written));
    assert_eq!(reading.recv().expect("recv"), b"first");
    assert_eq!(reading.recv().expect("recv"), b"second");
    // The second frame was already in the buffer, so the stream was read once.
    assert_eq!(reading.into_inner().read_calls, 1);
}

#[test]
fn compression_applies_after_the_switch() {
    let payload = vec![0x5a_u8; 4096];
    let mut sending = Conn::new(CountingStream::default());
    sending.set_compression(Compression::Enabled { threshold: 256 });
    sending.send(&payload).expect("send");
    let bytes = sending.into_inner().written;

    let mut receiving = Conn::new(CountingStream::with_input(&bytes));
    receiving.set_compression(Compression::Enabled { threshold: 256 });
    assert_eq!(receiving.recv().expect("recv"), payload);
}

#[test]
fn a_truncated_stream_is_an_error_not_a_panic() {
    let mut conn = Conn::new(CountingStream::with_input(&[0x05, 0x01]));
    assert!(conn.recv().is_err(), "a short frame must be reported");
}

#[test]
fn a_failed_refill_does_not_replay_consumed_bytes() {
    let mut first = Vec::new();
    write_frame(&mut first, b"first", Compression::Disabled).expect("frame");
    let mut second = Vec::new();
    write_frame(&mut second, b"second", Compression::Disabled).expect("frame");

    let stream = ScriptedStream::new(vec![
        Script::Bytes(first),
        Script::Fail(std::io::ErrorKind::WouldBlock),
        Script::Bytes(second),
    ]);
    let mut conn = Conn::new(stream);

    assert_eq!(conn.recv().expect("first recv"), b"first");

    // The frame length prefix is read before anything else, so the failed
    // refill surfaces through the VarInt layer, keeping the io error kind.
    let error = conn.recv().expect_err("the failed refill must surface");
    assert!(
        matches!(
            &error,
            FrameError::VarInt(VarIntError::Io(cause)) if cause.kind() == io::ErrorKind::WouldBlock
        ),
        "error: {error:?}"
    );

    // The third receive serves the second frame. Replaying the first one would
    // mean the failed refill left its bytes in the read window.
    assert_eq!(conn.recv().expect("second recv"), b"second");
}
