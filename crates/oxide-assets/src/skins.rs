//! The skin pipeline: the default-skin rule, the profile-property decode and
//! the on-disk skin cache.
//!
//! The rule mirrors `DefaultPlayerSkin` (`client/resources/DefaultPlayerSkin.java`):
//! the two default models Steve (wide) and Alex (slim) are chosen from the
//! player's UUID by the low bit of the Java `UUID.hashCode`
//! (`isSlimSkin`, `:41-44`). A profile property's `textures` value carries the
//! custom skin's URL and model, with an optional cape
//! (`MinecraftProfileTexture`'s shape, read by `NetworkPlayerInfo.getSkinType`,
//! `client/network/NetworkPlayerInfo.java:87-90`); the decode walks the JSON by
//! hand, and everything it cannot vouch for is an error the caller treats as
//! "no custom skin". Fetched textures land in the store's `skins/` directory —
//! the era's `skinCacheDir` (per-hash files, `SkinManager.loadSkin`,
//! `client/resources/SkinManager.java:60-105`) — as the PNG bytes exactly as
//! the server's CDN served them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;

use crate::http::{HttpClient, HttpError, UreqClient};
use crate::store::{Store, StoreError, write_atomic};
use crate::texture::{Texture, TextureError};

/// The subdirectory of the store the skin cache writes into.
const SKINS_SUBDIR: &str = "skins";

/// The largest decoded profile property accepted, in bytes.
const MAX_PROPERTY_BYTES: usize = 4096;

/// The largest base64 form of a [`MAX_PROPERTY_BYTES`] payload: four
/// characters per three bytes, rounded up — 5464 characters.
const MAX_PROPERTY_B64_CHARS: usize = MAX_PROPERTY_BYTES.div_ceil(3) * 4;

/// The longest texture URL accepted, in bytes.
const MAX_URL_BYTES: usize = 512;

/// The longest cache name accepted: a SHA-256 hex digest.
const MAX_HASH_CHARS: usize = 64;

/// The texel size of any skin this client draws, on both axes.
const SKIN_SIZE: u32 = 64;

/// Errors from the skin pipeline.
#[derive(Debug, thiserror::Error)]
pub enum SkinError {
    /// The profile property is not valid base64.
    #[error("the profile property is not valid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    /// The decoded profile property is not UTF-8 text.
    #[error("the profile property is not UTF-8 text: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    /// The decoded profile property is not valid JSON.
    #[error("the profile property is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The decoded profile property is over the size cap.
    #[error("the profile property decodes to more than {MAX_PROPERTY_BYTES} bytes")]
    PropertyTooLarge,
    /// The property names no SKIN texture.
    #[error("the profile property names no SKIN texture")]
    MissingSkin,
    /// The SKIN entry names no usable url.
    #[error("the profile property's SKIN texture names no url")]
    MissingSkinUrl,
    /// A texture URL is not an http(s) URL within the size cap.
    #[error("the texture url {url:?} is not an http(s) url of at most {MAX_URL_BYTES} bytes")]
    BadUrl {
        /// The offending URL.
        url: String,
    },
    /// A texture URL has no hex hash to name its cache file.
    #[error("the texture url {url:?} has no hex texture hash as its last path segment")]
    BadCacheName {
        /// The offending URL.
        url: String,
    },
    /// The transport failed.
    #[error("the skin fetch failed: {0}")]
    Http(#[from] HttpError),
    /// The fetched bytes are not a decodable PNG.
    #[error("the skin image could not be decoded: {0}")]
    Texture(#[from] TextureError),
    /// The image is not a 64x64 skin.
    #[error("the skin is {width}x{height} texels; this client draws 64x64 skins only")]
    WrongSize {
        /// The image's width in texels.
        width: u32,
        /// The image's height in texels.
        height: u32,
    },
    /// A cache write failed.
    #[error("the skin cache could not be written: {0}")]
    Store(#[from] StoreError),
}

/// A profile property's decoded instructions: the skin's URL and model, and
/// the cape's URL when the property carries one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileTexture {
    /// The skin texture's URL: an http or https URL of at most
    /// [`MAX_URL_BYTES`] bytes.
    pub url: String,
    /// The model the metadata asks for; the slim model only when the metadata
    /// says exactly so.
    pub model: DefaultModel,
    /// The cape texture's URL when the property carries a usable one.
    pub cape_url: Option<String>,
}

/// Decodes a profile property's `textures` value.
///
/// The value is strict base64 — padded, no stray characters — of a JSON
/// document whose `textures.SKIN.url` names the skin and whose
/// `textures.SKIN.metadata.model` may name the slim model; a well-formed
/// `textures.CAPE.url` rides along as [`ProfileTexture::cape_url`]. The
/// decoded document must fit [`MAX_PROPERTY_BYTES`] and the URLs
/// [`MAX_URL_BYTES`]; every violation is a [`SkinError`] — the walk treats the
/// value as hostile throughout and never panics on any input.
pub fn decode_profile_property(value_b64: &str) -> Result<ProfileTexture, SkinError> {
    // The encoded length cannot be shorter than the decoded one, so this
    // refuses an oversized blob before it allocates.
    if value_b64.len() > MAX_PROPERTY_B64_CHARS {
        return Err(SkinError::PropertyTooLarge);
    }
    let bytes = STANDARD.decode(value_b64)?;
    if bytes.len() > MAX_PROPERTY_BYTES {
        return Err(SkinError::PropertyTooLarge);
    }
    let text = std::str::from_utf8(&bytes)?;
    let root: Value = serde_json::from_str(text)?;

    let textures = root.get("textures");
    let skin = textures
        .and_then(|textures| textures.get("SKIN"))
        .ok_or(SkinError::MissingSkin)?;
    let url = skin
        .get("url")
        .and_then(Value::as_str)
        .ok_or(SkinError::MissingSkinUrl)?;
    if !is_texture_url(url) {
        return Err(SkinError::BadUrl {
            url: url.to_owned(),
        });
    }
    let model = match skin
        .get("metadata")
        .and_then(|metadata| metadata.get("model"))
        .and_then(Value::as_str)
    {
        Some("slim") => DefaultModel::Slim,
        _ => DefaultModel::Wide,
    };
    let cape_url = textures
        .and_then(|textures| textures.get("CAPE"))
        .and_then(|cape| cape.get("url"))
        .and_then(Value::as_str)
        .filter(|url| is_texture_url(url))
        .map(str::to_owned);
    Ok(ProfileTexture {
        url: url.to_owned(),
        model,
        cape_url,
    })
}

/// True when `url` is an http(s) URL within the size cap.
fn is_texture_url(url: &str) -> bool {
    url.len() <= MAX_URL_BYTES && is_http_url(url)
}

/// True when `url` carries an `http` or `https` scheme.
///
/// The HTTP layer's own rule (`http.rs`), repeated here so the decode and the
/// cache refuse the same shapes without a live client.
fn is_http_url(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    })
}

