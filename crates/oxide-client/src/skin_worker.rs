//! The skin worker: decodes profile properties and fetches skins and capes
//! off the window thread.
//!
//! The window never waits on a texture. It forwards one request per
//! player-list entry that carries a `textures` property and drains the
//! updates into its own map, while this module's thread owns the decode and
//! the fetches behind it. A request is processed once per (uuid, property)
//! pair — the player list reports churn, and the same report must not refetch
//! — and a failed fetch is final for its entry: one update carrying `None`,
//! never a retry. The fetcher is a closure, so the client hands in
//! `SkinCache::fetch` and the tests drive a stub.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, unbounded};
use oxide_assets::skins::{DefaultModel, SkinError, decode_profile_property, default_skin};
use oxide_assets::texture::Texture;

/// One player's skin request: the profile's uuid and its `textures` property
/// value as the wire carried it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinRequest {
    /// The profile's hyphenated UUID, the dedupe key's first half.
    pub uuid: String,
    /// The base64 `textures` property, when the entry carried one.
    pub property: Option<String>,
}

/// One player's resolved skin: the fetched textures, or `None` where the
/// defaults stand in.
///
/// A `None` texture is the default skin — [`default_skin`]'s model picks which
/// — and a `None` cape is no cape; both are final for the request that
/// produced them.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinUpdate {
    /// The profile's hyphenated UUID, the map key the client files it under.
    pub uuid: String,
    /// The fetched skin; `None` means the default skin.
    pub texture: Option<Arc<Texture>>,
    /// The fetched cape, when the property named one and it arrived.
    pub cape: Option<Arc<Texture>>,
    /// The model to draw: the property's own when its skin arrived, or the
    /// UUID default otherwise — no usable property, or a failed fetch.
    pub model: DefaultModel,
}

/// Spawns the worker on its own thread and returns the channel pair: requests
/// in, updates out.
///
/// The thread owns the fetcher and ends when every request sender has dropped
/// — the client drops its sender when the session ends, and the update channel
/// then disconnects with the thread.
pub fn spawn<F>(fetch: F) -> (Sender<SkinRequest>, Receiver<SkinUpdate>)
where
    F: Fn(&str) -> Result<Arc<Texture>, SkinError> + Send + 'static,
{
    let (requests_tx, requests_rx) = unbounded();
    let (updates_tx, updates_rx) = unbounded();
    std::thread::spawn(move || run(fetch, requests_rx, updates_tx));
    (requests_tx, updates_rx)
}

/// The worker loop: processes requests until the channel closes.
///
/// One update goes out per processed request, in request order. A request is
/// processed once per (uuid, property) pair — a repeat of the last processed
/// pair for that uuid is skipped, so the player list's churn never refetches —
/// and the pair is recorded before the decode, so even a malformed property
/// and a failed fetch are final for that pair.
pub fn run<F>(fetch: F, requests: Receiver<SkinRequest>, updates: Sender<SkinUpdate>)
where
    F: Fn(&str) -> Result<Arc<Texture>, SkinError>,
{
    let mut processed: HashMap<String, u64> = HashMap::new();
    for request in requests.iter() {
        let hash = property_hash(request.property.as_deref());
        if processed.insert(request.uuid.clone(), hash) == Some(hash) {
            continue;
        }
        let update = match request
            .property
            .as_deref()
            .and_then(|value| decode_profile_property(value).ok())
        {
            Some(profile) => {
                let texture = match fetch(&profile.url) {
                    Ok(texture) => Some(texture),
                    Err(error) => {
                        tracing::debug!(%error, url = %profile.url, "the skin fetch failed");
                        None
                    }
                };
                let cape = match profile.cape_url.as_deref() {
                    Some(url) => match fetch(url) {
                        Ok(cape) => Some(cape),
                        Err(error) => {
                            tracing::debug!(%error, url, "the cape fetch failed");
                            None
                        }
                    },
                    None => None,
                };
                // A failed fetch leaves no skin to draw, so the model falls
                // back to the uuid's default; a fetched skin keeps the
                // property's own.
                let model = if texture.is_some() {
                    profile.model
                } else {
                    default_skin(&request.uuid)
                };
                SkinUpdate {
                    uuid: request.uuid.clone(),
                    texture,
                    cape,
                    model,
                }
            }
            // No property, or one the decode refused: the default model for
            // this uuid and nothing to fetch.
            None => SkinUpdate {
                uuid: request.uuid.clone(),
                texture: None,
                cape: None,
                model: default_skin(&request.uuid),
            },
        };
        // A window that stopped listening is not an error here.
        let _ = updates.send(update);
    }
}

