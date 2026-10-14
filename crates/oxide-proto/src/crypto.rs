//! Session cryptography for the online-mode login (protocol 47).
//!
//! This module owns the four primitives the encrypted session needs: the
//! non-standard Minecraft hexdigest, the AES-128-CFB8 cipher pair, RSA
//! PKCS#1 v1.5 encryption over a parsed `SubjectPublicKeyInfo`, and the
//! shared-secret generation.
//!
//! Behaviour pins (paraphrased from the pinned reference tree
//! `refs/_src/MCP-919`, never copied):
//!
//! - The shared secret is a fresh 128-bit AES key per session
//!   (`CryptManager.java`'s key-generator path, `createNewSharedKey`).
//! - The server-id hash digests the server-id string bytes, then the secret,
//!   then the server key's encoded form, in that order (`CryptManager.java`'s
//!   `getServerIdHash`); the id bytes use the Latin-1-compatible encoding,
//!   which is byte-identical to UTF-8 for ASCII.
//! - The hash is printed by interpreting the SHA-1 output as one signed
//!   big-endian integer and rendering it in base 16 with a `-` prefix when
//!   negative and no leading zeros (`NetHandlerLoginClient.java`'s login
//!   handler builds it through the big-integer hex conversion).
//! - Protocol encryption runs AES in CFB mode with 8-bit segments and no
//!   padding, keyed with the shared secret used as its own initial vector
//!   (`CryptManager.java`'s network-cipher constructor); the connection keeps
//!   one cipher per direction and both run continuously across packets
//!   (`NetworkManager.java`'s `enableEncryption` installs an encoder and a
//!   decoder above framing).
//!
//! `SubjectPublicKeyInfo` parsing finding (verified first-hand against the
//! `rsa` 0.9.10 sources): the `rsa` crate re-exports `pkcs8`
//! (`src/lib.rs`: `pub use pkcs8;`), and `RsaPublicKey` implements the
//! re-exported `pkcs8::DecodePublicKey` trait, whose `from_public_key_der`
//! entry point parses DER `SubjectPublicKeyInfo` directly. No `x509-cert`
//! dependency is needed, so per Decision 14 it is not added.

use aes::Aes128;
use cfb8::cipher::KeyIvInit;
use cfb8::{Decryptor, Encryptor};
use sha1::{Digest, Sha1};

use crate::codec::MAX_STRING_BYTES;

/// Length of the session's shared secret in bytes: a 128-bit AES key.
pub const SHARED_SECRET_LEN: usize = 16;

/// Failures from the session-crypto primitives.
///
/// No variant carries key material: parse and encryption failures report only
/// fixed sentences, never the offending bytes.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// A server key's DER did not parse as an RSA `SubjectPublicKeyInfo`.
    #[error("server key did not parse: {message}")]
    KeyParse {
        /// What was wrong, without any key bytes.
        message: String,
    },
    /// RSA encryption failed (for example the plaintext was too long).
    #[error("RSA encryption failed: {message}")]
    Encrypt {
        /// What was wrong, without any key or plaintext bytes.
        message: String,
    },
}

/// Generates a fresh random 128-bit shared secret from the OS randomness
/// source (the `KeyGenerator` analogue).
#[must_use]
pub fn new_secret_key() -> [u8; SHARED_SECRET_LEN] {
    use rand::RngExt as _;
    let mut secret = [0u8; SHARED_SECRET_LEN];
    rand::rng().fill(&mut secret[..]);
    secret
}

/// The Minecraft hexdigest: SHA-1 over `data`, printed by treating the
/// 20-byte digest as one large signed big-endian integer (two's complement)
/// and rendering it in base 16, lowercase, with a `-` prefix when negative
/// and no leading zeros.
///
/// Roughly half of all digests print with a leading `-`; an all-zero digest
/// prints `0`.
#[must_use]
pub fn hexdigest(data: impl AsRef<[u8]>) -> String {
    let digest = Sha1::digest(data.as_ref());
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&digest);
    format_digest(&bytes)
}

/// Renders a raw 20-byte SHA-1 digest with the signed-magnitude rule above.
///
/// Kept as a private seam so the `0` rule is pinned by feeding digest bytes
/// directly, without forging SHA-1.
fn format_digest(digest: &[u8; 20]) -> String {
    let negative = digest[0] & 0x80 != 0;
    let mut magnitude = *digest;
    if negative {
        // Two's complement negate: invert, then add one.
        let mut carry: u16 = 1;
        for byte in magnitude.iter_mut().rev() {
            let value = u16::from(!*byte) + carry;
            *byte = value as u8;
            carry = value >> 8;
        }
    }
    let Some(first) = magnitude.iter().position(|&byte| byte != 0) else {
        return "0".to_owned();
    };
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&format!("{:x}", magnitude[first]));
    for byte in &magnitude[first + 1..] {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Computes the server-id hash: `hexdigest(server_id || secret || key_der)`.
///
/// `server_id` contributes its bytes as sent on the wire (identical to the
/// source's Latin-1 read for ASCII); the string is capped at the crate's own
/// [`MAX_STRING_BYTES`] protocol ceiling before hashing, so an unbounded
/// caller string cannot grow the hash input without bound.
#[must_use]
pub fn server_id_hash(
    server_id: &str,
    secret: &[u8; SHARED_SECRET_LEN],
    public_key_der: &[u8],
) -> String {
    let mut id = server_id.as_bytes();
    if id.len() > MAX_STRING_BYTES {
        // Truncate on a character boundary so the prefix stays valid UTF-8
        // (and therefore valid ASCII when the input is ASCII).
        let mut len = MAX_STRING_BYTES;
        while !server_id.is_char_boundary(len) {
            len -= 1;
        }
        id = &server_id.as_bytes()[..len];
    }
    let mut hasher = Sha1::new();
    hasher.update(id);
    hasher.update(secret);
    hasher.update(public_key_der);
    let digest = hasher.finalize();
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&digest);
    format_digest(&bytes)
}

