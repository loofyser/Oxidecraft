//! The language loader: one `.lang` file's parse rules, the en_US base plus
//! the hashed locale overlay, and the available-locale list.
//!
//! Values and names from the reference tree (never copied): the parse rules
//! are `Locale.loadLocaleData` (`client/resources/Locale.java`:100-116 —
//! skip empty and `#`-first lines, split on `=` limit 2, require exactly two
//! parts, rewrite the numeric format patterns to `%s`), the missing-key
//! fallback is the key itself (`translateKeyPrivate`:121-125), the unicode
//! law is more than 10% of value chars at or above U+0100 (`checkUnicode`
//! :56-78), and the load order is en_US first with the current language over
//! it (`LanguageManager.onResourceManagerReload`:63-74).
//!
//! The tree's `assets/minecraft/lang/en_US.lang` (below the extraction root)
//! is the base; the `minecraft/lang/<code>.lang` objects the store's asset
//! index names merge over it through [`Store::object_path`] — never a
//! store-side lang path. The default pack ships no `en_US` object (the 1.8
//! index names 148 other locales), so the base is tree-only in practice.

use std::collections::HashMap;

use crate::asset_index::AssetIndex;
use crate::extract::Extractor;
use crate::store::{Store, StoreError};

/// The most lines one `.lang` file contributes: further lines are refused
/// (the parse keeps what it read). Far above the 2679 the tree's en_US
/// carries, far below anything that threatens the heap.
pub const MAX_LANG_LINES: usize = 65_536;
/// The most bytes one key keeps: longer keys truncate. Above every key the
/// tree's en_US carries.
/// Hostile-input discipline: truncate the long key/value, refuse lines past
/// the line cap — one hostile line never costs the whole file.
pub const MAX_LANG_KEY_LEN: usize = 512;
/// The most bytes one value keeps: longer values truncate.
pub const MAX_LANG_VALUE_LEN: usize = 65_536;

/// The index key prefix every locale object shares.
const LANG_OBJECT_PREFIX: &str = "minecraft/lang/";
/// The tree path of the base file below the extraction root.
const TREE_LANG_FILE: &str = "assets/minecraft/lang/en_US.lang";
/// The base locale's code.
pub const BASE_LOCALE: &str = "en_US";

/// Errors from language loads.
#[derive(Debug, thiserror::Error)]
pub enum LangError {
    /// A filesystem failure.
    #[error("filesystem error at {path}: {source}")]
    Io {
        /// The path involved.
        path: std::path::PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A lang file's bytes are not valid UTF-8.
    #[error("the language object {what} is not valid UTF-8")]
    Utf8 {
        /// Which file failed.
        what: String,
    },
    /// The version document or the asset index would not parse.
    #[error("the language index would not parse: {0}")]
    Index(String),
    /// A store path derivation failed.
    #[error("the language store path failed: {0}")]
    Store(#[from] StoreError),
}

/// Parses one `.lang` file's text the way the source's own load does: skip
/// empty lines and `#`-first lines, split each remaining line on the first
/// `=` (the source's limit-2 split), keep only lines with both halves, and
/// run the numeric-placeholder rewrite over the value. Later lines win, as
/// the source's map put does.
pub fn parse_lang(text: &str) -> HashMap<String, String> {
    let mut entries = HashMap::new();
    for line in text.lines().take(MAX_LANG_LINES) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = truncate(key, MAX_LANG_KEY_LEN);
        let value = rewrite_placeholders(&truncate(value, MAX_LANG_VALUE_LEN));
        entries.insert(key, value);
    }
    entries
}

/// Keeps at most `max` bytes of `text`, cut at a UTF-8 boundary.
fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// The source's numeric-placeholder rewrite (`Locale.java`:111, pattern
/// `%(\\d+\\$)?[\\d\\.]*[df]` → `%$1s`): `%d`, `%f`, `%5d`, `%.2f` and
/// `%1$d` all read `%s` (indexed stays indexed). Anything else — `%s`,
/// `%%`, a trailing `%` — passes through untouched.
fn rewrite_placeholders(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            // ASCII-fast path; multibyte chars rejoin below.
            if bytes[i] < 0x80 {
                out.push(bytes[i] as char);
                i += 1;
            } else {
                let rest = &value[i..];
                let ch = rest.chars().next().unwrap_or('\u{FFFD}');
                out.push(ch);
                i += ch.len_utf8();
            }
            continue;
        }
        match match_numeric_spec(value, i + 1) {
            Some((indexed, end)) => {
                out.push('%');
                out.push_str(indexed);
                out.push('s');
                i = end;
            }
            None => {
                out.push('%');
                i += 1;
            }
        }
    }
    out
}

