use anyhow::{Context, Result};
use std::path::PathBuf;

/// Voice system — ElevenLabs TTS + optional STT
pub struct VoiceEngine {
    api_key: String,
    voice_id: String,
    cache_dir: PathBuf,
}

const DEFAULT_VOICE_ID: &str = "pFZP5JQG7iQjIQuC4Bku"; // Lily
const ELEVENLABS_TTS_URL: &str = "https://api.elevenlabs.io/v1/text-to-speech";

impl VoiceEngine {
    pub fn new(api_key: String, voice_id: Option<String>) -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("devman")
            .join("voice-cache");
        std::fs::create_dir_all(&cache_dir)?;

        Ok(Self {
            api_key,
            voice_id: voice_id.unwrap_or_else(|| DEFAULT_VOICE_ID.to_string()),
            cache_dir,
        })
    }

    /// Convert text to speech, return path to audio file
    pub async fn tts(&self, text: &str) -> Result<PathBuf> {
        // Check cache (hash of text + voice_id)
        let hash = simple_hash(text, &self.voice_id);
        let cache_path = self.cache_dir.join(format!("{hash}.mp3"));
        if cache_path.exists() {
            return Ok(cache_path);
        }

        let client = reqwest::Client::new();
        let url = format!("{}/{}", ELEVENLABS_TTS_URL, self.voice_id);

        let body = serde_json::json!({
            "text": text,
            "model_id": "eleven_multilingual_v2",
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75,
                "style": 0.0,
                "use_speaker_boost": true
            }
        });

        let response = client
            .post(&url)
            .header("xi-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "audio/mpeg")
            .json(&body)
            .send()
            .await
            .context("ElevenLabs TTS request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("ElevenLabs TTS error {status}: {text}");
        }

        let bytes = response.bytes().await?;
        std::fs::write(&cache_path, &bytes)?;

        Ok(cache_path)
    }

    /// Transcribe audio file to text (ElevenLabs Scribe)
    pub async fn stt(&self, audio_path: &PathBuf) -> Result<String> {
        let client = reqwest::Client::new();

        let file_bytes = tokio::fs::read(audio_path).await?;
        let file_name = audio_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str("audio/ogg")?;

        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model_id", "scribe_v1");

        let response = client
            .post("https://api.elevenlabs.io/v1/speech-to-text")
            .header("xi-api-key", &self.api_key)
            .multipart(form)
            .send()
            .await
            .context("ElevenLabs STT request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("ElevenLabs STT error {status}: {text}");
        }

        let data: serde_json::Value = response.json().await?;
        let text = data["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(text)
    }
}

/// Simple hash for cache keys
fn simple_hash(text: &str, voice_id: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    voice_id.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
