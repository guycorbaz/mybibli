//! Throwaway helper: print an Argon2 PHC hash for a plain-text password,
//! using the project's own `services::password::hash_password` so the
//! parameters match what `verify_password` expects at login.
//!
//! Usage:  cargo run --example hash_password -- 'my new password'
fn main() {
    let Some(plain) = std::env::args().nth(1) else {
        eprintln!("usage: cargo run --example hash_password -- '<password>'");
        std::process::exit(2);
    };
    match mybibli::services::password::hash_password(&plain) {
        Ok(h) => println!("{h}"),
        Err(e) => {
            eprintln!("hash failed: {e:?}");
            std::process::exit(1);
        }
    }
}