/// Parses a server key from DER `SubjectPublicKeyInfo` (never PEM).
///
/// Trailing garbage after the DER structure is rejected: the parse must
/// consume the whole input. Any failure is a typed [`CryptoError::KeyParse`];
/// this function never panics on hostile input.
pub fn parse_server_key(der: &[u8]) -> Result<rsa::RsaPublicKey, CryptoError> {
    use rsa::pkcs8::DecodePublicKey as _;
    rsa::RsaPublicKey::from_public_key_der(der).map_err(|_| CryptoError::KeyParse {
        message: "DER did not parse as an RSA SubjectPublicKeyInfo".to_owned(),
    })
}

/// Bridges the workspace RNG (`rand` 0.10, whose traits live in `rand_core`
/// 0.10) to the `rand_core` 0.6 interface `rsa` 0.9 encrypts with
/// (re-exported as `rsa::rand_core`; `src/key.rs` encrypts with any
/// `R: CryptoRngCore`).
struct RsaRng(rand::rngs::ThreadRng);

impl rsa::rand_core::RngCore for RsaRng {
    fn next_u32(&mut self) -> u32 {
        use rand::TryRng as _;
        self.0.try_next_u32().expect("OS entropy failure")
    }

    fn next_u64(&mut self) -> u64 {
        use rand::TryRng as _;
        self.0.try_next_u64().expect("OS entropy failure")
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        use rand::TryRng as _;
        self.0.try_fill_bytes(dest).expect("OS entropy failure");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rsa::rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl rsa::rand_core::CryptoRng for RsaRng {}

/// Encrypts `plaintext` with PKCS#1 v1.5 padding (never OAEP).
///
/// The output length equals the parsed modulus's byte length
/// (`key.size()`), never a hardcoded constant.
///
/// The length guard is deliberately conservative: plaintexts of
/// `modulus - 11` bytes or more are refused with a typed
/// [`CryptoError::Encrypt`], even though PKCS#1 v1.5 could carry exactly
/// `modulus - 11`. The session only ever encrypts short values (the 16-byte
/// secret, short verify tokens), so the stricter bound cannot break the
/// real flow and keeps the failure at the typed boundary.
pub fn encrypt_rsa(key: &rsa::RsaPublicKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    use rsa::traits::PublicKeyParts as _;
    if plaintext.len().saturating_add(11) >= key.size() {
        return Err(CryptoError::Encrypt {
            message: "plaintext too long for RSA PKCS#1 v1.5".to_owned(),
        });
    }
    key.encrypt(&mut RsaRng(rand::rng()), rsa::Pkcs1v15Encrypt, plaintext)
        .map_err(|_| CryptoError::Encrypt {
            message: "RSA PKCS#1 v1.5 encryption failed".to_owned(),
        })
}

/// The session's two continuous AES-128-CFB8 contexts: one encrypts, one
/// decrypts. Both are keyed with the shared secret used as its own initial
/// vector, with an 8-bit segment size, and both carry state across calls —
/// they are never re-initialised per packet.
pub struct CipherPair {
    /// The outbound context.
    encryptor: Encryptor<Aes128>,
    /// The inbound context.
    decryptor: Decryptor<Aes128>,
}

impl CipherPair {
    /// Builds both contexts: key == IV == `secret`.
    #[must_use]
    pub fn new(secret: &[u8; SHARED_SECRET_LEN]) -> Self {
        let key: [u8; SHARED_SECRET_LEN] = *secret;
        Self {
            encryptor: Encryptor::<Aes128>::new((&key).into(), (&key).into()),
            decryptor: Decryptor::<Aes128>::new((&key).into(), (&key).into()),
        }
    }

    /// Encrypts `bytes` in place through the continuous encrypt context.
    pub fn encrypt(&mut self, bytes: &mut [u8]) {
        self.encryptor.encrypt(bytes);
    }

    /// Decrypts `bytes` in place through the continuous decrypt context.
    pub fn decrypt(&mut self, bytes: &mut [u8]) {
        self.decryptor.decrypt(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::format_digest;

    #[test]
    fn all_zero_digest_prints_zero() {
        assert_eq!(format_digest(&[0u8; 20]), "0");
    }

    #[test]
    fn positive_digest_strips_leading_zero_bytes() {
        let mut digest = [0u8; 20];
        digest[1] = 0x08;
        digest[2] = 0x8e;
        digest[19] = 0xf6;
        let mut expected = String::from("88e");
        expected.push_str(&"00".repeat(16));
        expected.push_str("f6");
        assert_eq!(format_digest(&digest), expected);
    }

    #[test]
    fn negative_digest_prints_magnitude_with_minus() {
        // 0xff...ff is -1 in two's complement.
        assert_eq!(format_digest(&[0xffu8; 20]), "-1");
    }

    #[test]
    fn min_value_negates_without_overflow() {
        // 0x80 followed by zeros is the most negative value; its magnitude
        // is a 1 bit past the 159 low bits.
        let mut digest = [0u8; 20];
        digest[0] = 0x80;
        let printed = format_digest(&digest);
        assert!(printed.starts_with('-'), "{printed}");
        assert_eq!(printed.len(), 1 + 40);
    }
}
