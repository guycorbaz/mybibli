//! Integration tests for #384: UPC-A check-digit validation in
//! `TitleService::create_from_code`.
//!
//! `CodeType::Upc` is a catch-all for 8–13 digit product barcodes (EAN-8,
//! UPC-A, EAN-13 — see `routes::catalog::detect_code_type`). The #384 guard
//! is deliberately scoped to the 12-digit UPC-A case, so a 13-digit EAN-13
//! product code (also classified `Upc`) must NOT be rejected by the UPC-A
//! mod-10 algorithm. These tests lock both halves of that contract.
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test upc_checksum_guard

use mybibli::error::AppError;
use mybibli::models::media_type::{CodeType, MediaType};
use mybibli::services::title::TitleService;
use sqlx::MySqlPool;

#[sqlx::test(migrations = "./migrations")]
async fn invalid_12_digit_upc_is_rejected(pool: MySqlPool) {
    // 036000291453 = the valid UPC-A 036000291452 with its check digit
    // flipped 2 → 3. The #384 guard must reject it before the provider chain.
    let err = TitleService::create_from_code(
        &pool,
        "036000291453",
        MediaType::Book,
        CodeType::Upc,
        None,
    )
    .await
    .expect_err("invalid 12-digit UPC-A must be rejected");
    assert!(matches!(err, AppError::BadRequest(_)), "expected BadRequest, got {err:?}");
}

#[sqlx::test(migrations = "./migrations")]
async fn valid_12_digit_upc_is_accepted(pool: MySqlPool) {
    let (_title, is_new) = TitleService::create_from_code(
        &pool,
        "036000291452",
        MediaType::Book,
        CodeType::Upc,
        None,
    )
    .await
    .expect("valid UPC-A must be accepted");
    assert!(is_new, "a fresh UPC-A should create a new title");
}

#[sqlx::test(migrations = "./migrations")]
async fn thirteen_digit_ean_classified_as_upc_is_not_rejected(pool: MySqlPool) {
    // 0093624738626 is a real 13-digit EAN-13 CD barcode that
    // detect_code_type classifies as CodeType::Upc. The #384 guard is scoped
    // to 12-digit codes, so it must pass through untouched (no UPC-A check).
    let (_title, is_new) = TitleService::create_from_code(
        &pool,
        "0093624738626",
        MediaType::Cd,
        CodeType::Upc,
        None,
    )
    .await
    .expect("13-digit EAN classified as UPC must pass the #384 guard");
    assert!(is_new, "a fresh 13-digit EAN should create a new title");
}
