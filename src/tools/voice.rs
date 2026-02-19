use anyhow::Result;
use serde_json::json;

use crate::types::ToolDefinition;
use crate::voice::VoiceEngine;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "tts".into(),
        description: "Convert text to speech using ElevenLabs. Returns the path to the generated audio file.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to convert to speech"
                }
            },
            "required": ["text"]
        }),
    }
}

pub async fn execute(input: &serde_json::Value, voice: &VoiceEngine) -> Result<String> {
    let text = input["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'text' field"))?;

    let audio_path = voice.tts(text).await?;
    Ok(format!("Audio generated: {}", audio_path.display()))
}
