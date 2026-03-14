use crate::state_model::VisemeFrame;

#[derive(Debug, Clone)]
pub struct VoicePipeline;

impl VoicePipeline {
    pub fn new() -> Self {
        Self
    }

    #[allow(clippy::unused_async)]
    pub async fn transcribe_mock(&self, spoken_text: &str) -> String {
        spoken_text.to_owned()
    }

    #[allow(clippy::unused_async)]
    pub async fn synthesize(&self, text: &str) -> Vec<u8> {
        text.as_bytes().to_vec()
    }

    #[allow(clippy::unused_self)]
    pub fn estimate_duration_ms(&self, text: &str) -> u64 {
        let base = (text.chars().count() as u64).saturating_mul(80);
        base.max(300)
    }

    #[allow(clippy::unused_self)]
    pub fn lipsync_frames(&self, text: &str, total_ms: u64) -> Vec<VisemeFrame> {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return vec![];
        }

        let step = (total_ms / chars.len() as u64).max(30);
        chars
            .iter()
            .enumerate()
            .map(|(idx, ch)| {
                let start_ms = idx as u64 * step;
                let end_ms = start_ms + step;
                let viseme = map_viseme(*ch).to_owned();
                let intensity = if "aeiouAEIOU".contains(*ch) { 1.0 } else { 0.5 };
                VisemeFrame {
                    start_ms,
                    end_ms,
                    viseme,
                    intensity,
                }
            })
            .collect()
    }
}

impl Default for VoicePipeline {
    fn default() -> Self {
        Self::new()
    }
}

fn map_viseme(ch: char) -> &'static str {
    match ch.to_ascii_lowercase() {
        'a' => "A",
        'e' => "E",
        'i' => "I",
        'o' => "O",
        'u' => "U",
        _ => "M",
    }
}
