//! Story 15-5 (#564, FR165) — volume value proposals filed by an agent.
//!
//! The agent proposes what a volume is worth today (`current_value`), the
//! publisher's **list price at publication** as a labelled stand-in for the
//! purchase price, or both (story 16-3, #639: at least one). The list price
//! fills an empty purchase price, or replaces one matching a listed
//! inconsistent shape ([`purchase_price_problem`], thresholds set in Admin →
//! System). The MCP tool
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
//! The **current-value line**, when a value is proposed (not removable):
//!
//! ```json
//! {"line": "current_value", "amount_cents": 1850, "currency": "CHF",
//!  "current": {"amount_cents": 1200 | null, "currency": "CHF" | null,
//!              "updated_at": "2026-01-02T10:00:00Z" | null},
//!  "note": "Good condition, first printing." | null,
//!  "fingerprint": "1850CHF:list:2490CHF"}
//! ```
//!
//! The **list-price line**, when the volume had no purchase price at filing
//! or one of an inconsistent shape (removable unless it is the last line):
//!
//! ```json
//! {"line": "list_price", "amount_cents": 2490, "currency": "CHF"}
//! ```
//!
//! Story 16-3 adds, only when they apply: `replaces` (the purchase price at
//! filing, `{"amount_cents", "currency"}`) and `inconsistent` (its
//! [`PriceProblem`]) on a replacement; `note` and `fingerprint` on a line
//! filed without a current value (`list:2490CHF`).
//!
//! - `current` — the volume's current value at filing: acceptance is
//!   stale when it differs from the live one (per field, D7); a list-price
//!   line is stale once the purchase price differs from the one filed.
//! - `fingerprint` — the amounts as filed ([`value_fingerprint`]): the
//!   equivalence key is `volume_value:volumes/<id>:<fingerprint>`, so a
//!   rejected estimate blocks the same amounts on the same volume (D5).
//!
//! Filing refuses an unknown volume, an amount out of range or with more
//! than two decimals (never rounded, Rule #20), another currency than the
//! default, a list price for a volume whose purchase price is not of an
//! inconsistent shape, no amount at all, an unchanged value, a pending value proposal on the volume (D4) and an equivalent
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
/// Story 16-3 — the `settings` keys of [`PriceThresholds`] (Admin → System).
pub const KEY_NEGLIGIBLE_AMOUNT: &str = "purchase_negligible_amount";
pub const KEY_GAP_FACTOR: &str = "purchase_gap_factor";
/// Story 16-3 — the accepted range of `purchase_gap_factor`: a factor of 1
/// would make every price replaceable.
pub const MIN_GAP_FACTOR: i64 = 2;
pub const MAX_GAP_FACTOR: i64 = 100;
/// Story 16-3 — from this factor, a probable scale error: only the reason
/// shown to the librarian, not a threshold.
pub const SCALE_FACTOR: i64 = 10;

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
    pub current_value: Option<AmountArg>,
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
    /// Story 16-3 — neither `current_value` nor `list_price`.
    NothingProposed,
    /// A list price for a volume whose purchase price is of no inconsistent
    /// shape.
    HasPurchasePrice {
        volume_id: u64,
        thresholds: PriceThresholds,
    },
    /// The current value is the proposed one.
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
            InvalidAmount { .. } | WrongCurrency { .. } | NoteTooLong { .. } | NothingProposed => {
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
            NothingProposed => write!(
                f,
                "Give `current_value`, `list_price` or both: there is nothing to propose. \
                 Nothing was filed."
            ),
            HasPurchasePrice {
                volume_id,
                thresholds,
            } => write!(
                f,
                "Volume {volume_id} already has a purchase price that does not look wrong: \
                 propose `list_price` only for a volume without one, or whose purchase price is \
                 at most {} against a higher list price, or differs from the list price by a \
                 factor of {} or more, in the same currency (get_title shows `purchase_price`). \
                 Nothing was filed.",
                crate::models::volume_value::cents_to_decimal(thresholds.negligible_cents),
                thresholds.gap_factor
            ),
            NoChange { volume_id } => write!(
                f,
                "Volume {volume_id} already has this current value: propose another value, or \
                 leave `current_value` out to propose `list_price` alone. Nothing was filed."
            ),
            PendingDuplicate { proposal_id } => write!(
                f,
                "Proposal {proposal_id} already proposes amounts for this volume and is waiting \
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

/// Story 16-3 — the fingerprint of a list price filed without a value
/// (pure; pinned with [`value_fingerprint`]).
pub fn list_fingerprint(cents: i64, currency: &str) -> String {
    format!("list:{cents}{currency}")
}

/// Story 16-3 (#639) — why a stored purchase price looks wrong against an
/// attested list price. Positive shapes only (Rule #20): anything else is
/// a real price, even a bargain, and is left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceProblem {
    /// Zero, or at most [`PriceThresholds::negligible_cents`], against a list
    /// price above it.
    Negligible,
    /// Past the gap factor and [`SCALE_FACTOR`] or more away from the list
    /// price (24 typed 2400).
    Scale,
    /// [`PriceThresholds::gap_factor`] or more away from the list price.
    Gap,
}

impl PriceProblem {
    pub fn as_str(self) -> &'static str {
        match self {
            PriceProblem::Negligible => "negligible",
            PriceProblem::Scale => "scale",
            PriceProblem::Gap => "gap",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "negligible" => Some(PriceProblem::Negligible),
            "scale" => Some(PriceProblem::Scale),
            "gap" => Some(PriceProblem::Gap),
            _ => None,
        }
    }
}