/// Matches the source's numeric pattern at `value[start..]` (just past the
/// `%`): the optional `digits$` group, then digits/dots, then `d` or `f`.
/// Answers the group text and the end offset on a match.
fn match_numeric_spec(value: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = value.as_bytes();
    let mut i = start;
    let mut indexed = "";
    // The optional `digits$` group.
    let mut j = i;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    if j > i && j < bytes.len() && bytes[j] == b'$' {
        indexed = &value[i..=j];
        i = j + 1;
    }
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        i += 1;
    }
    if i < bytes.len() && (bytes[i] == b'd' || bytes[i] == b'f') {
        Some((indexed, i + 1))
    } else {
        None
    }
}

/// The source's unicode law (`checkUnicode`:56-78) over the given values:
/// true when more than 10% of all value chars sit at or above U+0100. An
/// empty set reads false (the source's 0/0 NaN fails its own comparison).
pub fn values_are_unicode<'a>(values: impl IntoIterator<Item = &'a str>) -> bool {
    let mut wide = 0u64;
    let mut total = 0u64;
    for value in values {
        for ch in value.chars() {
            total += 1;
            if ch >= '\u{100}' {
                wide += 1;
            }
        }
    }
    total > 0 && wide as f64 / total as f64 > 0.1
}

/// One locale the loader can list: the code the settings store persists
/// plus the name and region its own file carries (`Language.java`:28-31
/// displays them as `name (region)`; the list sorts by code,
/// `Language.compareTo`:43-46).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageInfo {
    /// The locale code (`en_US`).
    pub code: String,
    /// The locale's own name (`English`).
    pub name: String,
    /// The locale's own region (`US`).
    pub region: String,
}

impl LanguageInfo {
    /// The list row: `name (region)`.
    pub fn display(&self) -> String {
        format!("{} ({})", self.name, self.region)
    }
}

/// The loaded language table: translation keys to template strings with
/// the key passthrough (`translateKeyPrivate`: a missing key renders as
/// the raw key).
#[derive(Debug, Clone, Default)]
pub struct Lang {
    entries: HashMap<String, String>,
    unicode: bool,
}

impl Lang {
    /// Parses one `.lang` text into a table.
    pub fn parse(text: &str) -> Self {
        let entries = parse_lang(text);
        let unicode = values_are_unicode(entries.values().map(String::as_str));
        Self { entries, unicode }
    }

    /// Merges one more `.lang` text over the table, in source order (the
    /// current language over en_US): later lines win per key.
    pub fn merge(&mut self, text: &str) {
        for (key, value) in parse_lang(text) {
            self.entries.insert(key, value);
        }
        self.unicode = values_are_unicode(self.entries.values().map(String::as_str));
    }

