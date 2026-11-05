//! Story 15-5 (#564, FR165) — volume value proposals filed by an agent.
//!
//! The agent proposes what a volume is worth today (`current_value`) and,
//! when the volume has no purchase price, the publisher's **list price at
//! publication** as a labelled stand-in for it. The MCP tool
//! `propose_volume_value` is a thin adapter over this module; the catalog
//! does not change until a librarian accepts the proposal
//! (`services::volume_value_acceptance`).
//!
//! ## Item shapes (contract for history, review and acceptance)
//!
//! Every item targets `volumes/<id>` (it records the volume's `version`,
//! not used for staleness — `proposals::version_counts`). Amounts are
//! integer cents (D2), currencies the instance's default currency (D3).
//!
//! The **current-value line**, always present (and the last line kept):
//!
//! ```json
//! {"line": "current_value", "amount_cents": 1850, "currency": "CHF",
//!  "current": {"amount_cents": 1200 | null, "currency": "CHF" | null,
//!              "updated_at": "2026-01-02T10:00:00Z" | null},
//!  "note": "Good condition, first printing." | null,
//!  "fingerprint": "1850CHF:list:2490CHF"}
//! ```
//!
//! The **list-price line**, only when the volume had no purchase price at
//! filing (removable by the librarian):
//!
//! ```json
//! {"line": "list_price", "amount_cents": 2490, "currency": "CHF"}
//! ```
//!
//! - `current` — the volume's current value at filing: acceptance is
//!   stale when it differs from the live one (per field, D7); a list-price
//!   line is stale once a purchase price was set.
//! - `fingerprint` — the amounts as filed ([`value_fingerprint`]): the
//!   equivalence key is `volume_value:volumes/<id>:<fingerprint>`, so a
//!   rejected estimate blocks the same amounts on the same volume (D5).
//!
//! Filing refuses an unknown volume, an amount out of range or with more
//! than two decimals (never rounded, Rule #20), another currency than the
//! default, a list price for a volume that has a purchase price, no
//! change, a pending value proposal on the volume (D4) and an equivalent
//! rejection. Lock order: the filing key row (exclusive), then plain
//! reads; the duplicate read starts from the target index.

use std::fmt;

use serde_json::{Number, Value, json};

use crate::config::normalize_currency_code;
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::proposal::{ProposalKind, ProposalModel, ProposalTarget, ProposalTargetTable};
use crate::models::volume_value::{Amount, VolumeValueModel, VolumeValueState};
use crate::services::proposal_history::rfc3339_utc;
use crate::services::proposals::{
    FiledProposal, NewProposal, NewProposalItem, ProposalError, ProposalService, Refusal,
    RefusalClass, log_filed, validate_sources,
};

/// Largest amount a `DECIMAL(10,2)` column holds, in cents.
pub const MAX_AMOUNT_CENTS: i64 = 9_999_999_999;
/// Longest note the agent may attach, in characters.
pub const MAX_NOTE_CHARS: usize = 500;
/// The `settings` key of the instance currency (CR #243).
const KEY_DEFAULT_CURRENCY: &str = "default_currency";
/// The default currency when the setting is missing or unreadable (the
/// same fallback as `AppSettings`).
const FALLBACK_CURRENCY: &str = "CHF";

/// Which amount of the request (agent-facing names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountField {
    CurrentValue,
    ListPrice,
}

impl AmountField {
    fn name(self) -> &'static str {
        match self {
            AmountField::CurrentValue => "current_value",
            AmountField::ListPrice => "list_price",
        }
    }
}

/// An amount as the agent sends it.
#[derive(Debug, Clone, PartialEq)]
pub struct AmountArg {
    pub amount: Number,
    pub currency: String,
}

/// A value proposal as the agent sends it.
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeValueRequest {
    pub api_key_id: u64,
    pub volume_id: u64,
    pub current_value: AmountArg,
    pub list_price: Option<AmountArg>,
    pub note: Option<String>,
    pub sources: Vec<String>,
}