/// The session server's profile endpoint: `GET <endpoint>/<uuid_hex>`
/// answers the player's `properties` array, whose `textures` entries carry
/// the skin's URL and model
/// (docs/research/launcher-assets-auth-survey.md §3.6). The endpoint is
/// public: no bearer travels.
const PROFILE_ENDPOINT: &str = "https://sessionserver.mojang.com/session/minecraft/profile";

/// The largest profile body accepted, in bytes: room for a small array of
/// max-size properties in their base64 form plus the JSON around them —
/// orders of magnitude above the survey's ~700-byte shape, so no real
/// profile is ever refused, while a hostile body never reaches the parse.
const MAX_PROFILE_BODY_BYTES: usize = 8 * MAX_PROPERTY_B64_CHARS;

/// Fetches the signed-in player's textures from the session server's profile
/// endpoint: GETs [`PROFILE_ENDPOINT`] under the 32-hex-digit `uuid_hex`
/// and decodes every `textures` entry of the `properties` array through
/// [`decode_profile_property`].
///
/// Unknown fields — `profileActions`, anything the future adds — are ignored
/// rather than rejected (§3.6's own note for a 1.8.9 client). A body-less
/// answer (204) and a 404 both mean the account carries no textures and come
/// back as an empty list. The body is refused past
/// [`MAX_PROFILE_BODY_BYTES`] before the JSON walk; every other failure is
/// the fetch's or the decode's own typed error.
pub fn fetch_profile_textures(
    http: &dyn HttpClient,
    uuid_hex: &str,
) -> Result<Vec<ProfileTexture>, SkinError> {
    let url = format!("{PROFILE_ENDPOINT}/{uuid_hex}");
    let body = match http.get(&url) {
        Ok(body) => body,
        Err(HttpError::Status { code: 404, .. }) => return Ok(Vec::new()),
        Err(error) => return Err(SkinError::Http(error)),
    };
    if body.is_empty() {
        return Ok(Vec::new());
    }
    if body.len() > MAX_PROFILE_BODY_BYTES {
        return Err(SkinError::PropertyTooLarge);
    }
    let root: Value = serde_json::from_slice(&body)?;
    let Some(properties) = root.get("properties").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut textures = Vec::new();
    for property in properties {
        let (Some(name), Some(value)) = (
            property.get("name").and_then(Value::as_str),
            property.get("value").and_then(Value::as_str),
        ) else {
            continue;
        };
        if name == "textures" {
            textures.push(decode_profile_property(value)?);
        }
    }
    Ok(textures)
}

/// The skin cache: 64x64 PNGs under the store's `skins/` directory, named by
/// the texture URL's own hash.
///
/// The name is the URL's last path segment — the CDN's texture hash — after a
/// hex and length check, so a URL that reached here unvalidated cannot steer
/// the path out of the store. The era shards the directory by the hash's
/// first two characters (`SkinManager.loadSkin`,
/// `client/resources/SkinManager.java:74-75`); this milestone's layout is
/// flat, `skins/<hash>.png`.
pub struct SkinCache {
    /// The resolved `skins/` directory.
    dir: PathBuf,
    /// The HTTP client fetches travel through — the agent and retry rules of
    /// `http.rs`, unchanged.
    http: UreqClient,
}

impl SkinCache {
    /// Opens the cache under `store`'s `skins/` directory.
    ///
    /// [`Store::open`] creates that directory; a write from here recreates it
    /// if it went missing.
    pub fn new(store: &Store) -> Self {
        Self {
            dir: store.root().join(SKINS_SUBDIR),
            http: UreqClient::new(),
        }
    }

    /// The path a texture URL stores under, when it names one.
    ///
    /// `None` when the URL is not an http(s) URL of at most
    /// [`MAX_URL_BYTES`] bytes, or its last path segment is not 1 to
    /// [`MAX_HASH_CHARS`] hex characters — the refusals [`SkinError::BadUrl`]
    /// and [`SkinError::BadCacheName`] name.
    pub fn path_for(&self, url: &str) -> Option<PathBuf> {
        cache_name(url).ok().map(|name| self.path_for_name(name))
    }

