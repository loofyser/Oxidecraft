//! Stream encryption for framed connections: the cipher sits below framing,
//! so every byte on the wire — length field included — passes through it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::rc::Rc;

use oxide_proto::conn::Conn;
use oxide_proto::crypto::{CipherPair, SHARED_SECRET_LEN};
use oxide_proto::frame::{Compression, write_frame};
use oxide_proto::varint::read_varint;

/// A fixed session secret: deterministic wire bytes, no randomness in tests.
const SECRET: [u8; SHARED_SECRET_LEN] = *b"0123456789abcdef";

/// One direction of the memory pipe: a shared byte queue.
#[derive(Clone, Default)]
struct Pipe {
    inner: Rc<RefCell<VecDeque<u8>>>,
}

impl Pipe {
    fn snapshot(&self) -> Vec<u8> {
        self.inner.borrow().iter().copied().collect()
    }

    fn push(&self, bytes: &[u8]) {
        self.inner.borrow_mut().extend(bytes.iter().copied());
    }
}

/// An in-memory stream: reads drain the inbound queue, writes feed the
/// outbound queue. [`MemStream::pair`] wires two of them crosswise, like a
/// loopback socket pair with inspectable bytes.
struct MemStream {
    inbound: Pipe,
    outbound: Pipe,
}

impl MemStream {
    fn pair() -> (Self, Self) {
        let a_to_b: Pipe = Pipe::default();
        let b_to_a: Pipe = Pipe::default();
        (
            Self {
                inbound: b_to_a.clone(),
                outbound: a_to_b.clone(),
            },
            Self {
                inbound: a_to_b,
                outbound: b_to_a,
            },
        )
    }
}

