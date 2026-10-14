//! The encrypted login flow against an honest mock server.
//!
//! A thread listens on an ephemeral port and speaks the login sequence the
//! way a vanilla server does: it reads the handshake and Login Start, writes
//! an Encryption Request built over a fresh 1024-bit RSA keypair, reads the
//! client's Encryption Response from the plaintext stream, RSA-decrypts both
//! arrays with the private key, enables a matching cipher pair, and sends Set
//! Compression and Login Success through the encrypted stream. The refusal
//! cases assert the client sends literally nothing after the request.
//!
//! The RSA keypair is generated with the `openssl` CLI rather than a new
//! crate: the task's frozen-file rule keeps `Cargo.toml` untouched, and the
//! CLI still gives a fresh real keypair per test with real PKCS#1 v1.5
//! decryption on the server side.

use std::io::{Cursor, Read};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use oxide_game::session::{
    JoinServer, LoginOutcome, Session, SessionAccount, SessionConfig, SessionError,
};
use oxide_proto::codec::{read_string, write_string};
use oxide_proto::conn::Conn;
use oxide_proto::crypto::{CryptoError, server_id_hash};
use oxide_proto::frame::Compression;
use oxide_proto::varint::{read_varint, write_varint};
use oxide_proto_v47::clientbound::read_packet_id;
use tempfile::TempDir;

/// How long the mock server waits for a close it must see (a refusal) before
/// failing instead of hanging the suite.
const SERVER_WAIT: Duration = Duration::from_secs(10);

/// The fixed verify token every request carries.
const VERIFY_TOKEN: [u8; 4] = [4, 3, 2, 1];

/// The test account's UUID, 32 hex digits without dashes.
const ACCOUNT_UUID_HEX: &str = "069a79f444e94726a5befca90e38aaf5";

/// The test account's UUID as Login Success sends it, hyphenated.
const ACCOUNT_UUID_DASHED: &str = "069a79f4-44e9-4726-a5be-fca90e38aaf5";

/// The profile name Login Start sends and Login Success echoes.
const USERNAME: &str = "TestPlayer";

/// The access token the harness joiner records.
const ACCESS_TOKEN: &str = "test-access-token";

