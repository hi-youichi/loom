use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ModalityType {
    Text,
    Image,
    Audio,
    Video,
    Pdf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Modalities {
    #[serde(default)]
    pub input: Vec<ModalityType>,

    #[serde(default)]
    pub output: Vec<ModalityType>,
}

impl Modalities {
    pub fn supports_text(&self) -> bool {
        self.input.contains(&ModalityType::Text)
    }

    pub fn supports_vision(&self) -> bool {
        self.input.contains(&ModalityType::Image)
    }

    pub fn supports_audio(&self) -> bool {
        self.input.contains(&ModalityType::Audio)
    }

    pub fn supports_video(&self) -> bool {
        self.input.contains(&ModalityType::Video)
    }

    pub fn supports_pdf(&self) -> bool {
        self.input.contains(&ModalityType::Pdf)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelLimit {
    pub context: u32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u32>,

    pub output: u32,
}

impl ModelLimit {
    pub fn new(context: u32, output: u32) -> Self {
        Self {
            context,
            input: None,
            output,
        }
    }

    pub fn with_input(mut self, limit: u32) -> Self {
        self.input = Some(limit);
        self
    }
}