/// Story 16-3 — from when a purchase price looks wrong (Admin → System).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceThresholds {
    pub negligible_cents: i64,
    pub gap_factor: i64,
}

impl Default for PriceThresholds {
    fn default() -> Self {
        PriceThresholds {
            negligible_cents: 100,
            gap_factor: 3,
        }
    }
}

/// An amount typed or stored as text (pure): digits, at most two decimals, at
/// most [`MAX_AMOUNT_CENTS`], zero allowed. `None` otherwise, never rounded.
pub fn parse_threshold_cents(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    let (whole, frac) = raw.split_once('.').unwrap_or((raw, ""));
    let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
    // `.5` is what a number input may send; a lone `.` is nothing.
    if (whole.is_empty() && frac.is_empty()) || !digits(whole) || !digits(frac) || frac.len() > 2 {
        return None;
    }
    let frac: i64 = format!("{frac:0<2}").parse().ok()?;
    let whole = if whole.is_empty() { "0" } else { whole };
    whole
        .parse::<i64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(frac)
        .filter(|c| *c <= MAX_AMOUNT_CENTS)
}

/// A gap factor typed or stored as text (pure): a whole number in range.
pub fn parse_gap_factor(raw: &str) -> Option<i64> {
    raw.trim()
        .parse::<i64>()
        .ok()
        .filter(|f| (MIN_GAP_FACTOR..=MAX_GAP_FACTOR).contains(f))
}

impl PriceThresholds {
    /// As stored (pure): an invalid or missing value reads as its default.
    pub fn parse(negligible: Option<&str>, gap: Option<&str>) -> Self {
        let defaults = Self::default();
        PriceThresholds {
            negligible_cents: negligible
                .and_then(parse_threshold_cents)
                .unwrap_or(defaults.negligible_cents),
            gap_factor: gap.and_then(parse_gap_factor).unwrap_or(defaults.gap_factor),
        }
    }
}