    /// The template a key resolves to, or the caller's key when the table
    /// does not hold it. The output borrows from whichever side survives:
    /// the table's value for a hit, the caller's key otherwise.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        match self.entries.get(key) {
            Some(value) => value.as_str(),
            None => key,
        }
    }

    /// Whether the loaded values read as a unicode locale (the >10% law).
    pub fn is_unicode(&self) -> bool {
        self.unicode
    }

    /// The table's entries, by key.
    pub fn into_entries(self) -> HashMap<String, String> {
        self.entries
    }

    /// The number of keys the table holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table holds no keys.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The table's own locale entry: `language.name`, `language.region`
    /// and `language.code` as the file carries them, falling back to the
    /// base code with the code as its own name.
    pub fn language_info(&self, code: &str) -> LanguageInfo {
        let name = self
            .entries
            .get("language.name")
            .cloned()
            .unwrap_or_else(|| code.to_string());
        let region = self
            .entries
            .get("language.region")
            .cloned()
            .unwrap_or_else(|| code.to_string());
        LanguageInfo {
            code: code.to_string(),
            name,
            region,
        }
    }

    /// Loads the table: the extraction tree's `en_US.lang` first, then the
    /// requested language's hashed locale object over it (source order).
    /// The index comes from the version document's own asset-index id, read
    /// through [`Store::object_path`]. A missing index or a missing object
    /// reads as tree-only — only `en_US` ships in the tree in practice —
    /// while a missing tree base or unreadable bytes fail.
    pub fn load(store: &Store, version: &str, language: &str) -> Result<Self, LangError> {
        let tree = Extractor::new(store, version).root();
        let path = tree.join(TREE_LANG_FILE);
        let base = std::fs::read_to_string(&path).map_err(|source| LangError::Io {
            path: path.clone(),
            source,
        })?;
        let mut lang = Self::parse(&base);
        if let Some(overlay) = read_locale_object(store, version, language)? {
            lang.merge(&overlay);
        }
        Ok(lang)
    }

    /// Lists the locales the store can offer, sorted by code: the tree's
    /// en_US head plus every `minecraft/lang/*.lang` object the version's
    /// index names whose bytes read. Names come from each file's own
    /// `language.name`/`language.region`; an unreadable file's code is
    /// skipped, and a missing index lists en_US alone.
    pub fn available_languages(
        store: &Store,
        version: &str,
    ) -> Result<Vec<LanguageInfo>, LangError> {
        let tree = Extractor::new(store, version).root();
        let path = tree.join(TREE_LANG_FILE);
        let base = std::fs::read_to_string(&path).map_err(|source| LangError::Io {
            path: path.clone(),
            source,
        })?;
        let head = Self::parse(&base);
        let mut out = vec![head.language_info(BASE_LOCALE)];
        if let Some(index) = read_index(store, version)? {
            let mut codes: Vec<&str> = index
                .objects
                .keys()
                .filter_map(|key| key.strip_prefix(LANG_OBJECT_PREFIX)?.strip_suffix(".lang"))
                .filter(|code| *code != BASE_LOCALE)
                .collect();
            codes.sort_unstable();
            for code in codes {
                let object = locale_object_key(code);
                let Some(entry) = index.objects.get(&object) else {
                    continue;
                };
                let Ok(path) = store.object_path(&entry.hash) else {
                    continue;
                };
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let Ok(text) = std::string::String::from_utf8(bytes) else {
                    continue;
                };
                let table = Self::parse(&text);
                out.push(table.language_info(code));
            }
        }
        Ok(out)
    }
}

/// The index key holding `code`'s locale object.
fn locale_object_key(code: &str) -> String {
    format!("{LANG_OBJECT_PREFIX}{code}.lang")
}

/// Reads the version's asset index through the version document's own id.
/// A missing document or index reads as `None` (tree-only); a present but
/// unparsable one fails.
fn read_index(store: &Store, version: &str) -> Result<Option<AssetIndex>, LangError> {
    let path = store.version_json_path(version).map_err(LangError::Store)?;
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| LangError::Index(error.to_string()))?;
    let Some(id) = document
        .get("assetIndex")
        .and_then(|index| index.get("id"))
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(None);
    };
    let path = store.index_path(id).map_err(LangError::Store)?;
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    AssetIndex::parse(&text)
        .map(Some)
        .map_err(|error| LangError::Index(error.to_string()))
}

