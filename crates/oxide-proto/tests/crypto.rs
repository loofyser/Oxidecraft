//! M7 Task 1: crypto evidence — goldens, KAT, RSA, hostile DER.
//!
//! Golden vectors: docs/research/launcher-assets-auth-survey.md section 3.7.
//! Cipher KAT provenance: recorded beside the fixture below.

use oxide_proto::crypto::{
    CipherPair, CryptoError, SHARED_SECRET_LEN, encrypt_rsa, hexdigest, new_secret_key,
    parse_server_key, server_id_hash,
};

// Step 1: the three golden vectors, verbatim from the survey (section 3.7).
#[test]
fn hexdigest_notch_is_the_published_positive_vector() {
    assert_eq!(
        hexdigest(b"Notch"),
        "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48"
    );
}

#[test]
fn hexdigest_jeb_is_the_published_negative_vector() {
    assert_eq!(
        hexdigest(b"jeb_"),
        "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1"
    );
}

#[test]
fn hexdigest_simon_strips_the_leading_zero() {
    assert_eq!(
        hexdigest(b"simon"),
        "88e16a1019277b15d58faf0541e11910eb756f6"
    );
}

// Step 2: the empty-input digest, pinned to its computed value
// (sha1("") = da39a3ee... is negative, so the hexdigest carries `-`).
#[test]
fn hexdigest_empty_input_is_the_computed_negative_digest() {
    assert_eq!(hexdigest(b""), "-25c65c11a194b4f2cdaa40106a9fe76f5027f8f7");
}

// Step 2: `server_id_hash` concatenates id || secret || key DER in that
// order; a non-empty pure-ASCII id is pinned through the manual
// concatenation so the ordering and the ASCII byte rule are both covered.
#[test]
fn server_id_hash_concatenates_id_secret_and_key() {
    let secret = [0xabu8; 16];
    let key_der = [0x01u8, 0x02, 0x03];
    let mut raw = Vec::new();
    raw.extend_from_slice(b"test-server");
    raw.extend_from_slice(&secret);
    raw.extend_from_slice(&key_der);
    assert_eq!(
        server_id_hash("test-server", &secret, &key_der),
        hexdigest(&raw)
    );
    // The id actually contributes: empty and non-empty differ.
    assert_ne!(
        server_id_hash("", &secret, &key_der),
        server_id_hash("test-server", &secret, &key_der)
    );
}

#[test]
fn server_id_hash_caps_an_overlong_id_at_the_protocol_ceiling() {
    // The crate's own string rule (MAX_STRING_BYTES, 32767) caps the id:
    // a longer id hashes as its truncated prefix.
    let secret = [0x01u8; 16];
    let long = "a".repeat(40_000);
    assert_eq!(
        server_id_hash(&long, &secret, &[]),
        server_id_hash(&long[..32_767], &secret, &[])
    );
}

// Step 3: the cipher pair.
//
// KAT provenance: the fixture below is synthetic bytes (no Mojang content)
// generated once with:
//   openssl enc -aes-128-cfb8 \
//     -K 00112233445566778899aabbccddeeff \
//     -iv 00112233445566778899aabbccddeeff \
//     -in pt.bin
// where pt.bin holds 64 bytes 0x00..0x3f, with OpenSSL 3.6.4 25 Aug 2026
// (Library: OpenSSL 3.6.4 25 Aug 2026).
fn kat_key() -> [u8; SHARED_SECRET_LEN] {
    [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ]
}

fn kat_plaintext() -> Vec<u8> {
    (0u8..64).collect()
}

fn kat_ciphertext() -> Vec<u8> {
    from_hex(
        "62b1e00b51105f34da28d079cec0b45b406e9c83ab35f62479d77e3ef4d35a7e8da34a0d52262f4a8cb30ae07df05f99f12bf31dc642a0de0386977422d655f8",
    )
}

fn from_hex(hex: &str) -> Vec<u8> {
    assert!(hex.len() % 2 == 0, "test fixture must be even hex");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("fixture hex"))
        .collect()
}

#[test]
fn cipher_pair_matches_the_openssl_kat() {
    let mut pair = CipherPair::new(&kat_key());
    let mut buf = kat_plaintext();
    pair.encrypt(&mut buf);
    assert_eq!(buf, kat_ciphertext());
}

#[test]
fn cipher_pair_encryption_is_continuous_across_calls() {
    // 16 + 48 in two calls must equal the one-shot 64-byte encryption:
    // the context carries state and is never re-initialised per call.
    let mut split = CipherPair::new(&kat_key());
    let mut buf = kat_plaintext();
    split.encrypt(&mut buf[..16]);
    split.encrypt(&mut buf[16..]);
    assert_eq!(buf, kat_ciphertext());
}

#[test]
fn cipher_pair_decrypt_inverts_encrypt() {
    let mut pair = CipherPair::new(&kat_key());
    let mut buf = kat_plaintext();
    pair.encrypt(&mut buf);
    assert_eq!(buf, kat_ciphertext());
    pair.decrypt(&mut buf);
    assert_eq!(buf, kat_plaintext());
}

// `new_secret_key` yields a fresh 128-bit secret per call.
#[test]
fn new_secret_key_yields_fresh_16_byte_secrets() {
    assert_eq!(SHARED_SECRET_LEN, 16);
    let first = new_secret_key();
    let second = new_secret_key();
    assert_eq!(first.len(), SHARED_SECRET_LEN);
    assert_ne!(first, second);
}

