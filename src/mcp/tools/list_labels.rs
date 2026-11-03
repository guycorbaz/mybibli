//! Story 13-9 (#581) — `list_labels`: the library's labels, so an agent
//! can name one in `propose_label`.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{args, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolFuture};
use crate::models::label::{LabelModel, LabelUsage};
use crate::services::label_proposals::library_labels;

pub struct ListLabels;

const TOOL: &str = "list_labels";

/// No argument is accepted.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Item {
    id: u64,
    name: String,
    /// Live titles carrying the label.
    title_count: i64,
    /// Live volumes carrying the label.
    volume_count: i64,
}

impl From<(LabelModel, LabelUsage)> for Item {
    fn from((label, usage): (LabelModel, LabelUsage)) -> Self {
        Item {
            id: label.id,
            name: label.name,
            title_count: usage.titles,
            volume_count: usage.volumes,
        }
    }
}

#[derive(Debug, Serialize)]
struct Answer {
    items: Vec<Item>,
}

impl McpTool for ListLabels {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "List the library's labels"
    }

    fn description(&self) -> &'static str {
        "List the labels the librarians defined to mark titles and volumes (for example \
         \"Code douteux\" or \"À vérifier\"), sorted by name, all at once. Each item gives the \
         label `id`, its `name`, and how many live titles (`title_count`) and volumes \
         (`volume_count`) carry it. Use an id or the exact name in propose_label to propose \
         marking a doubtful title or volume; get_title shows the labels a title and its \
         volumes already carry. Only librarians create labels: there is no tool to create one. \
         Takes no arguments. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let _: Args = args::parse_object(args, "{} (no arguments)")?;
            let items = library_labels(&ctx.state.pool)
                .await?
                .into_iter()
                .map(Item::from)
                .collect();
            to_json(Answer { items }, TOOL)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tool::ToolError;

    #[test]
    fn arguments_are_refused() {
        assert!(args::parse_object::<Args>(json!({}), "").is_ok());
        assert!(args::parse_object::<Args>(Value::Null, "").is_ok());
        assert!(matches!(
            args::parse_object::<Args>(json!({"page": 1}), ""),
            Err(ToolError::InvalidArguments(_))
        ));
    }

    #[test]
    fn item_carries_the_usage() {
        let item = Item::from((
            LabelModel {
                id: 3,
                name: "Code douteux".into(),
                color: Some("amber".into()),
                version: 2,
            },
            LabelUsage {
                titles: 1,
                volumes: 4,
            },
        ));
        assert_eq!(
            serde_json::to_value(item).unwrap(),
            json!({"id": 3, "name": "Code douteux", "title_count": 1, "volume_count": 4})
        );
    }
}