/// The dedupe hash of a request's property value.
///
/// Equality of the hash stands in for equality of the property: the same
/// report twice hashes the same, a changed property does not, and nothing
/// leaves the process.
fn property_hash(property: Option<&str>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    property.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    //! The worker's loop against a stub fetcher: the dedupe, the one-shot
    //! failures, the cape's second fetch, and the close that ends the thread.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crossbeam_channel::{RecvTimeoutError, unbounded};
    use oxide_assets::http::HttpError;
    use oxide_assets::skins::{DefaultModel, SkinError};
    use oxide_assets::texture::Texture;

    use super::{SkinRequest, SkinUpdate, run, spawn};

    /// The rule's wide vector: the all-zero uuid is the Steve model.
    const UUID_WIDE: &str = "00000000-0000-0000-0000-000000000000";

    /// The rule's slim vector: the low bit of the hash is one, the Alex model.
    const UUID_SLIM: &str = "00000000-0000-0000-0000-000000000001";

    /// The fixture skin URLs. Nothing fetches them outside the stub.
    const SKIN_URL: &str = "https://textures.example/skin";
    const SKIN_URL_B: &str = "https://textures.example/skin-b";
    const CAPE_URL: &str = "https://textures.example/cape";

    /// A synthetic texture: 64x64 with one distinctive byte, so an update's
    /// skin and its cape can be told apart.
    fn texture(fill: u8) -> Texture {
        Texture {
            width: 64,
            height: 64,
            rgba: vec![fill; 64 * 64 * 4],
        }
    }

    /// The wire form of a property: the JSON text, base64-encoded by this
    /// file's own encoder.
    ///
    /// The encoder shares nothing with the decoder under test — the same rule
    /// the texture tests' hand-built PNG follows — so the fixtures enter the
    /// worker through the wire shape a server would send.
    fn property(json: &str) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes = json.as_bytes();
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let n = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[((n >> 6) & 0x3f) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[(n & 0x3f) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    /// The property naming one skin URL, no metadata and no cape.
    fn skin_property(url: &str) -> String {
        property(&format!(r#"{{"textures":{{"SKIN":{{"url":"{url}"}}}}}}"#))
    }

    /// The property naming the skin at [`SKIN_URL`] and its cape.
    fn cape_property() -> String {
        property(&format!(
            r#"{{"textures":{{"SKIN":{{"url":"{SKIN_URL}"}},"CAPE":{{"url":"{CAPE_URL}"}}}}}}"#
        ))
    }

    /// The property naming the skin at [`SKIN_URL`] with a cape outside the
    /// http(s) shape the decode accepts.
    fn odd_cape_property() -> String {
        property(&format!(
            r#"{{"textures":{{"SKIN":{{"url":"{SKIN_URL}"}},"CAPE":{{"url":"ftp://textures.example/cape"}}}}}}"#
        ))
    }

    /// The property asking for the slim model.
    fn slim_property() -> String {
        property(&format!(
            r#"{{"textures":{{"SKIN":{{"url":"{SKIN_URL}","metadata":{{"model":"slim"}}}}}}}}"#
        ))
    }

    /// A stub fetcher: one canned answer per URL, and a shared call log.
    #[derive(Clone)]
    struct Stub {
        /// What each URL answers; `None` answers a transport failure.
        answers: HashMap<&'static str, Option<Texture>>,
        /// Every URL the fetcher was called with, in order.
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl Stub {
        /// A stub answering the given URLs.
        fn new(answers: &[(&'static str, Option<Texture>)]) -> Self {
            Self {
                answers: answers
                    .iter()
                    .map(|(url, texture)| (*url, texture.clone()))
                    .collect(),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        /// The fetcher: log the URL, then answer what the map holds. A URL the
        /// map does not name is a test that described the wrong fetch.
        fn fetch(&self, url: &str) -> Result<Arc<Texture>, SkinError> {
            self.calls
                .lock()
                .expect("the call log")
                .push(url.to_owned());
            match self.answers.get(url) {
                Some(Some(texture)) => Ok(Arc::new(texture.clone())),
                Some(None) => Err(SkinError::Http(HttpError::Transport {
                    url: url.to_owned(),
                    message: "the stub refuses".to_owned(),
                })),
                None => panic!("the stub has no answer for {url:?}"),
            }
        }

        /// The URLs fetched so far, in order.
        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("the call log").clone()
        }
    }

    /// Runs the loop over the given requests on this thread and returns the
    /// updates it produced.
    ///
    /// The request channel closes behind them, so the call returning is the
    /// loop's own exit on the closed channel.
    fn settle(stub: &Stub, requests: &[SkinRequest]) -> Vec<SkinUpdate> {
        let (request_tx, request_rx) = unbounded();
        let (update_tx, update_rx) = unbounded();
        for request in requests {
            request_tx
                .send(request.clone())
                .expect("the channel is open");
        }
        drop(request_tx);
        run(|url| stub.fetch(url), request_rx, update_tx);
        update_rx.try_iter().collect()
    }

    #[test]
    fn repeated_requests_dedupe_by_uuid_and_property() {
        let stub = Stub::new(&[
            (SKIN_URL, Some(texture(0x11))),
            (SKIN_URL_B, Some(texture(0x22))),
        ]);
        let repeated = SkinRequest {
            uuid: UUID_WIDE.to_owned(),
            property: Some(skin_property(SKIN_URL)),
        };
        let changed = SkinRequest {
            uuid: UUID_WIDE.to_owned(),
            property: Some(skin_property(SKIN_URL_B)),
        };
        let other = SkinRequest {
            uuid: UUID_SLIM.to_owned(),
            property: Some(skin_property(SKIN_URL)),
        };
        let updates = settle(
            &stub,
            &[repeated.clone(), repeated.clone(), other, repeated, changed],
        );
        // The repeats of one request collapse; the other uuid and the changed
        // property each get their own fetch and update.
        let uuids: Vec<&str> = updates.iter().map(|update| update.uuid.as_str()).collect();
        assert_eq!(uuids, vec![UUID_WIDE, UUID_SLIM, UUID_WIDE]);
        assert_eq!(stub.calls(), vec![SKIN_URL, SKIN_URL, SKIN_URL_B]);
    }

    #[test]
    fn a_failed_fetch_is_one_none_update_with_no_retry() {
        let stub = Stub::new(&[(SKIN_URL, None)]);
        let request = SkinRequest {
            uuid: UUID_WIDE.to_owned(),
            property: Some(skin_property(SKIN_URL)),
        };
        let updates = settle(&stub, &[request.clone(), request]);
        assert_eq!(updates.len(), 1, "the churned repeat did not fetch again");
        let update = &updates[0];
        assert_eq!(update.uuid, UUID_WIDE);
        assert!(update.texture.is_none());
        assert!(update.cape.is_none());
        assert_eq!(
            update.model,
            DefaultModel::Wide,
            "the fetch failed: the uuid's default"
        );
        assert_eq!(stub.calls(), vec![SKIN_URL], "one fetch, no retry");
    }

    #[test]
    fn a_failed_fetch_takes_the_uuid_default_model() {
        // The metadata asks for slim, the uuid rule says wide: the two
        // disagree, so this vector can tell which model a failed fetch
        // leaves. With no fetched skin to draw, the update carries the
        // uuid's default.
        let stub = Stub::new(&[(SKIN_URL, None)]);
        let updates = settle(
            &stub,
            &[SkinRequest {
                uuid: UUID_WIDE.to_owned(),
                property: Some(slim_property()),
            }],
        );
        assert_eq!(updates.len(), 1);
        let update = &updates[0];
        assert!(update.texture.is_none(), "the fetch failed");
        assert_eq!(
            update.model,
            DefaultModel::Wide,
            "the uuid's default, not the property's model"
        );
    }

    #[test]
    fn a_cape_bearing_request_fetches_both_urls() {
        let stub = Stub::new(&[
            (SKIN_URL, Some(texture(0x11))),
            (CAPE_URL, Some(texture(0x22))),
        ]);
        let updates = settle(
            &stub,
            &[SkinRequest {
                uuid: UUID_WIDE.to_owned(),
                property: Some(cape_property()),
            }],
        );
        assert_eq!(
            stub.calls(),
            vec![SKIN_URL, CAPE_URL],
            "the skin, then the cape"
        );
        let update = &updates[0];
        assert_eq!(
            update.texture.as_ref().map(|texture| texture.rgba[0]),
            Some(0x11)
        );
        assert_eq!(
            update.cape.as_ref().map(|texture| texture.rgba[0]),
            Some(0x22)
        );
    }

    #[test]
    fn a_failing_cape_still_reports_the_skin() {
        let stub = Stub::new(&[(SKIN_URL, Some(texture(0x11))), (CAPE_URL, None)]);
        let updates = settle(
            &stub,
            &[SkinRequest {
                uuid: UUID_WIDE.to_owned(),
                property: Some(cape_property()),
            }],
        );
        assert_eq!(stub.calls(), vec![SKIN_URL, CAPE_URL]);
        assert!(updates[0].texture.is_some());
        assert!(
            updates[0].cape.is_none(),
            "the failed cape is None, not a retry"
        );
    }

    #[test]
    fn a_request_with_no_usable_cape_fetches_no_cape() {
        let stub = Stub::new(&[(SKIN_URL, Some(texture(0x11)))]);
        // No cape entry at all, and one whose URL the decode drops: neither
        // may produce a third fetch.
        let updates = settle(
            &stub,
            &[
                SkinRequest {
                    uuid: UUID_WIDE.to_owned(),
                    property: Some(skin_property(SKIN_URL)),
                },
                SkinRequest {
                    uuid: UUID_SLIM.to_owned(),
                    property: Some(odd_cape_property()),
                },
            ],
        );
        assert!(updates[0].cape.is_none());
        assert!(updates[1].cape.is_none());
        assert_eq!(
            stub.calls(),
            vec![SKIN_URL, SKIN_URL],
            "no cape was fetched"
        );
    }

    #[test]
    fn a_request_without_a_usable_property_takes_the_uuid_default() {
        let stub = Stub::new(&[]);
        let updates = settle(
            &stub,
            &[
                SkinRequest {
                    uuid: UUID_WIDE.to_owned(),
                    property: None,
                },
                SkinRequest {
                    uuid: UUID_SLIM.to_owned(),
                    property: Some("not base64!!!".to_owned()),
                },
            ],
        );
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].model, DefaultModel::Wide, "the all-zero uuid");
        assert_eq!(updates[1].model, DefaultModel::Slim, "the low bit is one");
        assert!(
            updates
                .iter()
                .all(|update| update.texture.is_none() && update.cape.is_none())
        );
        assert!(stub.calls().is_empty(), "nothing was fetchable");
    }

    #[test]
    fn the_property_model_rides_the_update() {
        let stub = Stub::new(&[(SKIN_URL, Some(texture(0x11)))]);
        let updates = settle(
            &stub,
            &[SkinRequest {
                uuid: UUID_WIDE.to_owned(),
                property: Some(slim_property()),
            }],
        );
        assert_eq!(
            updates[0].model,
            DefaultModel::Slim,
            "the metadata's own model wins over the uuid"
        );
    }

    #[test]
    fn the_spawned_worker_reports_and_ends_on_close() {
        let stub = Stub::new(&[(SKIN_URL, Some(texture(0x11)))]);
        let worker_stub = stub.clone();
        let (requests, updates) = spawn(move |url| worker_stub.fetch(url));
        requests
            .send(SkinRequest {
                uuid: UUID_WIDE.to_owned(),
                property: Some(skin_property(SKIN_URL)),
            })
            .expect("the worker is up");
        let update = updates
            .recv_timeout(Duration::from_secs(10))
            .expect("one update");
        assert_eq!(update.uuid, UUID_WIDE);
        assert_eq!(stub.calls(), vec![SKIN_URL]);
        drop(requests);
        assert!(
            matches!(
                updates.recv_timeout(Duration::from_secs(10)),
                Err(RecvTimeoutError::Disconnected)
            ),
            "the loop returned and the update sender dropped"
        );
    }
}
