//! Story 11-14 — `propose_cover`: propose a cover image for a title
//! (FR133).

use serde::Deserialize;
use serde_json::{Value, json};

use super::args;
use super::proposal_args::{filed_document, sources_property, tool_error};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::services::cover::COVER_MEDIA_TYPES;
use crate::services::cover_proposals::{
    CoverProposalRequest, CoverProposalService, MAX_IMAGE_BASE64_CHARS,
};

pub struct ProposeCover;

const TOOL: &str = "propose_cover";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    title_id: u64,
    version: Option<i32>,
    image_base64: String,
    media_type: String,
    sources: Vec<String>,
}

/// Validate the raw arguments' shape (pure — unit tested); the image
/// rules live in `services::cover_proposals`.
fn parse_args(args: Value, api_key_id: u64) -> Result<CoverProposalRequest, ToolError> {
    let a: Args = args::parse_object(
        args,
        "{\"title_id\": <id>, \"image_base64\": <base64>, \"media_type\": <type>, \
         \"sources\": [<url>], \"version\"?: <int>}",
    )?;
    Ok(CoverProposalRequest {
        api_key_id,
        title_id: a.title_id,
        version: a.version,
        image_base64: a.image_base64,
        media_type: a.media_type,
        sources: a.sources,
    })
}

impl McpTool for ProposeCover {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "Propose a cover image for a title"
    }

    fn description(&self) -> &'static str {
        "Propose a cover image for a title, with or without a current cover (find titles \
         without one with list_titles_without_cover). Send the image itself, base64-encoded in \
         `image_base64` (standard alphabet, no `data:` prefix), with its `media_type` \
         (image/jpeg, image/png, image/webp or image/gif); the library never downloads anything \
         itself, so fetch the image first. Every proposal needs `sources`: the http(s) URLs of \
         the page (or image) where you found it. The image is checked like a librarian's \
         upload, then kept in a private staging area, reduced to at most 400 px wide (JPEG); \
         this files ONE proposal for a librarian to accept or reject; it does not change the \
         catalog. Returns `proposal_id`, `status` (`pending`), `item_count` (1) and \
         `replaces_existing` (true when the title already has a cover — the proposal is still \
         filed, as a replacement). Refused with an explanation, nothing filed, when: the title \
         id is unknown or deleted, or its `version` no longer matches (read it again with \
         get_title); the base64 is invalid or empty; the decoded image exceeds 2 MiB (send a \
         smaller version); the bytes are not a JPEG, PNG, WebP or GIF, or not the declared \
         `media_type`; the image cannot be decoded or exceeds 10000 px per side or about 16 \
         megapixels; a source is not an http(s) URL; the key was revoked or lacks the propose \
         scope. Calls are limited to about 3.7 MiB and 30 seconds."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title_id": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Numeric id of the title (from search_titles, list_titles_without_cover or get_title)."
                },
                "version": {
                    "type": "integer",
                    "description": "The title's `version` as returned by get_title. When given and the title changed since, the proposal is refused so you can re-read it. Without it, a title edited while the call runs is refused too (retry)."
                },
                "image_base64": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": MAX_IMAGE_BASE64_CHARS,
                    "description": "The image file's bytes, base64-encoded (standard alphabet; padding optional; line breaks ignored; no `data:` prefix). At most 2 MiB once decoded."
                },
                "media_type": {
                    "type": "string",
                    "enum": COVER_MEDIA_TYPES,
                    "description": "The image's type; it must match the bytes."
                },
                "sources": sources_property()
            },
            "required": ["title_id", "image_base64", "media_type", "sources"],
            "additionalProperties": false
        })
    }

    fn read_only(&self) -> bool {
        false
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let req = parse_args(args, ctx.api_key_id)?;
            let staging = ctx.state.cover_staging_dir();
            let filed = CoverProposalService::propose(&ctx.state.pool, staging.as_deref(), &req)
                .await
                .map_err(|e| tool_error(e, TOOL))?;
            let mut doc = filed_document(&filed.filed);
            doc["replaces_existing"] = json!(filed.replaces_existing);
            Ok(doc)
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
                "title_id": 4,
                "version": 3,
                "image_base64": "iVBORw0K",
                "media_type": "image/png",
                "sources": ["https://example.org/t"]
            }),
            9,
        )
        .unwrap();
        assert_eq!((req.api_key_id, req.title_id, req.version), (9, 4, Some(3)));
        assert_eq!(req.media_type, "image/png");
        assert_eq!(req.image_base64, "iVBORw0K");
    }

    #[test]
    fn parse_args_refuses_malformed_arguments() {
        let ok = json!({
            "title_id": 1, "image_base64": "x", "media_type": "image/png",
            "sources": ["https://a.b"]
        });
        assert!(parse_args(ok.clone(), 1).is_ok());
        for drop in ["title_id", "image_base64", "media_type", "sources"] {
            let mut bad = ok.clone();
            bad.as_object_mut().unwrap().remove(drop);
            assert!(
                matches!(parse_args(bad, 1), Err(ToolError::InvalidArguments(_))),
                "missing {drop}"
            );
        }
        for bad in [
            json!(null),
            json!([]),
            json!({"title_id": 1, "image_base64": 5, "media_type": "image/png", "sources": []}),
            json!({"title_id": 1, "image_base64": "x", "media_type": "image/png",
                   "sources": [], "image_url": "https://a.b/c.jpg"}),
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

    #[test]
    fn schema_bounds_follow_the_service() {
        let s = ProposeCover.input_schema();
        let p = &s["properties"];
        assert_eq!(p["image_base64"]["maxLength"], MAX_IMAGE_BASE64_CHARS);
        assert_eq!(p["media_type"]["enum"], json!(COVER_MEDIA_TYPES));
        // The largest image fits the body cap.
        const { assert!(MAX_IMAGE_BASE64_CHARS < crate::mcp::max_request_body_bytes()) };
    }
}
