use crate::types::ToolDefinition;
use serde_json::json;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "self_improve".to_string(),
        description: "Track learnings, view model usage stats, and generate retrospectives. Actions: add_learning, list_learnings, retrospective, stats.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add_learning", "list_learnings", "retrospective", "stats"],
                    "description": "The action to perform"
                },
                "category": {
                    "type": "string",
                    "enum": ["mistake", "preference", "pattern", "optimization", "tool"],
                    "description": "Learning category (required for add_learning, optional filter for list_learnings)"
                },
                "text": {
                    "type": "string",
                    "description": "Learning text (required for add_learning)"
                },
                "source": {
                    "type": "string",
                    "description": "Source task/run that produced this learning (required for add_learning)"
                }
            },
            "required": ["action"]
        }),
    }
}
