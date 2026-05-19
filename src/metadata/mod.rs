pub mod bdgest;
pub mod bnf;
pub mod chain;
pub mod comic_vine;
pub mod google_books;
pub mod library_of_congress;
pub mod musicbrainz;
pub mod omdb;
pub mod open_library;
pub mod provider;
pub mod rate_limiter;
pub mod registry;
pub mod tmdb;

/// Single source of truth for which providers expose an API-key field
/// in the admin/system settings panel and the first-launch setup
/// wizard. Both `routes/admin_system.rs` and `routes/setup.rs` import
/// from here. A future provider that needs a key adds itself in the
/// same PR that wires it. Story 8-8 review P3 moved this from
/// `routes/setup.rs` so the const lives next to the providers it
/// names.
pub const KEYED_PROVIDERS: &[&str] = &["google_books", "omdb", "tmdb"];