/// Reads one locale's hashed object through the version's index. Answers
/// `None` when the language is the base (the tree already holds it) or
/// when the index, the key or the bytes are missing; fails on unreadable
/// bytes or a bad store path.
fn read_locale_object(
    store: &Store,
    version: &str,
    language: &str,
) -> Result<Option<String>, LangError> {
    if language == BASE_LOCALE {
        return Ok(None);
    }
    let Some(index) = read_index(store, version)? else {
        return Ok(None);
    };
    let Some(entry) = index.objects.get(&locale_object_key(language)) else {
        return Ok(None);
    };
    let path = store.object_path(&entry.hash).map_err(LangError::Store)?;
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(None);
    };
    std::string::String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| LangError::Utf8 {
            what: locale_object_key(language),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanks_comments_and_keyless_lines_fall_out() {
        let table = parse_lang("# a comment\n#skipped=absent\n\nkey=value\nkeyless\n");
        assert_eq!(table.len(), 1);
        assert_eq!(table["key"], "value");
        assert!(!table.contains_key("#skipped"));
    }

    #[test]
    fn the_split_holds_the_first_equals_only() {
        let table = parse_lang("key=a=b=c\n");
        assert_eq!(table["key"], "a=b=c");
    }

    #[test]
    fn numeric_placeholders_rewrite_to_s() {
        let table = parse_lang(
            "a=Top - %d\nb=%.2f\nc=%1$d\nf=Estimated: %dx%d\ng=%s stays\nh=100%% sure\n",
        );
        assert_eq!(table["a"], "Top - %s");
        assert_eq!(table["b"], "%s");
        assert_eq!(table["c"], "%1$s");
        assert_eq!(table["f"], "Estimated: %sx%s");
        assert_eq!(table["g"], "%s stays");
        assert_eq!(table["h"], "100%% sure");
    }

    #[test]
    fn missing_keys_pass_through() {
        let lang = Lang::parse("a=b\n");
        assert_eq!(lang.get("a"), "b");
        assert_eq!(lang.get("nope"), "nope");
    }

    #[test]
    fn the_merge_prefers_the_overlay() {
        let mut lang = Lang::parse("a=base\nb=base\n");
        lang.merge("b=over\nc=over\n");
        assert_eq!(lang.get("a"), "base");
        assert_eq!(lang.get("b"), "over");
        assert_eq!(lang.get("c"), "over");
    }

    #[test]
    fn the_unicode_law_needs_more_than_ten_percent() {
        assert!(!values_are_unicode(["abcdefghi\u{100}"]));
        assert!(values_are_unicode(["\u{100}\u{100}abcdefghi"]));
        assert!(!values_are_unicode(Vec::<&str>::new()));
    }

    #[test]
    fn hostile_lines_truncate_and_stop() {
        let long_key = "k".repeat(MAX_LANG_KEY_LEN + 8);
        let long_value = "v".repeat(MAX_LANG_VALUE_LEN + 8);
        let table = parse_lang(&format!("{long_key}={long_value}\n"));
        let (key, value) = table.iter().next().expect("one row stands");
        assert_eq!(key.len(), MAX_LANG_KEY_LEN);
        assert_eq!(value.len(), MAX_LANG_VALUE_LEN);
        let many = "a=b\n".repeat(MAX_LANG_LINES + 4);
        let table = parse_lang(&many);
        assert_eq!(table.len(), 1, "the later lines overwrite the one key");
        let numbered: String = (0..MAX_LANG_LINES + 4)
            .map(|i| format!("k{i}=v\n"))
            .collect();
        let table = parse_lang(&numbered);
        assert_eq!(table.len(), MAX_LANG_LINES, "lines past the cap refuse");
    }

    #[test]
    fn a_missing_tree_base_fails() {
        let dir = tempfile::tempdir().expect("a scratch dir stands");
        let store = Store::open(dir.path().to_path_buf()).expect("the store opens");
        let error = Lang::load(&store, "1.8.9", "en_US").expect_err("no tree stands");
        assert!(matches!(error, LangError::Io { .. }));
    }

    /// The store-gated real-file case (checklist row 55's test vector):
    /// the tree's en_US plus the hashed locale objects behind it. Run with
    /// `OXIDECRAFT_STORE` set and `-- --ignored`.
    #[test]
    #[ignore = "reads the asset store; set OXIDECRAFT_STORE and run with -- --ignored"]
    fn the_real_tree_and_locale_objects_load() {
        let root = std::env::var_os("OXIDECRAFT_STORE")
            .filter(|root| !root.is_empty())
            .expect("OXIDECRAFT_STORE must name the store root");
        let store = Store::open(std::path::PathBuf::from(root)).expect("the store opens");
        let lang = Lang::load(&store, "1.8.9", "en_US").expect("en_US loads");
        // The tree's en_US.lang: 2679 lines, no comments, 2553 `key=value`
        // rows.
        assert_eq!(lang.len(), 2553, "the tree's en_US entry count");
        assert_eq!(lang.get("gui.done"), "Done");
        assert_eq!(lang.get("no.such.key"), "no.such.key");
        assert!(!lang.is_unicode(), "en_US reads narrow");
        // The rewrite rode the real file: `%d` patterns read `%s`.
        assert_eq!(lang.get("createWorld.customize.flat.layer.top"), "Top - %s");
        // A hashed locale overlays in source order.
        let german = Lang::load(&store, "1.8.9", "de_DE").expect("de_DE overlays");
        assert!(
            german.len() >= lang.len(),
            "the overlay keeps the base keys"
        );
        assert_eq!(german.get("gui.done"), "Fertig");
        // The locale list: the tree's en_US head plus the index's objects,
        // sorted by code.
        let locales = Lang::available_languages(&store, "1.8.9").expect("the list reads");
        assert_eq!(locales[0].code, "en_US");
        assert_eq!(locales[0].display(), "English (US)");
        let codes: Vec<&str> = locales.iter().map(|info| info.code.as_str()).collect();
        let mut tail = codes[1..].to_vec();
        tail.sort_unstable();
        assert_eq!(&codes[1..], tail.as_slice(), "the tail sorts by code");
        assert_eq!(locales.len(), 75, "en_US plus the readable locale objects");
        let _ = codes;
    }
}