/// Runs one `openssl` invocation, failing the test with its stderr when it
/// does not exit cleanly.
fn openssl(args: &[&str]) {
    let output = Command::new("openssl")
        .args(args)
        .output()
        .expect("the openssl CLI runs");
    assert!(
        output.status.success(),
        "openssl {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Generates a fresh 1024-bit RSA keypair: returns the public key's SPKI DER
/// and the private key's path for the server's decrypts.
fn fresh_keypair(dir: &TempDir) -> (Vec<u8>, PathBuf) {
    let private = dir.path().join("server.pem");
    let public_der = dir.path().join("server-pub.der");
    openssl(&["genrsa", "-out", private.to_str().unwrap(), "1024"]);
    openssl(&[
        "rsa",
        "-in",
        private.to_str().unwrap(),
        "-pubout",
        "-outform",
        "DER",
        "-out",
        public_der.to_str().unwrap(),
    ]);
    (
        std::fs::read(&public_der).expect("the DER reads back"),
        private,
    )
}

/// RSA-decrypts one PKCS#1 v1.5 ciphertext with the server's private key.
fn rsa_decrypt(dir: &TempDir, private: &Path, ciphertext: &[u8]) -> Vec<u8> {
    let input = dir.path().join("ct.bin");
    let output = dir.path().join("pt.bin");
    std::fs::write(&input, ciphertext).expect("the ciphertext stages");
    openssl(&[
        "pkeyutl",
        "-decrypt",
        "-inkey",
        private.to_str().unwrap(),
        "-in",
        input.to_str().unwrap(),
        "-out",
        output.to_str().unwrap(),
    ]);
    std::fs::read(&output).expect("the plaintext reads back")
}

/// The `JoinServer` seam's test double: records every call, and fails when
/// told to.
#[derive(Debug, Default)]
struct RecordingJoiner {
    /// Every `(uuid_hex, access_token, server_id)` the session posted.
    calls: Mutex<Vec<(String, String, String)>>,
    /// When set, every call fails with this message.
    failure: Option<String>,
}

impl JoinServer for RecordingJoiner {
    fn join(&self, uuid_hex: &str, access_token: &str, server_id: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push((
            uuid_hex.to_owned(),
            access_token.to_owned(),
            server_id.to_owned(),
        ));
        match &self.failure {
            Some(message) => Err(message.clone()),
            None => Ok(()),
        }
    }
}

/// Builds the test account over `joiner`.
fn account(joiner: Arc<RecordingJoiner>) -> Arc<SessionAccount> {
    Arc::new(SessionAccount {
        uuid_hex: ACCOUNT_UUID_HEX.to_owned(),
        access_token: ACCESS_TOKEN.to_owned(),
        joiner,
    })
}

/// The session config under test: the mock server's port, the profile name,
/// and `account`.
fn config(port: u16, account: Option<Arc<SessionAccount>>) -> SessionConfig {
    SessionConfig {
        host: "127.0.0.1".to_owned(),
        port,
        username: USERNAME.to_owned(),
        settings: oxide_proto_v47::serverbound::ClientSettings::default(),
        mesh: None,
        account,
    }
}

/// Reads one length-prefixed byte array from `cursor`, panicking on a hostile
/// length or a short read: the server writes these scripts, so either is a
/// test bug.
fn read_array(cursor: &mut Cursor<&[u8]>) -> Vec<u8> {
    let len = read_varint(&mut *cursor).expect("the array length reads");
    assert!(len >= 0, "the array length is not negative");
    let start = cursor.position() as usize;
    let end = start + len as usize;
    assert!(end <= cursor.get_ref().len(), "the array is fully present");
    cursor.set_position(end as u64);
    cursor.get_ref()[start..end].to_vec()
}

/// Writes one length-prefixed byte array.
fn write_array(out: &mut Vec<u8>, bytes: &[u8]) {
    write_varint(&mut *out, bytes.len() as i32).expect("writing to a Vec cannot fail");
    out.extend_from_slice(bytes);
}

/// The Encryption Request payload the mock server sends: the id, the server
/// id string, the DER key and the verify token as length-prefixed arrays.
fn encryption_request_payload(server_id: &str, public_key_der: &[u8], token: &[u8]) -> Vec<u8> {
    let mut out = vec![0x01];
    write_string(&mut out, server_id).expect("writing to a Vec cannot fail");
    write_array(&mut out, public_key_der);
    write_array(&mut out, token);
    out
}

/// Reads the handshake and Login Start, asserting the profile name the
/// client sent.
fn read_handshake_and_start(conn: &mut Conn<TcpStream>) {
    let handshake = conn.recv().expect("the handshake arrives");
    let (id, _) = read_packet_id(&handshake).expect("the handshake id reads");
    assert_eq!(id, 0x00, "the first frame is the handshake");
    let start = conn.recv().expect("Login Start arrives");
    let (id, rest) = read_packet_id(&start).expect("the Login Start id reads");
    assert_eq!(id, 0x00, "the second frame is Login Start");
    let mut cursor = Cursor::new(rest);
    let name = read_string(&mut cursor, 16).expect("the name reads");
    assert_eq!(name, USERNAME, "Login Start carries the profile name");
}

/// The client's Encryption Response, split into its two ciphertexts.
fn read_response(conn: &mut Conn<TcpStream>) -> (Vec<u8>, Vec<u8>) {
    let payload = conn.recv().expect("the Encryption Response arrives");
    let (id, rest) = read_packet_id(&payload).expect("the response id reads");
    assert_eq!(id, 0x01, "the response carries id 0x01");
    let mut cursor = Cursor::new(rest);
    (read_array(&mut cursor), read_array(&mut cursor))
}

/// Asserts the stream closes with no further bytes: the refusal cases must
/// send literally nothing after the request.
fn assert_no_response_bytes(conn: Conn<TcpStream>) {
    let mut stream = conn.into_inner();
    stream
        .set_read_timeout(Some(SERVER_WAIT))
        .expect("the timeout sets");
    let mut probe = [0u8; 1024];
    match stream.read(&mut probe) {
        Ok(0) => {}
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
        Ok(n) => panic!("the client sent {n} response bytes after the refusal"),
        Err(error) => panic!("waiting for the close failed: {error:?}"),
    }
}

/// The happy path: the full encrypted login. The server returns the shared
/// secret and the DER it sent, so the test recomputes the expected hash
/// independently and checks the recorded join call against it.
#[test]
fn the_encrypted_login_runs_through_the_cipher() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("the port reads").port();
    let server: JoinHandle<(Vec<u8>, Vec<u8>)> = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("the client connects");
        stream
            .set_read_timeout(Some(SERVER_WAIT))
            .expect("the timeout sets");
        let mut conn = Conn::new(stream);
        read_handshake_and_start(&mut conn);
        let scratch = TempDir::new().expect("a server scratch dir");
        let (der, private) = fresh_keypair(&scratch);
        conn.send(&encryption_request_payload("", &der, &VERIFY_TOKEN))
            .expect("the request sends");
        let (encrypted_secret, encrypted_token) = read_response(&mut conn);
        let secret = rsa_decrypt(&scratch, &private, &encrypted_secret);
        assert_eq!(secret.len(), 16, "the secret decrypts to 16 bytes");
        let echoed = rsa_decrypt(&scratch, &private, &encrypted_token);
        assert_eq!(
            echoed, VERIFY_TOKEN,
            "the token decrypts to the request's token"
        );
        let mut secret16 = [0u8; 16];
        secret16.copy_from_slice(&secret);
        conn.enable_encryption(&secret16);
        let mut set_compression = vec![0x03];
        write_varint(&mut set_compression, 256).expect("writing to a Vec cannot fail");
        conn.send(&set_compression).expect("Set Compression sends");
        conn.set_compression(Compression::Enabled { threshold: 256 });
        let mut success = vec![0x02];
        write_string(&mut success, ACCOUNT_UUID_DASHED).expect("writing to a Vec cannot fail");
        write_string(&mut success, USERNAME).expect("writing to a Vec cannot fail");
        conn.send(&success).expect("Login Success sends");
        (secret, der)
    });

    let joiner = Arc::new(RecordingJoiner::default());
    let mut session =
        Session::connect(&config(port, Some(account(Arc::clone(&joiner))))).expect("connect");
    let (events, _) = crossbeam_channel::unbounded();
    match session.run_login(&events).expect("the login settles") {
        LoginOutcome::Accepted { uuid, username } => {
            assert_eq!(uuid, ACCOUNT_UUID_DASHED, "the accepted uuid");
            assert_eq!(username, USERNAME, "the accepted name");
        }
        LoginOutcome::Ended => panic!("the login was accepted, not ended"),
    }
    assert!(
        session.is_encrypted(),
        "the ciphers are enabled after the response"
    );
    drop(session);

    let (secret, der) = server.join().expect("the server thread");
    let mut secret16 = [0u8; 16];
    secret16.copy_from_slice(&secret);
    let expected_hash = server_id_hash("", &secret16, &der);
    let calls = joiner.calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        1,
        "the join ran exactly once, before the response"
    );
    assert_eq!(calls[0].0, ACCOUNT_UUID_HEX, "the join's uuid");
    assert_eq!(calls[0].1, ACCESS_TOKEN, "the join's token");
    assert_eq!(
        calls[0].2, expected_hash,
        "the join's hash recomputed independently"
    );
}