/// The shape of an inconsistent purchase price (pure — unit tested);
/// `None` is every other price, or one in another currency (amounts in two
/// currencies cannot be compared).
pub fn purchase_price_problem(
    purchase: &Amount,
    list_cents: i64,
    default: &str,
    thresholds: PriceThresholds,
) -> Option<PriceProblem> {
    if purchase.currency.as_deref() != Some(default) {
        return None;
    }
    if purchase.cents <= thresholds.negligible_cents && list_cents > thresholds.negligible_cents {
        return Some(PriceProblem::Negligible);
    }
    // Below, a purchase price of zero against a list price that is itself
    // negligible: no factor to read.
    if purchase.cents == 0 {
        return None;
    }
    let (high, low) = (purchase.cents.max(list_cents), purchase.cents.min(list_cents));
    if high / thresholds.gap_factor < low {
        None
    } else if high / SCALE_FACTOR >= low {
        Some(PriceProblem::Scale)
    } else {
        Some(PriceProblem::Gap)
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

/// Story 16-3 — a list-price line replacing an inconsistent purchase price.
pub fn with_replacement(mut line: Value, purchase: &Amount, problem: PriceProblem) -> Value {
    line["replaces"] = json!({"amount_cents": purchase.cents, "currency": purchase.currency});
    line["inconsistent"] = json!(problem.as_str());
    line
}

/// Story 16-3 — a list-price line filed without a current value: it
/// carries the note and the proposal's fingerprint.
pub fn alone(mut line: Value, cents: i64, currency: &str, note: Option<&str>) -> Value {
    line["note"] = json!(note);
    line["fingerprint"] = json!(list_fingerprint(cents, currency));
    line
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

/// A list-price line read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPriceLine {
    pub amount: Amount,
    /// Story 16-3 — the purchase price at filing; `None`: there was none.
    pub replaces: Option<Amount>,
    pub problem: Option<PriceProblem>,
    /// Story 16-3 — the note of a line filed without a current value.
    pub note: Option<String>,
}

/// Read a list-price line (pure); `None` when malformed.
pub fn parse_list_price_line(v: &Value) -> Option<ListPriceLine> {
    if v.get("line")?.as_str()? != "list_price" {
        return None;
    }
    let replaces = match v.get("replaces") {
        None | Some(Value::Null) => None,
        Some(r) => Some(Amount {
            cents: r.get("amount_cents")?.as_i64()?,
            currency: r.get("currency").and_then(Value::as_str).map(str::to_string),
        }),
    };
    Some(ListPriceLine {
        amount: Amount {
            cents: v.get("amount_cents")?.as_i64()?,
            currency: Some(v.get("currency")?.as_str()?.to_string()),
        },
        replaces,
        problem: v
            .get("inconsistent")
            .and_then(Value::as_str)
            .and_then(PriceProblem::parse),
        note: v.get("note").and_then(Value::as_str).map(str::to_string),
    })
}

/// Story 15-5 (review round 1) — the current-value line once the
/// list-price line is removed (pure): its fingerprint drops the list
/// price, so a rejection then blocks the amounts the librarian rejected
/// (the key is recomputed from the live lines at rejection). `None` when
/// the line would then change nothing — the proposed value is the one
/// filed as current: removing the list price is refused (reject
/// instead), so an unchanged estimate is never re-dated.
pub fn value_line_without_list_price(values: &Value) -> Option<Value> {
    let line = parse_value_line(values)?;
    if same_value(Some(&line.amount), line.current.as_ref()) {
        return None;
    }
    let currency = line.amount.currency.as_deref().unwrap_or_default();
    let mut out = values.clone();
    out["fingerprint"] = json!(value_fingerprint(line.amount.cents, currency, None));
    Some(out)
}

/// Whether the volume's live value is the one filed (pure): same cents,
/// same currency (D7).
pub fn same_value(filed: Option<&Amount>, live: Option<&Amount>) -> bool {
    filed == live
}

/// Story 15-6 (D5) — one page of the volumes an agent should value first
/// (the MCP `list_volumes_to_value` tool); `None` when the `series_id`
/// filter names no live series (review: an empty page would read as
/// "nothing to value").
pub async fn volumes_to_value(
    pool: &DbPool,
    filter: crate::models::value_coverage::VolumesToValueFilter,
    page: u32,
) -> Result<
    Option<crate::models::PaginatedList<crate::models::value_coverage::VolumeToValue>>,
    AppError,
> {
    if let Some(id) = filter.series_id
        && !ProposalModel::live_series_names(pool, &[id])
            .await?
            .contains_key(&id)
    {
        return Ok(None);
    }
    crate::models::value_coverage::ValueCoverageModel::volumes_to_value(pool, filter, page)
        .await
        .map(Some)
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
        if req.current_value.is_none() && req.list_price.is_none() {
            return Err(VolumeValueError::NothingProposed);
        }
        let value_cents = req
            .current_value
            .as_ref()
            .map(|v| check_amount(AmountField::CurrentValue, v, &default))
            .transpose()?;
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
        // Story 16-3 — a purchase price is replaced only when of a listed
        // inconsistent shape (Rule #20); the librarian decides.
        let replaced = match (list_cents, state.purchase_price.as_ref()) {
            (Some(list), Some(purchase)) => {
                let thresholds = PriceThresholds::parse(
                    ProposalModel::setting_value(&mut tx, KEY_NEGLIGIBLE_AMOUNT)
                        .await?
                        .as_deref(),
                    ProposalModel::setting_value(&mut tx, KEY_GAP_FACTOR)
                        .await?
                        .as_deref(),
                );
                let problem = purchase_price_problem(purchase, list, &default, thresholds).ok_or(
                    VolumeValueError::HasPurchasePrice {
                        volume_id: req.volume_id,
                        thresholds,
                    },
                )?;
                Some((purchase, problem))
            }
            _ => None,
        };
        let unchanged = value_cents.is_some_and(|cents| {
            let proposed = Amount {
                cents,
                currency: Some(default.clone()),
            };
            same_value(Some(&proposed), state.current_value.as_ref())
        });
        // Review of 16-3 — also with a list price: a value is optional now,
        // and an unchanged estimate is never re-dated.
        if unchanged {
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
        let mut items = Vec::new();
        if let Some(cents) = value_cents {
            items.push(NewProposalItem {
                target,
                expected_version: None,
                proposed_values: value_line(&state, cents, &default, list_cents, note.as_deref()),
            });
        }
        if let Some(cents) = list_cents {
            let mut line = list_price_line(cents, &default);
            if let Some((purchase, problem)) = replaced {
                line = with_replacement(line, purchase, problem);
            }
            if value_cents.is_none() {
                line = alone(line, cents, &default, note.as_deref());
            }
            items.push(NewProposalItem {
                target,
                expected_version: None,
                proposed_values: line,
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
        assert_eq!(list.amount.cents, 2490);
        assert_eq!((list.replaces, list.problem, list.note), (None, None, None));
        assert_eq!(parse_value_line(&json!({"line": "list_price"})), None);
        assert_eq!(parse_list_price_line(&json!({"line": "list_price"})), None);
    }

    fn chf(cents: i64) -> Amount {
        Amount {
            cents,
            currency: Some("CHF".into()),
        }
    }

    #[test]
    fn only_listed_shapes_of_purchase_price_are_inconsistent() {
        use PriceProblem::*;
        let t = PriceThresholds::default();
        let p = |cents, list| purchase_price_problem(&chf(cents), list, "CHF", t);
        assert_eq!(p(0, 2400), Some(Negligible));
        assert_eq!(p(100, 2400), Some(Negligible));
        // Two negligible prices that agree are not a problem.
        assert_eq!(p(100, 100), None);
        assert_eq!(p(50, 60), None);
        assert_eq!(p(0, 50), None);
        assert_eq!(p(240000, 2400), Some(Scale));
        assert_eq!(p(24000, 2400), Some(Scale));
        assert_eq!(p(240, 2400), Some(Scale));
        assert_eq!(p(7200, 2400), Some(Gap));
        assert_eq!(p(800, 2400), Some(Gap));
        // Everything else is a real price, a bargain included.
        assert_eq!(p(801, 2400), None);
        assert_eq!(p(2400, 2400), None);
        assert_eq!(p(7199, 2400), None);
        // Two currencies cannot be compared, nor a price without a currency.
        let eur = Amount {
            cents: 240000,
            currency: Some("EUR".into()),
        };
        assert_eq!(purchase_price_problem(&eur, 2400, "CHF", t), None);
        let bare = Amount {
            cents: 0,
            currency: None,
        };
        assert_eq!(purchase_price_problem(&bare, 2400, "CHF", t), None);
    }

    #[test]
    fn the_thresholds_are_read_and_applied() {
        use PriceProblem::*;
        let at = |negligible_cents, gap_factor, cents, list| {
            let t = PriceThresholds {
                negligible_cents,
                gap_factor,
            };
            purchase_price_problem(&chf(cents), list, "CHF", t)
        };
        // A wider negligible bound, a tighter or looser factor.
        assert_eq!(at(500, 3, 500, 2400), Some(Negligible));
        assert_eq!(at(100, 2, 1200, 2400), Some(Gap));
        assert_eq!(at(100, 3, 1200, 2400), None);
        // Past a high factor only: the scale label does not widen the rule.
        assert_eq!(at(100, 20, 24000, 2400), None);
        assert_eq!(at(100, 20, 48000, 2400), Some(Scale));
        // Stored values: invalid reads as the default.
        let d = PriceThresholds::default();
        assert_eq!(PriceThresholds::parse(None, None), d);
        assert_eq!(PriceThresholds::parse(Some("abc"), Some("1")), d);
        assert_eq!(PriceThresholds::parse(Some("-1"), Some("101")), d);
        assert_eq!(
            PriceThresholds::parse(Some(" 2.5 "), Some(" 4 ")),
            PriceThresholds {
                negligible_cents: 250,
                gap_factor: 4
            }
        );
        assert_eq!(parse_threshold_cents("0"), Some(0));
        assert_eq!(parse_threshold_cents(".5"), Some(50));
        assert_eq!(parse_threshold_cents("."), None);
        assert_eq!(parse_threshold_cents("1.005"), None);
        assert_eq!(parse_threshold_cents("1,00"), None);
    }

    #[test]
    fn a_replacing_line_alone_round_trips() {
        let line = alone(
            with_replacement(list_price_line(2400, "CHF"), &chf(240000), PriceProblem::Scale),
            2400,
            "CHF",
            Some("why"),
        );
        assert_eq!(line["fingerprint"], json!("list:2400CHF"));
        let read = parse_list_price_line(&line).unwrap();
        assert_eq!(read.replaces, Some(chf(240000)));
        assert_eq!(read.problem, Some(PriceProblem::Scale));
        assert_eq!(read.note.as_deref(), Some("why"));
        for p in [PriceProblem::Negligible, PriceProblem::Scale, PriceProblem::Gap] {
            assert_eq!(PriceProblem::parse(p.as_str()), Some(p));
        }
        assert_eq!(PriceProblem::parse("other"), None);
    }

    #[test]
    fn removing_the_list_price_drops_it_from_the_fingerprint() {
        let v = value_line(&state(None), 1850, "CHF", Some(2490), None);
        let out = value_line_without_list_price(&v).unwrap();
        assert_eq!(out["fingerprint"], json!("1850CHF"));
        assert_eq!(out["amount_cents"], json!(1850));
        // The same value as the current one: nothing would remain.
        let held = Amount {
            cents: 1850,
            currency: Some("CHF".into()),
        };
        let same = value_line(&state(Some(held)), 1850, "CHF", Some(2490), None);
        assert_eq!(value_line_without_list_price(&same), None);
    }

    #[test]
    fn refusals_are_classed_and_actionable() {
        use VolumeValueError::*;
        assert_eq!(VolumeNotFound { volume_id: 3 }.class(), RefusalClass::NotFound);
        assert_eq!(NothingProposed.class(), RefusalClass::InvalidInput);
        assert!(
            VolumeNotFound { volume_id: 3 }
                .to_string()
                .contains("get_title")
        );
        for conflict in [
            HasPurchasePrice {
                volume_id: 3,
                thresholds: PriceThresholds::default(),
            },
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