/// A value proposal that could not be filed.
#[derive(Debug)]
pub enum VolumeValueError {
    /// Not a plain positive decimal with at most two decimals, or out of
    /// range.
    InvalidAmount {
        field: AmountField,
        given: String,
    },
    WrongCurrency {
        field: AmountField,
        given: String,
        default: String,
    },
    NoteTooLong {
        chars: usize,
    },
    VolumeNotFound {
        volume_id: u64,
    },
    /// A list price for a volume that already has a purchase price.
    HasPurchasePrice {
        volume_id: u64,
    },
    /// The current value is the proposed one and no list price is given.
    NoChange {
        volume_id: u64,
    },
    PendingDuplicate {
        proposal_id: u64,
    },
    Proposal(ProposalError),
}

impl Refusal for VolumeValueError {
    fn class(&self) -> RefusalClass {
        use VolumeValueError::*;
        match self {
            InvalidAmount { .. } | WrongCurrency { .. } | NoteTooLong { .. } => {
                RefusalClass::InvalidInput
            }
            VolumeNotFound { .. } => RefusalClass::NotFound,
            HasPurchasePrice { .. } | NoChange { .. } | PendingDuplicate { .. } => {
                RefusalClass::Conflict
            }
            Proposal(e) if e.is_invalid_input() => RefusalClass::InvalidInput,
            Proposal(e) if e.is_conflict() => RefusalClass::Conflict,
            Proposal(_) => RefusalClass::Internal,
        }
    }
}

impl fmt::Display for VolumeValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use VolumeValueError::*;
        match self {
            InvalidAmount { field, given } => write!(
                f,
                "`{}.amount` is {given}: give a positive amount with at most two decimals \
                 (e.g. 18.5), at most 99999999.99. Nothing was filed.",
                field.name()
            ),
            WrongCurrency {
                field,
                given,
                default,
            } => write!(
                f,
                "`{}.currency` is {}, but the library records values in {default} and does \
                 not convert currencies: give the amount in {default}. Nothing was filed.",
                field.name(),
                Value::String(given.clone())
            ),
            NoteTooLong { chars } => write!(
                f,
                "`note` is {chars} characters long; at most {MAX_NOTE_CHARS} are allowed. \
                 Nothing was filed."
            ),
            VolumeNotFound { volume_id } => write!(
                f,
                "No volume with id {volume_id} exists in the catalog (or it was deleted). \
                 Find volume ids in get_title."
            ),
            HasPurchasePrice { volume_id } => write!(
                f,
                "Volume {volume_id} already has a purchase price: propose `list_price` only for \
                 a volume without one (get_title shows `purchase_price`). Nothing was filed."
            ),
            NoChange { volume_id } => write!(
                f,
                "Volume {volume_id} already has this current value: there is nothing to \
                 propose."
            ),
            PendingDuplicate { proposal_id } => write!(
                f,
                "Proposal {proposal_id} already proposes a value for this volume and is waiting \
                 for a librarian. Nothing was filed; follow it with list_my_proposals."
            ),
            Proposal(e) => write!(f, "{e}"),
        }
    }
}

impl From<AppError> for VolumeValueError {
    fn from(e: AppError) -> Self {
        VolumeValueError::Proposal(ProposalError::Database(e))
    }
}

impl From<sqlx::Error> for VolumeValueError {
    fn from(e: sqlx::Error) -> Self {
        VolumeValueError::Proposal(ProposalError::from(e))
    }
}

impl From<ProposalError> for VolumeValueError {
    fn from(e: ProposalError) -> Self {
        VolumeValueError::Proposal(e)
    }
}

/// The cents of a JSON number (pure — unit tested): a plain decimal
/// (`18`, `18.5`, `18.50`), positive, at most two decimals, at most
/// [`MAX_AMOUNT_CENTS`]. Anything else is `None` — never rounded.
pub fn amount_cents(n: &Number) -> Option<i64> {
    if let Some(i) = n.as_i64() {
        return i
            .checked_mul(100)
            .filter(|c| (1..=MAX_AMOUNT_CENTS).contains(c));
    }
    let text = n.to_string();
    let (whole, frac) = text.split_once('.')?;
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || frac.is_empty()
        || frac.len() > 2
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let frac_cents: i64 = format!("{frac:0<2}").parse().ok()?;
    whole
        .parse::<i64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(frac_cents)
        .filter(|c| (1..=MAX_AMOUNT_CENTS).contains(c))
}

