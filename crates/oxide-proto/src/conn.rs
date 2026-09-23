//! A framed connection over one stream: buffered reads and coalesced writes.

use std::io::{self, Read, Write};

use crate::frame::{Compression, FrameError, read_frame, write_frame};

/// Bytes pulled from the stream in one read call.
const READ_CHUNK: usize = 8 * 1024;

/// A connection that frames packets, buffering both directions.
///
/// Reads fill an internal buffer and are served from it, so a burst of packets
/// costs one stream read rather than one per field. Writes are appended to a
/// buffer and flushed once per [`Conn::send`], so a frame reaches the stream in
/// a single write call.
///
/// The same buffer backs the bare [`Write`] impl: bytes written through it
/// leave only when [`Write::flush`] runs, and [`Conn::into_inner`] drops
/// anything still staged. A caller that writes through `Write` must flush
/// before unwrapping the connection.
#[derive(Debug)]
pub struct Conn<S> {
    /// The underlying stream.
    stream: S,
    /// The read buffer and the window of it already consumed.
    read_buf: Vec<u8>,
    read_start: usize,
    read_end: usize,
    /// Bytes staged for the next flush.
    write_buf: Vec<u8>,
    /// The framing mode in force.
    compression: Compression,
}

impl<S: Read + Write> Conn<S> {
    /// Wraps `stream`.
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            read_buf: vec![0; READ_CHUNK],
            read_start: 0,
            read_end: 0,
            write_buf: Vec::new(),
            compression: Compression::Disabled,
        }
    }

    /// The framing mode in force.
    pub fn compression(&self) -> Compression {
        self.compression
    }

    /// Switches the framing mode, as Set Compression requires.
    pub fn set_compression(&mut self, mode: Compression) {
        self.compression = mode;
    }

    /// Frames and sends one `Packet ID + Data` payload.
    ///
    /// The frame is staged in the write buffer and flushed to the stream before
    /// this returns, so one send costs one stream write. A failed flush leaves
    /// the connection unusable — part of the frame may already have reached the
    /// stream — and the caller must not keep sending on it.
    pub fn send(&mut self, payload: &[u8]) -> Result<(), FrameError> {
        let compression = self.compression;
        write_frame(&mut *self, payload, compression)?;
        self.flush().map_err(FrameError::Io)
    }

    /// Reads one frame and returns the decompressed payload.
    pub fn recv(&mut self) -> Result<Vec<u8>, FrameError> {
        let compression = self.compression;
        read_frame(&mut *self, compression)
    }

    /// Returns the wrapped stream.
    ///
    /// Bytes staged by the [`Write`] impl and not yet flushed are dropped
    /// rather than pushed to the stream; flush first if you wrote through it.
    pub fn into_inner(self) -> S {
        self.stream
    }

    /// Refills the read buffer from the stream.
    ///
    /// The window is reset only once the read succeeds. A failed read leaves it
    /// empty, so the next read retries the stream instead of replaying bytes
    /// that were already handed out.
    fn fill(&mut self) -> io::Result<()> {
        let n = self.stream.read(&mut self.read_buf)?;
        self.read_start = 0;
        self.read_end = n;
        Ok(())
    }
}

impl<S: Read + Write> Read for Conn<S> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.read_start == self.read_end {
            self.fill()?;
        }
        let available = self.read_end - self.read_start;
        let n = available.min(out.len());
        out[..n].copy_from_slice(&self.read_buf[self.read_start..self.read_start + n]);
        self.read_start += n;
        Ok(n)
    }
}

impl<S: Read + Write> Write for Conn<S> {
    /// Stages `data` in the write buffer; nothing reaches the stream until
    /// [`Write::flush`] or [`Conn::send`] runs.
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.write_buf.extend_from_slice(data);
        Ok(data.len())
    }

    /// Pushes the staged bytes to the stream, then flushes it.
    ///
    /// A failure leaves the staged bytes in place, and the connection must not
    /// be used for further sends.
    fn flush(&mut self) -> io::Result<()> {
        if !self.write_buf.is_empty() {
            self.stream.write_all(&self.write_buf)?;
            self.write_buf.clear();
        }
        self.stream.flush()
    }
}