// Steps 4-5: RSA.
//
// The tests generate their own keypairs in-test (no fixtures, no key
// material committed). Key generation needs the `rand_core` 0.6 interface
// `rsa` 0.9 speaks, so the test carries the same small bridge as the
// library's private one.
struct TestRng(rand::rngs::ThreadRng);

impl rsa::rand_core::RngCore for TestRng {
    fn next_u32(&mut self) -> u32 {
        use rand::TryRng as _;
        self.0.try_next_u32().expect("test entropy failure")
    }

    fn next_u64(&mut self) -> u64 {
        use rand::TryRng as _;
        self.0.try_next_u64().expect("test entropy failure")
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        use rand::TryRng as _;
        self.0.try_fill_bytes(dest).expect("test entropy failure");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rsa::rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl rsa::rand_core::CryptoRng for TestRng {}

fn generate_keypair(bits: usize) -> rsa::RsaPrivateKey {
    rsa::RsaPrivateKey::new(&mut TestRng(rand::rng()), bits).expect("test keygen")
}

fn public_der(private: &rsa::RsaPrivateKey) -> Vec<u8> {
    use rsa::pkcs8::EncodePublicKey as _;
    rsa::RsaPublicKey::from(private)
        .to_public_key_der()
        .expect("test DER encode")
        .as_bytes()
        .to_vec()
}

#[test]
fn rsa_1024_parse_encrypt_and_decrypt_roundtrip() {
    use rsa::traits::PublicKeyParts as _;
    let private = generate_keypair(1024);
    let der = public_der(&private);
    let public = parse_server_key(&der).expect("valid SPKI parses");
    // The parse round-trips: re-encoding gives back the same DER.
    assert_eq!(public_der(&private), {
        use rsa::pkcs8::EncodePublicKey as _;
        public
            .to_public_key_der()
            .expect("re-encode")
            .as_bytes()
            .to_vec()
    });
    let plaintext = b"sixteen-byte-secret!";
    let ciphertext = encrypt_rsa(&public, plaintext).expect("short plaintext encrypts");
    // Ciphertext sized from the parsed modulus, never hardcoded.
    assert_eq!(ciphertext.len(), public.size());
    assert_eq!(ciphertext.len(), 128);
    let recovered = private
        .decrypt(rsa::Pkcs1v15Encrypt, &ciphertext)
        .expect("private key decrypts");
    assert_eq!(recovered, plaintext);
}

#[test]
fn rsa_2048_ciphertext_is_modulus_sized() {
    use rsa::traits::PublicKeyParts as _;
    let private = generate_keypair(2048);
    let public = parse_server_key(&public_der(&private)).expect("valid SPKI parses");
    let ciphertext = encrypt_rsa(&public, b"session secret").expect("encrypts");
    // A 2048-bit modulus must yield 256 bytes: this is the case that fails
    // if the length were ever a hardcoded 128.
    assert_eq!(ciphertext.len(), public.size());
    assert_eq!(ciphertext.len(), 256);
    let recovered = private
        .decrypt(rsa::Pkcs1v15Encrypt, &ciphertext)
        .expect("private key decrypts");
    assert_eq!(recovered, b"session secret");
}

#[test]
fn rsa_oversized_plaintext_is_a_typed_error() {
    let private = generate_keypair(1024);
    let public = parse_server_key(&public_der(&private)).expect("valid SPKI parses");
    // PKCS#1 v1.5 carries at most modulus - 11 bytes: 117 of 128 here.
    let oversized = vec![0x55u8; 128 - 11];
    assert!(
        matches!(
            encrypt_rsa(&public, &oversized),
            Err(CryptoError::Encrypt { .. })
        ),
        "plaintext of modulus - 11 bytes must be refused"
    );
}

#[test]
fn rsa_empty_der_is_a_typed_error() {
    assert!(matches!(
        parse_server_key(&[]),
        Err(CryptoError::KeyParse { .. })
    ));
}

// Step 5: the hostile DER set. Every input below is a typed `CryptoError`,
// never a panic. The decided rule for trailing garbage is: reject.
#[test]
fn hostile_der_inputs_are_typed_errors() {
    let private = generate_keypair(1024);
    let der = public_der(&private);

    // Truncated DER.
    let truncated = &der[..der.len() / 2];
    assert!(matches!(
        parse_server_key(truncated),
        Err(CryptoError::KeyParse { .. })
    ));

    // Valid DER with trailing garbage: rejected.
    let mut trailing = der.clone();
    trailing.push(0x00);
    assert!(matches!(
        parse_server_key(&trailing),
        Err(CryptoError::KeyParse { .. })
    ));

    // Zero-length input.
    assert!(matches!(
        parse_server_key(&[]),
        Err(CryptoError::KeyParse { .. })
    ));

    // A non-RSA algorithm in a well-formed SubjectPublicKeyInfo: an EC P-256
    // key. Synthetic bytes generated once with
    // `openssl ecparam -name prime256v1 -genkey -noout | openssl ec -pubout
    // -outform DER` (OpenSSL 3.6.4); no Mojang content.
    let ec_spki = from_hex(
        "3059301306072a8648ce3d020106082a8648ce3d030107034200043965f273fcea7037d4d6706bc0c57044fe8fa82c78c1701f13b15a9407bba6e7cb8e4e252d7b59d8270511da25792382a103d4c3dbf76b3278b52dc3c354d12f",
    );
    assert!(matches!(
        parse_server_key(&ec_spki),
        Err(CryptoError::KeyParse { .. })
    ));

    // Random bytes of key-like length.
    assert!(matches!(
        parse_server_key(&[0x30u8; 162]),
        Err(CryptoError::KeyParse { .. })
    ));
}