/// The default currency as stored in `settings` (pure): normalized, or
/// the fallback when missing or not a three-letter code.
pub fn parse_default_currency(raw: Option<&str>) -> String {
    raw.and_then(normalize_currency_code)
        .unwrap_or_else(|| FALLBACK_CURRENCY.to_string())
}

/// Check one amount of the request against the default currency (pure).
pub fn check_amount(
    field: AmountField,
    arg: &AmountArg,
    default: &str,
) -> Result<i64, VolumeValueError> {
    let cents = amount_cents(&arg.amount).ok_or_else(|| VolumeValueError::InvalidAmount {
        field,
        given: arg.amount.to_string(),
    })?;
    if normalize_currency_code(&arg.currency).as_deref() != Some(default) {
        return Err(VolumeValueError::WrongCurrency {
            field,
            given: arg.currency.clone(),
            default: default.to_string(),
        });
    }
    Ok(cents)
}

/// Trim the note; empty means none (pure).
pub fn normalize_note(note: Option<&str>) -> Result<Option<String>, VolumeValueError> {
    let Some(n) = note.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    let chars = n.chars().count();
    if chars > MAX_NOTE_CHARS {
        return Err(VolumeValueError::NoteTooLong { chars });
    }
    Ok(Some(n.to_string()))
}

/// The amounts as filed, for the equivalence key (D5; pure). Changing it
/// changes stored keys: the unit test `the_fingerprint_is_stable` pins it.
pub fn value_fingerprint(value_cents: i64, currency: &str, list_price: Option<i64>) -> String {
    match list_price {
        Some(list) => format!("{value_cents}{currency}:list:{list}{currency}"),
        None => format!("{value_cents}{currency}"),
    }
}

/// The current-value line (see the module doc for the contract).
pub fn value_line(
    state: &VolumeValueState,
    value_cents: i64,
    currency: &str,
    list_price: Option<i64>,
    note: Option<&str>,
) -> Value {
    let current = state.current_value.as_ref();
    json!({
        "line": "current_value",
        "amount_cents": value_cents,
        "currency": currency,
        "current": {
            "amount_cents": current.map(|a| a.cents),
            "currency": current.and_then(|a| a.currency.clone()),
            "updated_at": state.current_value_updated_at.map(rfc3339_utc),
        },
        "note": note,
        "fingerprint": value_fingerprint(value_cents, currency, list_price),
    })
}

/// The list-price line.
pub fn list_price_line(cents: i64, currency: &str) -> Value {
    json!({"line": "list_price", "amount_cents": cents, "currency": currency})
}

/// A current-value line read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueLine {
    pub amount: Amount,
    /// The volume's current value at filing (`None`: no value).
    pub current: Option<Amount>,
    /// RFC 3339 UTC, as filed.
    pub current_updated_at: Option<String>,
    pub note: Option<String>,
}

/// Read a current-value line (pure); `None` when malformed.
pub fn parse_value_line(v: &Value) -> Option<ValueLine> {
    if v.get("line")?.as_str()? != "current_value" {
        return None;
    }
    let cents = v.get("amount_cents")?.as_i64()?;
    let currency = v.get("currency")?.as_str()?.to_string();
    let cur = v.get("current")?;
    let current = cur
        .get("amount_cents")
        .and_then(Value::as_i64)
        .map(|c| Amount {
            cents: c,
            currency: cur.get("currency").and_then(Value::as_str).map(str::to_string),
        });
    Some(ValueLine {
        amount: Amount {
            cents,
            currency: Some(currency),
        },
        current,
        current_updated_at: cur
            .get("updated_at")
            .and_then(Value::as_str)
            .map(str::to_string),
        note: v.get("note").and_then(Value::as_str).map(str::to_string),
    })
}

/// Read a list-price line (pure); `None` when malformed.
pub fn parse_list_price_line(v: &Value) -> Option<Amount> {
    if v.get("line")?.as_str()? != "list_price" {
        return None;
    }
    Some(Amount {
        cents: v.get("amount_cents")?.as_i64()?,
        currency: Some(v.get("currency")?.as_str()?.to_string()),
    })
}

/// Whether the volume's live value is the one filed (pure): same cents,
/// same currency (D7).
pub fn same_value(filed: Option<&Amount>, live: Option<&Amount>) -> bool {
    filed == live
}