    /// Loads the cached texture for `url`, when a decodable 64x64 PNG is on
    /// disk.
    ///
    /// A URL outside the shape, a missing file and a file that is not a 64x64
    /// skin are all `None` — a cache miss the fetch may replace.
    pub fn load(&self, url: &str) -> Option<Arc<Texture>> {
        let name = cache_name(url).ok()?;
        Self::load_file(&self.path_for_name(name))
    }

    /// Stores `png_bytes` under `url`'s name, with the store's atomic write.
    ///
    /// The bytes must already be a decoded 64x64 skin
    /// ([`SkinCache::fetch`] decodes before writing); a URL outside the shape
    /// is refused.
    pub fn store(&self, url: &str, png_bytes: &[u8]) -> Result<(), SkinError> {
        let name = cache_name(url)?;
        write_atomic(&self.path_for_name(name), png_bytes).map_err(SkinError::from)
    }

    /// Fetches `url` unless it is already cached, decodes it and caches it.
    ///
    /// The bytes travel through [`UreqClient`] — the agent and retry rules are
    /// the HTTP layer's own — and must decode to a 64x64 PNG; anything else is
    /// an error, and bytes that failed the decode are never written.
    pub fn fetch(&self, url: &str) -> Result<Arc<Texture>, SkinError> {
        self.fetch_with(&self.http, url)
    }

    /// [`SkinCache::fetch`] against any HTTP client: the seam the tests stub.
    fn fetch_with(&self, http: &dyn HttpClient, url: &str) -> Result<Arc<Texture>, SkinError> {
        let name = cache_name(url)?;
        let path = self.path_for_name(name);
        if let Some(texture) = Self::load_file(&path) {
            return Ok(texture);
        }
        let bytes = http.get(url)?;
        let texture = decode_skin(&bytes)?;
        write_atomic(&path, &bytes)?;
        Ok(Arc::new(texture))
    }

    /// The final path for an already-validated cache name.
    fn path_for_name(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.png"))
    }

    /// Reads and decodes one cache file; every failure is a miss.
    fn load_file(path: &Path) -> Option<Arc<Texture>> {
        let bytes = std::fs::read(path).ok()?;
        let texture = decode_skin(&bytes).ok()?;
        Some(Arc::new(texture))
    }
}

/// The cache name a texture URL supplies: its last path segment, when the URL
/// is an http(s) URL within the size cap and the segment is a hex hash of at
/// most [`MAX_HASH_CHARS`] characters.
fn cache_name(url: &str) -> Result<&str, SkinError> {
    if !is_texture_url(url) {
        return Err(SkinError::BadUrl {
            url: url.to_owned(),
        });
    }
    let Some((_, rest)) = url.split_once("://") else {
        return Err(SkinError::BadUrl {
            url: url.to_owned(),
        });
    };
    let name = rest
        .find('/')
        .and_then(|slash| rest[slash + 1..].split(['?', '#']).next())
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| {
            !name.is_empty()
                && name.len() <= MAX_HASH_CHARS
                && name.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
    name.ok_or_else(|| SkinError::BadCacheName {
        url: url.to_owned(),
    })
}

/// Decodes PNG bytes and enforces the skin size rule: 64x64 or nothing.
///
/// The era converts a legacy 64x32 image itself (`TextureManager`'s legacy
/// path); that conversion is not built here, and the refusal names the
/// dimensions it saw rather than guessing (a recorded limit).
fn decode_skin(bytes: &[u8]) -> Result<Texture, SkinError> {
    let texture = Texture::from_png(bytes)?;
    if texture.width == SKIN_SIZE && texture.height == SKIN_SIZE {
        Ok(texture)
    } else {
        Err(SkinError::WrongSize {
            width: texture.width,
            height: texture.height,
        })
    }
}

/// The two default player models.
///
/// The names are the source's skin types: the wide (Steve) and slim (Alex)
/// models of `DefaultPlayerSkin` (`client/resources/DefaultPlayerSkin.java:8-12`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultModel {
    /// The classic, four-pixel-arm model.
    Wide,
    /// The slim, three-pixel-arm model.
    Slim,
}

/// The default model for `uuid`: the `DefaultPlayerSkin` rule.
///
/// `isSlimSkin` (`client/resources/DefaultPlayerSkin.java:41-44`) picks Alex
/// when the lowest bit of the UUID's Java `hashCode` is one and Steve
/// otherwise. Java's `UUID.hashCode` is the XOR of the two 32-bit halves of
/// `mostSigBits ^ leastSigBits`, so with `hilo` the halves' XOR the low bit is
/// `bit0(hilo) ^ bit32(hilo)`.
///
/// `uuid` must be the canonical hyphenated 8-4-4-4-12 hex form; anything else
/// has no computable hash and is the wide model.
pub fn default_skin(uuid: &str) -> DefaultModel {
    let Some((most, least)) = parse_uuid(uuid) else {
        return DefaultModel::Wide;
    };
    let hilo = most ^ least;
    if ((hilo >> 32) ^ hilo) & 1 == 1 {
        DefaultModel::Slim
    } else {
        DefaultModel::Wide
    }
}

