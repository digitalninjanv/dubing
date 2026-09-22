use crate::domain::DomainError;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelRole {
    Transcription,
    Translation,
    Tts,
}

impl ModelRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transcription => "transcription",
            Self::Translation => "translation",
            Self::Tts => "tts",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub transcription: bool,
    pub translation: bool,
    pub tts: bool,
    pub streaming_tts: bool,
}

impl ModelCapabilities {
    pub const fn transcription() -> Self {
        Self {
            transcription: true,
            translation: false,
            tts: false,
            streaming_tts: false,
        }
    }

    pub const fn translation() -> Self {
        Self {
            transcription: false,
            translation: true,
            tts: false,
            streaming_tts: false,
        }
    }

    pub const fn general_text_audio() -> Self {
        Self {
            transcription: true,
            translation: true,
            tts: false,
            streaming_tts: false,
        }
    }

    pub const fn tts() -> Self {
        Self {
            transcription: false,
            translation: false,
            tts: true,
            streaming_tts: true,
        }
    }

    fn supports(&self, role: ModelRole) -> bool {
        match role {
            ModelRole::Transcription => self.transcription,
            ModelRole::Translation => self.translation,
            ModelRole::Tts => self.tts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSpec {
    pub provider: String,
    pub id: String,
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    models: HashMap<(String, String), ModelSpec>,
}

impl ModelRegistry {
    pub fn standard() -> Self {
        let mut registry = Self::default();

        registry.register(ModelSpec {
            provider: "gemini".to_string(),
            id: "gemini-3.5-transcribe".to_string(),
            capabilities: ModelCapabilities::transcription(),
        });
        registry.register(ModelSpec {
            provider: "gemini".to_string(),
            id: "gemini-3.1-flash-lite".to_string(),
            capabilities: ModelCapabilities::general_text_audio(),
        });
        registry.register(ModelSpec {
            provider: "gemini".to_string(),
            id: "gemini-3.5-flash-lite".to_string(),
            capabilities: ModelCapabilities::translation(),
        });
        registry.register(ModelSpec {
            provider: "gemini".to_string(),
            id: "gemini-3.5-flash".to_string(),
            capabilities: ModelCapabilities::translation(),
        });
        registry.register(ModelSpec {
            provider: "gemini".to_string(),
            id: "gemini-3.1-flash-tts-preview".to_string(),
            capabilities: ModelCapabilities::tts(),
        });

        registry
    }

    pub fn register(&mut self, spec: ModelSpec) {
        self.models
            .insert((spec.provider.clone(), spec.id.clone()), spec);
    }

    pub fn get(&self, provider: &str, model: &str) -> Option<&ModelSpec> {
        self.models.get(&(provider.to_string(), model.to_string()))
    }

    /// Known models are capability-checked. Unknown model IDs are allowed so
    /// new provider releases do not require an application release first.
    pub fn validate(
        &self,
        provider: &str,
        model: &str,
        role: ModelRole,
    ) -> Result<(), DomainError> {
        if provider.trim().is_empty() {
            return Err(DomainError::UnsupportedProvider(
                "provider id cannot be empty".to_string(),
            ));
        }

        if let Some(spec) = self.get(provider, model) {
            if !spec.capabilities.supports(role) {
                return Err(DomainError::UnsupportedModel {
                    role: role.as_str().to_string(),
                    model: model.to_string(),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelRegistry, ModelRole};

    #[test]
    fn known_model_must_match_pipeline_role() {
        let registry = ModelRegistry::standard();

        assert!(registry
            .validate("gemini", "gemini-3.5-transcribe", ModelRole::Transcription)
            .is_ok());
        assert!(registry
            .validate("gemini", "gemini-3.5-transcribe", ModelRole::Tts)
            .is_err());
    }

    #[test]
    fn unknown_models_are_allowed_for_forward_compatibility() {
        let registry = ModelRegistry::standard();
        assert!(registry
            .validate("gemini", "future-model-2027", ModelRole::Tts)
            .is_ok());
    }
}