pub struct VolumeValueProposalService;

impl VolumeValueProposalService {
    /// Check the request and file one `volume_value` proposal.
    pub async fn propose(
        pool: &DbPool,
        req: &VolumeValueRequest,
    ) -> Result<FiledProposal, VolumeValueError> {
        let note = normalize_note(req.note.as_deref())?;
        // Fast refusal before any lock; `file_in` checks them again.
        validate_sources(&req.sources)?;

        let mut tx = pool.begin().await?;
        // Key row first (series order): filings of one key are serialised,
        // so the duplicate check below and the insert cannot interleave.
        if !ProposalModel::lock_filing_key_exclusive(&mut tx, req.api_key_id).await? {
            return Err(ProposalError::KeyNotAllowed {
                api_key_id: req.api_key_id,
            }
            .into());
        }
        let default = parse_default_currency(
            ProposalModel::setting_value(&mut tx, KEY_DEFAULT_CURRENCY)
                .await?
                .as_deref(),
        );
        let value_cents = check_amount(AmountField::CurrentValue, &req.current_value, &default)?;
        let list_cents = req
            .list_price
            .as_ref()
            .map(|l| check_amount(AmountField::ListPrice, l, &default))
            .transpose()?;
        let Some(state) = VolumeValueModel::find_in(&mut tx, req.volume_id, false).await? else {
            return Err(VolumeValueError::VolumeNotFound {
                volume_id: req.volume_id,
            });
        };
        if list_cents.is_some() && state.purchase_price.is_some() {
            return Err(VolumeValueError::HasPurchasePrice {
                volume_id: req.volume_id,
            });
        }
        let proposed = Amount {
            cents: value_cents,
            currency: Some(default.clone()),
        };
        if list_cents.is_none() && same_value(Some(&proposed), state.current_value.as_ref()) {
            return Err(VolumeValueError::NoChange {
                volume_id: req.volume_id,
            });
        }
        if let Some(proposal_id) =
            ProposalModel::pending_volume_value(&mut tx, req.volume_id).await?
        {
            return Err(VolumeValueError::PendingDuplicate { proposal_id });
        }
        let target = Some(ProposalTarget {
            table: ProposalTargetTable::Volumes,
            id: req.volume_id,
        });
        let mut items = vec![NewProposalItem {
            target,
            expected_version: None,
            proposed_values: value_line(&state, value_cents, &default, list_cents, note.as_deref()),
        }];
        if let Some(cents) = list_cents {
            items.push(NewProposalItem {
                target,
                expected_version: None,
                proposed_values: list_price_line(cents, &default),
            });
        }
        let proposal = NewProposal {
            kind: ProposalKind::VolumeValue,
            api_key_id: req.api_key_id,
            sources: req.sources.clone(),
            items,
        };
        let proposal_id =
            ProposalService::file_in(&mut tx, &proposal)
                .await
                .map_err(|e| match e {
                    ProposalError::TargetNotFound { .. } => VolumeValueError::VolumeNotFound {
                        volume_id: req.volume_id,
                    },
                    other => other.into(),
                })?;
        tx.commit().await?;
        log_filed(proposal_id, &proposal);
        Ok(FiledProposal {
            proposal_id,
            kind: ProposalKind::VolumeValue,
            item_count: proposal.items.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(v: Value) -> Number {
        match v {
            Value::Number(n) => n,
            other => panic!("{other} is not a number"),
        }
    }

    #[test]
    fn amounts_are_plain_decimals_with_two_decimals_at_most() {
        let c = |v: Value| amount_cents(&num(v));
        assert_eq!(c(json!(18)), Some(1800));
        assert_eq!(c(json!(18.5)), Some(1850));
        assert_eq!(c(json!(18.05)), Some(1805));
        assert_eq!(c(json!(0.01)), Some(1));
        assert_eq!(c(json!(99999999.99)), Some(MAX_AMOUNT_CENTS));
        for bad in [
            json!(0),
            json!(-3),
            json!(-0.5),
            json!(0.0),
            json!(18.555),
            json!(100000000),
            json!(1e20),
        ] {
            assert_eq!(c(bad.clone()), None, "{bad} should be refused");
        }
    }

    #[test]
    fn the_default_currency_falls_back_like_the_settings() {
        assert_eq!(parse_default_currency(Some("eur")), "EUR");
        assert_eq!(parse_default_currency(Some(" CHF ")), "CHF");
        assert_eq!(parse_default_currency(Some("EURO")), "CHF");
        assert_eq!(parse_default_currency(None), "CHF");
    }

    #[test]
    fn amounts_must_be_in_the_default_currency() {
        let arg = |amount: Value, currency: &str| AmountArg {
            amount: num(amount),
            currency: currency.to_string(),
        };
        assert_eq!(
            check_amount(AmountField::CurrentValue, &arg(json!(12.5), "chf"), "CHF").unwrap(),
            1250
        );
        let wrong = check_amount(AmountField::ListPrice, &arg(json!(12.5), "EUR"), "CHF")
            .unwrap_err();
        assert_eq!(wrong.class(), RefusalClass::InvalidInput);
        let m = wrong.to_string();
        assert!(m.contains("`list_price.currency`") && m.contains("in CHF"), "{m}");
        let bad = check_amount(AmountField::CurrentValue, &arg(json!(1.234), "CHF"), "CHF")
            .unwrap_err();
        assert!(bad.to_string().contains("`current_value.amount` is 1.234"), "{bad}");
    }

    #[test]
    fn notes_are_trimmed_and_bounded() {
        assert_eq!(normalize_note(Some("  ")).unwrap(), None);
        assert_eq!(normalize_note(Some(" ok ")).unwrap().as_deref(), Some("ok"));
        let long = "é".repeat(MAX_NOTE_CHARS + 1);
        assert!(matches!(
            normalize_note(Some(&long)),
            Err(VolumeValueError::NoteTooLong { chars: 501 })
        ));
    }

    #[test]
    fn the_fingerprint_is_stable() {
        assert_eq!(value_fingerprint(1850, "CHF", None), "1850CHF");
        assert_eq!(
            value_fingerprint(1850, "CHF", Some(2490)),
            "1850CHF:list:2490CHF"
        );
    }

    fn state(current: Option<Amount>) -> VolumeValueState {
        VolumeValueState {
            id: 4,
            title_id: 2,
            label: "V0004".into(),
            version: 3,
            purchase_price: None,
            current_value: current,
            current_value_updated_at: None,
        }
    }

    #[test]
    fn lines_round_trip() {
        let held = Amount {
            cents: 1200,
            currency: Some("CHF".into()),
        };
        let v = value_line(&state(Some(held.clone())), 1850, "CHF", Some(2490), Some("why"));
        let line = parse_value_line(&v).unwrap();
        assert_eq!(line.amount.cents, 1850);
        assert_eq!(line.current, Some(held));
        assert_eq!(line.note.as_deref(), Some("why"));
        assert_eq!(v["fingerprint"], json!("1850CHF:list:2490CHF"));
        let blank = parse_value_line(&value_line(&state(None), 1850, "CHF", None, None)).unwrap();
        assert_eq!(blank.current, None);
        let list = parse_list_price_line(&list_price_line(2490, "CHF")).unwrap();
        assert_eq!(list.cents, 2490);
        assert_eq!(parse_value_line(&json!({"line": "list_price"})), None);
        assert_eq!(parse_list_price_line(&json!({"line": "list_price"})), None);
    }

    #[test]
    fn refusals_are_classed_and_actionable() {
        use VolumeValueError::*;
        assert_eq!(VolumeNotFound { volume_id: 3 }.class(), RefusalClass::NotFound);
        assert!(
            VolumeNotFound { volume_id: 3 }
                .to_string()
                .contains("get_title")
        );
        for conflict in [
            HasPurchasePrice { volume_id: 3 },
            NoChange { volume_id: 3 },
            PendingDuplicate { proposal_id: 9 },
        ] {
            assert_eq!(conflict.class(), RefusalClass::Conflict, "{conflict}");
        }
        assert!(
            PendingDuplicate { proposal_id: 9 }
                .to_string()
                .starts_with("Proposal 9")
        );
        assert_eq!(
            Proposal(ProposalError::NoSources).class(),
            RefusalClass::InvalidInput
        );
    }
}