/// Parses the canonical hyphenated UUID form into its two 64-bit halves.
///
/// The layout is 8-4-4-4-12 hex digits with the three interior hyphens at
/// byte indices 8, 13, 18 and 23; a hyphen anywhere else, a missing one, any
/// other length or any non-hex byte is `None`.
fn parse_uuid(uuid: &str) -> Option<(u64, u64)> {
    let bytes = uuid.as_bytes();
    if bytes.len() != 36 {
        return None;
    }
    let mut hex = [0u8; 32];
    let mut at = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return None;
            }
            continue;
        }
        hex[at] = hex_value(byte)?;
        at += 1;
    }
    let mut most = 0u64;
    let mut least = 0u64;
    for &digit in &hex[..16] {
        most = (most << 4) | u64::from(digit);
    }
    for &digit in &hex[16..] {
        least = (least << 4) | u64::from(digit);
    }
    Some((most, least))
}

/// The value of one ASCII hex digit; both letter cases count.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! The default-skin rule: the Java `UUID.hashCode` fold pinned by golden
    //! vectors, and the refuse cases that fall back to the wide model; the
    //! property decode: synthetic wire fixtures and the malformed shapes.

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;

    use super::{DefaultModel, SkinCache, SkinError, decode_profile_property, default_skin};
    use crate::http::{HttpClient, HttpError, UreqClient};
    use crate::store::Store;

    #[test]
    fn the_uuid_hash_bit_picks_the_default_model() {
        // `DefaultPlayerSkin.isSlimSkin` (`client/resources/DefaultPlayerSkin.java:41-44`)
        // hands out Alex (slim) when the lowest bit of the UUID's Java
        // `hashCode` is one and Steve (wide) otherwise. Java's `UUID.hashCode`
        // is the XOR of the two 32-bit halves of `mostSigBits ^ leastSigBits`,
        // so with `hilo` the two 64-bit halves' XOR the low bit is
        //
        //     bit0(hilo) ^ bit32(hilo)
        //
        // and every vector below writes those two bits out by hand.
        let vectors: [(&str, DefaultModel); 6] = [
            // Both halves zero: 0 ^ 0 -> wide.
            ("00000000-0000-0000-0000-000000000000", DefaultModel::Wide),
            // The low half's bit 0: 1 ^ 0 -> slim.
            ("00000000-0000-0000-0000-000000000001", DefaultModel::Slim),
            // The high half's bit 32 (0x0000_0001_0000_0000): 0 ^ 1 -> slim.
            ("00000001-0000-0000-0000-000000000000", DefaultModel::Slim),
            // Both legs set — bit 0 (0x0000_0000_0000_0001) and bit 32 of the
            // same half — cancel: 1 ^ 1 -> wide.
            ("00000001-0000-0001-0000-000000000000", DefaultModel::Wide),
            // An offline-mode-shaped UUID (version 3, variant 8): the halves
            // XOR to 0x8000_0000_0000_3001, so 0 ^ 1 -> slim.
            ("00000000-0000-3000-8000-000000000001", DefaultModel::Slim),
            // Uppercase hex letters parse like `UUID.fromString` reads them:
            // 0xB is odd -> slim.
            ("00000000-0000-0000-0000-00000000000B", DefaultModel::Slim),
        ];
        for (uuid, expected) in vectors {
            assert_eq!(default_skin(uuid), expected, "the uuid {uuid}");
        }
    }

    #[test]
    fn a_uuid_outside_the_canonical_shape_falls_back_to_the_wide_model() {
        // Anything the hand parser cannot read as 8-4-4-4-12 hex is refused
        // without a panic, and the caller falls back to the wide model — the
        // rule's own default when nothing can be computed.
        for uuid in [
            "",
            "1234",                                  // a short string
            "00000000-0000-0000-0000-00000000000g",  // a non-hex segment
            "0000000-0000-0000-0000-000000000000",   // a hyphen one short
            "00000000-0000-0000-0000-0000000000000", // one character long
            "00000000_0000_0000_0000_000000000000",  // underscores
        ] {
            assert_eq!(
                default_skin(uuid),
                DefaultModel::Wide,
                "the malformed uuid {uuid:?}"
            );
        }
    }

    /// The wire form of a property: the JSON text base64-encoded.
    fn property(json: &str) -> String {
        STANDARD.encode(json.as_bytes())
    }

    #[test]
    fn a_property_decodes_to_its_url_and_model() {
        // The base shape: a SKIN entry with a url, no metadata blocks and no
        // cape. The url's scheme is read out of the property, not assumed.
        let value = property(r#"{"textures":{"SKIN":{"url":"http://textures.example/abc123"}}}"#);
        let decoded = decode_profile_property(&value).expect("a well-formed property");
        assert_eq!(decoded.url, "http://textures.example/abc123");
        assert_eq!(
            decoded.model,
            DefaultModel::Wide,
            "no metadata: the text default"
        );
        assert_eq!(decoded.cape_url, None, "the property names no cape");
    }

    #[test]
    fn the_slim_model_rides_in_the_metadata() {
        let slim = property(
            r#"{"textures":{"SKIN":{"url":"https://textures.example/abc123","metadata":{"model":"slim"}}}}"#,
        );
        assert_eq!(
            decode_profile_property(&slim).expect("slim resolves").model,
            DefaultModel::Slim
        );

        // Anything that is not exactly `slim` is the wide model: the source
        // reads the one string it knows and falls through otherwise.
        let other = property(
            r#"{"textures":{"SKIN":{"url":"https://textures.example/abc123","metadata":{"model":"default"}}}}"#,
        );
        assert_eq!(
            decode_profile_property(&other)
                .expect("other models resolve")
                .model,
            DefaultModel::Wide
        );

        let not_a_string = property(
            r#"{"textures":{"SKIN":{"url":"https://textures.example/abc123","metadata":{"model":7}}}}"#,
        );
        assert_eq!(
            decode_profile_property(&not_a_string)
                .expect("a non-string model resolves")
                .model,
            DefaultModel::Wide
        );
    }

    #[test]
    fn a_cape_entry_rides_alongside_the_skin() {
        let value = property(
            r#"{"textures":{"SKIN":{"url":"https://textures.example/skin"},"CAPE":{"url":"https://textures.example/cape"}}}"#,
        );
        let decoded = decode_profile_property(&value).expect("a cape-bearing property");
        assert_eq!(decoded.url, "https://textures.example/skin");
        assert_eq!(
            decoded.cape_url.as_deref(),
            Some("https://textures.example/cape")
        );
    }

    #[test]
    fn a_cape_url_outside_the_shape_is_dropped_not_fatal() {
        // The cape is decoration: a malformed cape url loses the cape, never
        // the skin the property was carrying.
        let bad_scheme = property(
            r#"{"textures":{"SKIN":{"url":"https://textures.example/skin"},"CAPE":{"url":"ftp://textures.example/cape"}}}"#,
        );
        let decoded = decode_profile_property(&bad_scheme).expect("the skin survives");
        assert_eq!(decoded.url, "https://textures.example/skin");
        assert_eq!(decoded.cape_url, None, "the ftp cape url is dropped");

        let empty = property(
            r#"{"textures":{"SKIN":{"url":"https://textures.example/skin"},"CAPE":{"url":""}}}"#,
        );
        assert_eq!(
            decode_profile_property(&empty)
                .expect("an empty cape url is no cape")
                .cape_url,
            None
        );
    }

    #[test]
    fn malformed_properties_are_refused_without_panicking() {
        // Not base64 at all: the strict engine refuses the space and bang.
        assert!(matches!(
            decode_profile_property("not base64!!!"),
            Err(SkinError::Base64(_))
        ));
        // Base64 that is not a whole number of quads.
        assert!(matches!(
            decode_profile_property("eyJ0ZXh0dXJlcyI6"),
            Err(SkinError::Base64(_)) | Err(SkinError::Utf8(_)) | Err(SkinError::Json(_))
        ));

        // Base64 that decodes to bytes that are not UTF-8 text.
        let not_text = STANDARD.encode([0xff, 0xfe]);
        assert!(matches!(
            decode_profile_property(&not_text),
            Err(SkinError::Utf8(_))
        ));

        // UTF-8 that is not JSON.
        let not_json = property("this is not JSON");
        assert!(matches!(
            decode_profile_property(&not_json),
            Err(SkinError::Json(_))
        ));

        // JSON with no SKIN entry (and a non-object walk target).
        for json in [
            r#"{"textures":{"CAPE":{"url":"https://textures.example/cape"}}}"#,
            r#"{"textures":"nope"}"#,
            r#"[]"#,
            r#""a string""#,
        ] {
            assert!(
                matches!(
                    decode_profile_property(&property(json)),
                    Err(SkinError::MissingSkin)
                ),
                "the property {json:?}"
            );
        }

        // A SKIN entry that names no url (absent, or not a string).
        for json in [
            r#"{"textures":{"SKIN":{}}}"#,
            r#"{"textures":{"SKIN":{"url":7}}}"#,
            r#"{"textures":{"SKIN":"https://textures.example/skin"}}"#,
        ] {
            assert!(
                matches!(
                    decode_profile_property(&property(json)),
                    Err(SkinError::MissingSkinUrl)
                ),
                "the property {json:?}"
            );
        }

        // A url outside the http/https shape.
        let bad_scheme = property(r#"{"textures":{"SKIN":{"url":"ftp://textures.example/skin"}}}"#);
        assert!(matches!(
            decode_profile_property(&bad_scheme),
            Err(SkinError::BadUrl { .. })
        ));
    }

    #[test]
    fn an_overlong_skin_url_is_refused() {
        // The url cap is 512 bytes; 25 + 488 is over it while the URL is
        // still made of readable path bytes.
        let url = format!("https://textures.example/{}", "a".repeat(488));
        assert!(url.len() > 512);
        let value = property(&format!(r#"{{"textures":{{"SKIN":{{"url":"{url}"}}}}}}"#));
        assert!(matches!(
            decode_profile_property(&value),
            Err(SkinError::BadUrl { .. })
        ));
    }

    #[test]
    fn the_property_cap_admits_a_full_four_kib_and_refuses_a_byte_more() {
        // Exactly 4 KiB of JSON decodes (the cap is `<=`): the URL still comes
        // out of the padding.
        let mut json =
            String::from(r#"{"padding":"PAD","textures":{"SKIN":{"url":"http://x/a"}}}"#);
        let pad = 4096 - json.len() + 3;
        json = json.replace("PAD", &"a".repeat(pad));
        assert_eq!(json.len(), 4096);
        let decoded = decode_profile_property(&property(&json)).expect("the cap admits 4 KiB");
        assert_eq!(decoded.url, "http://x/a");

        // One byte more is refused before the JSON walk; a blob whose base64
        // form alone is over the encodable maximum never decodes at all.
        let mut over =
            String::from(r#"{"padding":"PAD","textures":{"SKIN":{"url":"http://x/a"}}}"#);
        let pad = 4097 - over.len() + 3;
        over = over.replace("PAD", &"a".repeat(pad));
        assert_eq!(over.len(), 4097);
        assert!(matches!(
            decode_profile_property(&property(&over)),
            Err(SkinError::PropertyTooLarge)
        ));
        assert!(matches!(
            decode_profile_property(&"A".repeat(6000)),
            Err(SkinError::PropertyTooLarge)
        ));
    }

    /// A real-shaped texture URL: the CDN's 64-hex hash form.
    const SKIN_URL: &str = "https://textures.minecraft.net/texture/\
                            0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// The hash [`SKIN_URL`] ends in, the cache file's stem.
    const SKIN_HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// An opened temp store and its skin cache; the temp directory must live
    /// as long as the cache.
    fn temp_cache() -> (tempfile::TempDir, SkinCache) {
        let dir = tempfile::tempdir().expect("a temp dir");
        let store = Store::open(dir.path().to_path_buf()).expect("the store opens");
        (dir, SkinCache::new(&store))
    }

    /// A synthetic RGBA PNG of the given size, built by the dependency's
    /// encoder.
    fn skin_png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("the fixture's header");
        let pixels = vec![0x7f; width as usize * height as usize * 4];
        writer
            .write_image_data(&pixels)
            .expect("the fixture's data");
        drop(writer);
        bytes
    }

    /// An HTTP client with no network at all: every request fails.
    struct NoNetwork;

    impl HttpClient for NoNetwork {
        fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
            Err(HttpError::Transport {
                url: url.to_owned(),
                message: "the test has no network".to_owned(),
            })
        }

        fn post_json(&self, url: &str, _body: &[u8]) -> Result<Vec<u8>, HttpError> {
            Err(HttpError::Transport {
                url: url.to_owned(),
                message: "the test has no network".to_owned(),
            })
        }

        fn post_form(&self, url: &str, _fields: &[(&str, &str)]) -> Result<Vec<u8>, HttpError> {
            Err(HttpError::Transport {
                url: url.to_owned(),
                message: "the test has no network".to_owned(),
            })
        }

        fn get_with_bearer(&self, url: &str, _bearer: &str) -> Result<Vec<u8>, HttpError> {
            Err(HttpError::Transport {
                url: url.to_owned(),
                message: "the test has no network".to_owned(),
            })
        }
    }

    /// An HTTP client serving one canned body for every request.
    struct OneBody(Vec<u8>);

    impl HttpClient for OneBody {
        fn get(&self, _url: &str) -> Result<Vec<u8>, HttpError> {
            Ok(self.0.clone())
        }

        fn post_json(&self, _url: &str, _body: &[u8]) -> Result<Vec<u8>, HttpError> {
            Ok(self.0.clone())
        }

        fn post_form(&self, _url: &str, _fields: &[(&str, &str)]) -> Result<Vec<u8>, HttpError> {
            Ok(self.0.clone())
        }

        fn get_with_bearer(&self, _url: &str, _bearer: &str) -> Result<Vec<u8>, HttpError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn a_skin_round_trips_through_the_cache() {
        let (dir, cache) = temp_cache();
        let png = skin_png(64, 64);
        cache.store(SKIN_URL, &png).expect("the write lands");

        let path = cache.path_for(SKIN_URL).expect("a 64-hex url names a path");
        assert!(
            path.starts_with(dir.path().join("skins")),
            "the file lives under skins/"
        );
        let expected = format!("{SKIN_HASH}.png");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(expected.as_str()),
            "the name is the url's own hash"
        );
        assert_eq!(
            std::fs::read(&path).expect("the cache file"),
            png,
            "the bytes as served"
        );

        // The same hash behind a query names the same file.
        let with_query = format!("{SKIN_URL}?time=1");
        assert_eq!(cache.path_for(&with_query), Some(path.clone()));

        let loaded = cache.load(SKIN_URL).expect("the cached skin loads");
        assert_eq!((loaded.width, loaded.height), (64, 64));
        assert_eq!(loaded.rgba.len(), 64 * 64 * 4);
        assert_eq!(
            cache.load("https://textures.minecraft.net/texture/dead"),
            None,
            "no such file"
        );
    }

    #[test]
    fn a_cached_skin_is_served_without_any_network() {
        let (_dir, cache) = temp_cache();
        cache
            .store(SKIN_URL, &skin_png(64, 64))
            .expect("the write lands");
        let texture = cache
            .fetch_with(&NoNetwork, SKIN_URL)
            .expect("a cache hit never touches the network");
        assert_eq!((texture.width, texture.height), (64, 64));
    }

    #[test]
    fn a_fetch_decodes_and_caches_the_served_bytes() {
        let (_dir, cache) = temp_cache();
        let png = skin_png(64, 64);
        let texture = cache
            .fetch_with(&OneBody(png.clone()), SKIN_URL)
            .expect("the served bytes decode");
        assert_eq!((texture.width, texture.height), (64, 64));

        let path = cache.path_for(SKIN_URL).expect("a path");
        assert_eq!(std::fs::read(&path).expect("the cache file"), png);

        // The second fetch is the disk hit: the network stub is off.
        let again = cache
            .fetch_with(&NoNetwork, SKIN_URL)
            .expect("the cached copy serves");
        assert_eq!(again.rgba, texture.rgba);
    }

    #[test]
    fn an_image_that_is_not_64x64_is_refused_with_its_dimensions() {
        let (_dir, cache) = temp_cache();
        let legacy = skin_png(64, 32);
        match cache.fetch_with(&OneBody(legacy.clone()), SKIN_URL) {
            Err(SkinError::WrongSize { width, height }) => {
                assert_eq!(
                    (width, height),
                    (64, 32),
                    "the refusal names the dimensions"
                );
            }
            other => panic!("a 64x32 image must be refused: {other:?}"),
        }
        assert!(
            cache.load(SKIN_URL).is_none(),
            "the refused bytes are not cached"
        );
        assert!(!cache.path_for(SKIN_URL).expect("a path").exists());

        // A 64x32 file that reached the directory anyway is a miss, not a draw.
        cache
            .store(SKIN_URL, &legacy)
            .expect("the raw write succeeds");
        assert!(
            cache.load(SKIN_URL).is_none(),
            "load never serves a legacy size"
        );
    }

    #[test]
    fn bytes_that_are_not_a_png_are_refused_and_not_cached() {
        let (_dir, cache) = temp_cache();
        let error = cache
            .fetch_with(&OneBody(b"this is not a PNG file".to_vec()), SKIN_URL)
            .expect_err("bytes without the signature cannot decode");
        assert!(matches!(error, SkinError::Texture(_)));
        assert!(cache.load(SKIN_URL).is_none());
        assert!(!cache.path_for(SKIN_URL).expect("a path").exists());
    }

    #[test]
    fn a_shaped_wrong_url_names_no_cache_file() {
        let (_dir, cache) = temp_cache();
        for url in [
            "ftp://textures.minecraft.net/texture/abc", // not http(s)
            "http://textures.minecraft.net/",           // no segment
            "http://textures.minecraft.net",            // no path at all
            "http://textures.minecraft.net/skins/steve.png", // a dot, not a hash
            "http://textures.minecraft.net/texture/abc!def", // an invalid byte
            "http://textures.minecraft.net/texture/../evil", // would climb
        ] {
            assert!(cache.path_for(url).is_none(), "the url {url:?}");
        }

        // Over the URL cap.
        let long = format!("https://textures.minecraft.net/texture/{}", "a".repeat(480));
        assert!(long.len() > 512);
        assert!(cache.path_for(&long).is_none());

        // Over the hash cap.
        let long_hash = format!("https://textures.minecraft.net/texture/{}", "a".repeat(65));
        assert!(cache.path_for(&long_hash).is_none());

        // The cache operations name the two refusals; neither reaches the
        // network nor writes.
        assert!(matches!(
            cache.fetch("ftp://textures.minecraft.net/texture/abc"),
            Err(SkinError::BadUrl { .. })
        ));
        assert!(matches!(
            cache.store("http://textures.minecraft.net/skins/steve.png", b"x"),
            Err(SkinError::BadCacheName { .. })
        ));

        // A hash segment one character shorter than the cap is fine.
        let short = format!("https://textures.minecraft.net/texture/{}", "a".repeat(64));
        assert!(cache.path_for(&short).is_some());
    }

    /// An HTTP client with one scripted answer and a shared call log: the
    /// URLs every `get` saw, in order.
    struct ScriptedProfile {
        /// `None` answers the body below; `Some(code)` answers a status
        /// error with an empty body.
        status: Option<u16>,
        /// The 2xx body.
        body: Vec<u8>,
        /// Every URL `get` was called with, in order.
        calls: std::sync::Mutex<Vec<String>>,
    }

    impl ScriptedProfile {
        /// Answers `body` with a 2xx.
        fn ok(body: Vec<u8>) -> Self {
            Self {
                status: None,
                body,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        /// Answers a status error with an empty body.
        fn status(code: u16) -> Self {
            Self {
                status: Some(code),
                body: Vec::new(),
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        /// The URLs fetched so far, in order.
        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("the call log").clone()
        }
    }

    impl HttpClient for ScriptedProfile {
        fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
            self.calls
                .lock()
                .expect("the call log")
                .push(url.to_owned());
            match self.status {
                None => Ok(self.body.clone()),
                Some(code) => Err(HttpError::Status {
                    url: url.to_owned(),
                    code,
                    body: Vec::new(),
                    correlation_id: None,
                }),
            }
        }

        fn post_json(&self, url: &str, _body: &[u8]) -> Result<Vec<u8>, HttpError> {
            panic!("only get is scripted, not post_json for {url:?}")
        }

        fn post_form(&self, url: &str, _fields: &[(&str, &str)]) -> Result<Vec<u8>, HttpError> {
            panic!("only get is scripted, not post_form for {url:?}")
        }

        fn get_with_bearer(&self, url: &str, _bearer: &str) -> Result<Vec<u8>, HttpError> {
            panic!("only get is scripted, not get_with_bearer for {url:?}")
        }
    }

    /// The survey §3.6 profile body, verbatim but for one invented extra
    /// field: the truncated textures payload, `profileActions`, and an
    /// unknown field the walk must tolerate.
    fn survey_profile_body() -> Vec<u8> {
        br#"{
  "id" : "069a79f444e94726a5befca90e38aaf5",
  "name" : "Notch",
  "properties" : [ {
    "name" : "textures",
    "value" : "ewogICJ0aW1lc3RhbXAiIDogMTc5MDExMDQwNjUyMSwKICAicHJvZmlsZUlkIiA6ICIwNjlhNzlmNDQ0ZTk0NzI2YTViZWZjYTkwZTM4YWFmNSIsCiAgInByb2ZpbGVOYW1lIiA6ICJOb3RjaCIsCiAgInRleHR1cmVzIiA6IHsKICAgICJTS0lOIiA6IHsKICAgICAgInVybCIgOiAiaHR0cDovL3RleHR1cmVzLm1pbmVjcmFmdC5uZXQvdGV4dHVyZS8yOTIwMDlhNDkyNWI1OGYwMmM3N2RhZGMzZWNlZjA3ZWE0Yzc0NzJmNjRlMGZkYzMyY2U1NTIyNDg5MzYyNjgwIgogICAgfQogIH0KfQ=="
  } ],
  "profileActions" : [ ],
  "inventedFutureField" : { "nested" : [1, 2, 3] }
}"#
        .to_vec()
    }

    #[test]
    fn the_survey_profile_parses_to_its_skin() {
        // The §3.6 verbatim shape: one textures entry whose payload names
        // the SKIN url, beside `profileActions` and an invented future
        // field the walk ignores rather than rejects.
        let http = ScriptedProfile::ok(survey_profile_body());
        let textures = super::fetch_profile_textures(&http, "069a79f444e94726a5befca90e38aaf5")
            .expect("the verbatim profile parses");
        assert_eq!(textures.len(), 1, "the one textures entry");
        assert_eq!(
            textures[0].url,
            "http://textures.minecraft.net/texture/292009a4925b58f02c77dadc3ecef07ea4c7472f64e0fdc32ce5522489362680"
        );
        assert_eq!(
            textures[0].model,
            DefaultModel::Wide,
            "no metadata: the text default"
        );
        assert_eq!(textures[0].cape_url, None, "the payload names no cape");
        assert_eq!(
            http.calls(),
            vec![
                "https://sessionserver.mojang.com/session/minecraft/profile/069a79f444e94726a5befca90e38aaf5"
                    .to_owned()
            ],
            "one GET at the profile endpoint"
        );
    }

    #[test]
    fn a_malformed_textures_value_is_a_typed_error() {
        // The endpoint answered, but its textures entry is not base64: the
        // decode's own refusal travels out, typed.
        let body = br#"{"properties":[{"name":"textures","value":"!!! not base64 !!!"}]}"#;
        let http = ScriptedProfile::ok(body.to_vec());
        let error = super::fetch_profile_textures(&http, "069a79f444e94726a5befca90e38aaf5")
            .expect_err("a malformed textures entry is refused");
        assert!(
            matches!(error, SkinError::Base64(_)),
            "the typed refusal, not a bare fetch error: {error:?}"
        );
        assert_eq!(http.calls().len(), 1, "the endpoint was called once");
    }

    #[test]
    fn an_account_without_textures_is_an_empty_list() {
        // Neither code is documented for this endpoint in the survey: a
        // body-less 204 and a 404 both mean the account carries no textures.
        let empty = ScriptedProfile::ok(Vec::new());
        assert_eq!(
            super::fetch_profile_textures(&empty, "069a79f444e94726a5befca90e38aaf5")
                .expect("a body-less 204 is an empty list"),
            Vec::new()
        );
        assert_eq!(empty.calls().len(), 1);

        let missing = ScriptedProfile::status(404);
        assert_eq!(
            super::fetch_profile_textures(&missing, "069a79f444e94726a5befca90e38aaf5")
                .expect("a 404 is an empty list"),
            Vec::new()
        );
        assert_eq!(missing.calls().len(), 1);
    }

    #[test]
    fn other_failures_travel_out_typed() {
        // A transport failure is no textures list: the HTTP error travels.
        let error = super::fetch_profile_textures(&NoNetwork, "069a79f444e94726a5befca90e38aaf5")
            .expect_err("a transport failure is an error");
        assert!(matches!(error, SkinError::Http(_)), "{error:?}");

        // A refusal that is not a 404 stays a refusal.
        let forbidden = ScriptedProfile::status(403);
        let error = super::fetch_profile_textures(&forbidden, "069a79f444e94726a5befca90e38aaf5")
            .expect_err("a 403 is an error");
        assert!(matches!(error, SkinError::Http(_)), "{error:?}");
    }

    #[test]
    fn an_overlong_profile_body_is_refused_before_parse() {
        // The body cap sits before the JSON walk: a hostile body is a typed
        // refusal, never a parse allocation.
        let big = vec![b'x'; super::MAX_PROFILE_BODY_BYTES + 1];
        let http = ScriptedProfile::ok(big);
        let error = super::fetch_profile_textures(&http, "069a79f444e94726a5befca90e38aaf5")
            .expect_err("an overlong body is refused");
        assert!(matches!(error, SkinError::PropertyTooLarge), "{error:?}");
    }

    /// The live profile case (Task 13's leg): the public endpoint's answer
    /// for a known uuid, fetched for real and cached through the store. Run
    /// with `OXIDECRAFT_STORE` set and `-- --ignored`.
    #[test]
    #[ignore = "hits the live profile endpoint; set OXIDECRAFT_STORE and run with -- --ignored"]
    fn the_live_profile_endpoint_answers_and_caches() {
        let root = std::env::var_os("OXIDECRAFT_STORE")
            .filter(|root| !root.is_empty())
            .expect("OXIDECRAFT_STORE must name the store root");
        let store = Store::open(std::path::PathBuf::from(root)).expect("the store opens");
        let cache = SkinCache::new(&store);
        let http = UreqClient::new();
        let textures = super::fetch_profile_textures(&http, "069a79f444e94726a5befca90e38aaf5")
            .expect("the live endpoint answers");
        assert!(!textures.is_empty(), "the known account carries textures");
        let skin = &textures[0];
        assert!(
            skin.url
                .starts_with("https://textures.minecraft.net/texture/"),
            "the skin url names the CDN: {}",
            skin.url
        );
        let texture = cache.fetch(&skin.url).expect("the skin caches");
        assert!(!texture.rgba.is_empty(), "the skin decodes");
    }
}