impl Read for MemStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut queue = self.inbound.inner.borrow_mut();
        let mut n = 0;
        while n < out.len() {
            match queue.pop_front() {
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

impl Write for MemStream {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.outbound
            .inner
            .borrow_mut()
            .extend(data.iter().copied());
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn encrypted_bytes_hide_the_length_field() {
    // Packet ID + data; the plaintext framing is `len` then these bytes.
    let payload = b"\x00hello-encryption".to_vec();

    let (plain_end, _) = MemStream::pair();
    let mut plain = Conn::new(plain_end);
    plain.send(&payload).expect("plaintext send");
    let plain_bytes = plain.into_inner().outbound.snapshot();

    let (enc_end, _) = MemStream::pair();
    let mut enc = Conn::new(enc_end);
    assert!(!enc.is_encrypted(), "opt-in: never encrypted by default");
    enc.enable_encryption(&SECRET);
    assert!(enc.is_encrypted(), "enabled after the login boundary");
    enc.send(&payload).expect("encrypted send");
    let raw = enc.into_inner().outbound.snapshot();

    assert_ne!(
        raw, plain_bytes,
        "the wire bytes must differ from the plaintext framing"
    );
    // The length field travels encrypted: the raw stream's first varint is
    // not the plaintext length (or it does not even decode).
    let mut prefix = &raw[..];
    if let Ok(first) = read_varint(&mut prefix) {
        assert_ne!(
            first,
            payload.len() as i32,
            "the raw first varint must not be the plaintext length"
        );
    }

    // An independently constructed pair over the same secret recovers the
    // exact plaintext framing — decrypting is required to recover it.
    let mut decrypted = raw.clone();
    CipherPair::new(&SECRET).decrypt(&mut decrypted);
    assert_eq!(
        decrypted, plain_bytes,
        "independent decrypt must reproduce the plaintext framing"
    );
    let mut body = &decrypted[..];
    assert_eq!(
        read_varint(&mut body).expect("decrypted length decodes"),
        payload.len() as i32,
        "decrypting recovers the true length field"
    );
}

#[test]
fn three_encrypted_frames_read_back_continuously() {
    // The peer encrypts three hand-built frames through one continuous
    // context; the enabled `Conn` must decrypt them across frame boundaries
    // with no per-frame re-initialisation.
    let payloads: Vec<Vec<u8>> = vec![
        b"\x00first".to_vec(),
        b"\x01second-payload".to_vec(),
        vec![0x02, 0x7f, 0x80, 0x00, 0xff],
    ];
    let mut wire = Vec::new();
    for payload in &payloads {
        write_frame(&mut wire, payload, Compression::Disabled).expect("a frame");
    }
    CipherPair::new(&SECRET).encrypt(&mut wire);

    let (receiving_end, _) = MemStream::pair();
    receiving_end.inbound.push(&wire);
    let mut receiving = Conn::new(receiving_end);
    assert!(!receiving.is_encrypted());
    receiving.enable_encryption(&SECRET);
    for payload in &payloads {
        assert_eq!(
            receiving.recv().expect("each frame decrypts"),
            *payload,
            "frames must read back byte-equal with no per-frame re-init"
        );
    }
}

#[test]
fn encryption_composes_with_compression() {
    // The cipher sits below framing: the mode switch happens mid-stream over
    // the encrypted bytes, and the compression layer never sees raw stream
    // bytes in either direction.
    let mode = Compression::Enabled { threshold: 256 };
    let pre = b"\x00before-compression".to_vec();
    let small = b"\x01tiny".to_vec();
    let large = vec![0x5au8; 4096];

    let (a_end, b_end) = MemStream::pair();
    let wiretap = a_end.outbound.clone();
    let mut sending = Conn::new(a_end);
    let mut receiving = Conn::new(b_end);
    sending.enable_encryption(&SECRET);
    receiving.enable_encryption(&SECRET);

    sending.send(&pre).expect("pre-switch send");
    // Snapshot before the peer drains the queue: the pipe is a queue, not a
    // log, so each frame's wire bytes are tapped before its `recv`.
    let raw_pre = wiretap.snapshot();
    assert_eq!(receiving.recv().expect("pre-switch recv"), pre);

    sending.set_compression(mode);
    receiving.set_compression(mode);
    sending.send(&small).expect("below-threshold send");
    let raw_small = wiretap.snapshot();
    assert_eq!(receiving.recv().expect("below-threshold recv"), small);
    sending.send(&large).expect("above-threshold send");
    let raw_large = wiretap.snapshot();
    assert_eq!(receiving.recv().expect("above-threshold recv"), large);

    // The wire carries the compressed framing through the cipher: decrypting
    // the tapped bytes with an independent pair reproduces the plaintext
    // framing (plain, then below-threshold, then compressed) byte for byte.
    let raw: Vec<u8> = raw_pre
        .into_iter()
        .chain(raw_small)
        .chain(raw_large)
        .collect();
    let mut expected = Vec::new();
    write_frame(&mut expected, &pre, Compression::Disabled).expect("plain frame");
    write_frame(&mut expected, &small, mode).expect("uncompressed frame");
    write_frame(&mut expected, &large, mode).expect("compressed frame");
    assert_ne!(
        raw, expected,
        "the compressed frame's bytes must travel encrypted too"
    );
    let mut decrypted = raw.clone();
    CipherPair::new(&SECRET).decrypt(&mut decrypted);
    assert_eq!(
        decrypted, expected,
        "independent decrypt must reproduce the compressed framing"
    );
}

#[test]
fn two_encrypted_connections_loop_back_both_directions() {
    let (a_end, b_end) = MemStream::pair();
    let mut a = Conn::new(a_end);
    let mut b = Conn::new(b_end);
    assert!(
        !a.is_encrypted() && !b.is_encrypted(),
        "opt-in: neither side starts encrypted"
    );
    a.enable_encryption(&SECRET);
    b.enable_encryption(&SECRET);
    assert!(
        a.is_encrypted() && b.is_encrypted(),
        "both sides are encrypted after enabling"
    );

    // Mixed sizes both directions, including one payload wider than the 8 KiB
    // read chunk, so decryption spans several stream reads per frame.
    let payloads: Vec<Vec<u8>> = vec![
        vec![0x00],
        b"\x01hello".to_vec(),
        vec![0x02u8; 300],
        vec![0x03u8; 4096],
        (0..1024u32).map(|i| (i % 251) as u8).collect(),
        vec![0x05u8; 7],
        vec![0x06u8; 100_000],
        b"\x07bye".to_vec(),
    ];
    for (i, payload) in payloads.iter().enumerate() {
        if i % 2 == 0 {
            a.send(payload).expect("a sends");
            assert_eq!(b.recv().expect("b receives"), *payload);
        } else {
            b.send(payload).expect("b sends");
            assert_eq!(a.recv().expect("a receives"), *payload);
        }
    }
}
