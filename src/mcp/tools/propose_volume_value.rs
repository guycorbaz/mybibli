//! Story 15-5 (#564, FR165) — `propose_volume_value`: propose what a
//! volume is worth today, its list price at publication as a stand-in for
//! the purchase price (story 16-3, #639), or both.

use serde::Deserialize;
use serde_json::{Number, Value, json};

use super::args;
use super::proposal_args::{filed_document, sources_property, tool_error};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::services::volume_value_proposals::{
    AmountArg, MAX_NOTE_CHARS, VolumeValueProposalService, VolumeValueRequest,
};

pub struct ProposeVolumeValue;

const TOOL: &str = "propose_volume_value";

const EXPECTED: &str = "{\"volume_id\": <id>, \"current_value\"?: {\"amount\": 18.5, \
     \"currency\": \"CHF\"}, \"list_price\"?: {\"amount\": 24.9, \"currency\": \"CHF\"}, \
     \"note\"?: <text>, \"sources\": [<url>]}";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AmountIn {
    amount: Number,
    currency: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    volume_id: u64,
    current_value: Option<AmountIn>,
    list_price: Option<AmountIn>,
    note: Option<String>,
    sources: Vec<String>,
}

impl From<AmountIn> for AmountArg {
    fn from(a: AmountIn) -> Self {
        AmountArg {
            amount: a.amount,
            currency: a.currency,
        }
    }
}

/// Validate the raw arguments' shape (pure — unit tested); the amount,
/// currency, note and source rules live in
/// `services::volume_value_proposals`.
fn parse_args(args: Value, api_key_id: u64) -> Result<VolumeValueRequest, ToolError> {
    let a: Args = args::parse_object(args, EXPECTED)?;
    if a.volume_id == 0 {
        return Err(ToolError::InvalidArguments(
            "`volume_id` must be a positive integer".to_string(),
        ));
    }
    Ok(VolumeValueRequest {
        api_key_id,
        volume_id: a.volume_id,
        current_value: a.current_value.map(Into::into),
        list_price: a.list_price.map(Into::into),
        note: a.note,
        sources: a.sources,
    })
}

fn amount_schema(what: &str) -> Value {
    json!({
        "type": "object",
        "description": format!("{what}: an amount and its currency."),
        "properties": {
            "amount": {
                "type": "number",
                "exclusiveMinimum": 0,
                "maximum": 99999999.99,
                "description": format!("{what}, with at most two decimals (e.g. 18.5).")
            },
            "currency": {
                "type": "string",
                "description": "ISO 4217 code: must be the library's `default_currency` (from get_title); no conversion is made."
            }
        },
        "required": ["amount", "currency"],
        "additionalProperties": false
    })
}

impl McpTool for ProposeVolumeValue {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "Propose what a volume is worth"
    }

    fn description(&self) -> &'static str {
        "Propose, for one physical volume, its current market value (`current_value`: amount \
         and currency), the publisher's list price at publication (`list_price`) as a stand-in \
         for the purchase price, or both; at least one is required. The librarian sees the list \
         price labelled as a stand-in, since what was actually paid is unknown. `list_price` is \
         accepted on a volume with no purchase price, and on one whose purchase price looks \
         wrong: negligible, or far from the list price, in the same currency (the library sets \
         both thresholds; a refusal gives them). Find the volumes to value first with list_volumes_to_value. Read the title \
         with get_title first: each of its `volumes` gives its id, `condition_name`, \
         `edition_comment`, `purchase_price` and `current_value` (with `updated_at`), and the \
         document gives `default_currency`, the only currency accepted. Take the copy's \
         condition and edition into account and explain them in `note`; give `sources`: the \
         http(s) URLs of the price guides or completed sales you used. This files ONE proposal \
         for a librarian to accept or reject; it does not change the catalog. Returns \
         `proposal_id` and `status` (`pending`). Refused with an explanation when: the volume \
         id is unknown or deleted; neither amount is given; an amount is not positive, has more \
         than two decimals or exceeds 99999999.99; a currency is not the default one; \
         `list_price` is given for a volume whose purchase price does not look wrong; the \
         volume already has this current value; a value proposal \
         for this volume is still pending (the message gives its id); the same amounts were \
         rejected recently; a source is not an http(s) URL."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "volume_id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Numeric id of the volume (from get_title `volumes[].id`)."
                },
                "current_value": amount_schema("What the volume is worth today"),
                "list_price": amount_schema("The publisher's list price at publication; for a volume without purchase price, or whose purchase price looks wrong"),
                "note": {
                    "type": "string",
                    "maxLength": MAX_NOTE_CHARS,
                    "description": "Optional: how you reached the amount (condition, edition, comparable sales), shown to the librarian as plain text."
                },
                "sources": sources_property()
            },
            "required": ["volume_id", "sources"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        false
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let req = parse_args(args, ctx.api_key_id)?;
            let filed = VolumeValueProposalService::propose(&ctx.state.pool, &req)
                .await
                .map_err(|e| tool_error(e, TOOL))?;
            Ok(filed_document(&filed))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_reads_the_whole_request() {
        let req = parse_args(
            json!({
                "volume_id": 4,
                "current_value": {"amount": 18.5, "currency": "CHF"},
                "list_price": {"amount": 24, "currency": "CHF"},
                "note": "good condition",
                "sources": ["https://example.org/p"]
            }),
            9,
        )
        .unwrap();
        assert_eq!((req.volume_id, req.api_key_id), (4, 9));
        assert_eq!(req.current_value.unwrap().amount.to_string(), "18.5");
        assert_eq!(req.list_price.unwrap().amount.to_string(), "24");
        assert_eq!(req.note.as_deref(), Some("good condition"));
        let bare = parse_args(
            json!({"volume_id": 4, "current_value": {"amount": 1, "currency": "CHF"},
                   "sources": ["https://a.b"]}),
            1,
        )
        .unwrap();
        assert!(bare.list_price.is_none());
        // Story 16-3 — the service refuses a request with neither amount.
        let neither = parse_args(json!({"volume_id": 4, "sources": ["https://a.b"]}), 1).unwrap();
        assert!(neither.current_value.is_none());
    }

    #[test]
    fn parse_args_refuses_malformed_arguments() {
        let cv = json!({"amount": 1, "currency": "CHF"});
        for bad in [
            json!(null),
            json!({"volume_id": 0, "current_value": cv, "sources": ["https://a.b"]}),
            json!({"volume_id": 4, "current_value": cv}),
            json!({"volume_id": 4, "current_value": {"amount": "1", "currency": "CHF"},
                   "sources": ["https://a.b"]}),
            json!({"volume_id": 4, "current_value": {"amount": 1}, "sources": ["https://a.b"]}),
            json!({"volume_id": 4, "current_value": cv, "sources": ["https://a.b"], "x": 1}),
        ] {
            assert!(
                matches!(
                    parse_args(bad.clone(), 1),
                    Err(ToolError::InvalidArguments(_))
                ),
                "{bad} should be refused"
            );
        }
    }
}