/// Without an account the session refuses with the sign-in sentence, and the
/// server sees no response bytes.
#[test]
fn an_encryption_request_without_an_account_is_refused_silently() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("the port reads").port();
    let server: JoinHandle<()> = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("the client connects");
        stream
            .set_read_timeout(Some(SERVER_WAIT))
            .expect("the timeout sets");
        let mut conn = Conn::new(stream);
        read_handshake_and_start(&mut conn);
        let scratch = TempDir::new().expect("a server scratch dir");
        let (der, _) = fresh_keypair(&scratch);
        conn.send(&encryption_request_payload("", &der, &VERIFY_TOKEN))
            .expect("the request sends");
        assert_no_response_bytes(conn);
    });

    let mut session = Session::connect(&config(port, None)).expect("connect");
    let (events, _) = crossbeam_channel::unbounded();
    match session.run_login(&events) {
        Err(SessionError::EncryptionRequired) => {}
        other => panic!("expected the refusal, got {other:?}"),
    }
    let message = SessionError::EncryptionRequired.to_string();
    assert_eq!(
        message,
        "this server requires a signed-in account — run `oxide-launcher auth`, then start the client with --account",
        "the refusal carries the sign-in sentence"
    );
    drop(session);
    server.join().expect("the server thread");
}

/// A failing join is a typed error with the seam's text, and no response
/// byte is sent.
#[test]
fn a_failing_join_sends_no_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("the port reads").port();
    let server: JoinHandle<()> = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("the client connects");
        stream
            .set_read_timeout(Some(SERVER_WAIT))
            .expect("the timeout sets");
        let mut conn = Conn::new(stream);
        read_handshake_and_start(&mut conn);
        let scratch = TempDir::new().expect("a server scratch dir");
        let (der, _) = fresh_keypair(&scratch);
        conn.send(&encryption_request_payload("", &der, &VERIFY_TOKEN))
            .expect("the request sends");
        assert_no_response_bytes(conn);
    });

    let joiner = Arc::new(RecordingJoiner {
        failure: Some("the session service said no (test)".to_owned()),
        ..RecordingJoiner::default()
    });
    let mut session =
        Session::connect(&config(port, Some(account(Arc::clone(&joiner))))).expect("connect");
    let (events, _) = crossbeam_channel::unbounded();
    match session.run_login(&events) {
        Err(SessionError::JoinFailed { message }) => {
            assert_eq!(message, "the session service said no (test)");
        }
        other => panic!("expected the join failure, got {other:?}"),
    }
    let calls = joiner.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "the join ran before the refusal");
    assert_eq!(calls[0].0, ACCOUNT_UUID_HEX, "the join's uuid");
    assert_eq!(calls[0].1, ACCESS_TOKEN, "the join's token");
    assert!(!calls[0].2.is_empty(), "the join carried the computed hash");
    drop(session);
    server.join().expect("the server thread");
}

/// A malformed server key is a typed crypto error before any join or any
/// response byte.
#[test]
fn a_malformed_server_key_is_a_typed_error_with_no_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("the port reads").port();
    let server: JoinHandle<()> = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("the client connects");
        stream
            .set_read_timeout(Some(SERVER_WAIT))
            .expect("the timeout sets");
        let mut conn = Conn::new(stream);
        read_handshake_and_start(&mut conn);
        conn.send(&encryption_request_payload(
            "",
            b"this is not DER",
            &VERIFY_TOKEN,
        ))
        .expect("the request sends");
        assert_no_response_bytes(conn);
    });

    let joiner = Arc::new(RecordingJoiner::default());
    let mut session =
        Session::connect(&config(port, Some(account(Arc::clone(&joiner))))).expect("connect");
    let (events, _) = crossbeam_channel::unbounded();
    match session.run_login(&events) {
        Err(SessionError::Crypto(CryptoError::KeyParse { .. })) => {}
        other => panic!("expected the key-parse failure, got {other:?}"),
    }
    assert!(
        joiner.calls.lock().unwrap().is_empty(),
        "no join runs on an unparsed key"
    );
    drop(session);
    server.join().expect("the server thread");
}
