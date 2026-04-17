// i18n initialization — configured in lib.rs via rust_i18n::i18n! macro

pub mod resolve;
pub use resolve::resolve_locale;

#[cfg(test)]
mod audit;
