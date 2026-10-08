//! The client's library surface: the modules the integration suites reach.
//!
//! The binary's own tree stays in `main.rs` and its sibling modules; the parts
//! the tests must assert against the shipping code — today the item registry
//! table (`items`), the screens as they land — are compiled here and used by
//! the binary through this crate.

pub mod items;
