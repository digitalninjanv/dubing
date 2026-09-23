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
            capabilities: ModelCapabilities::general_text_audio(),
        });
        registry.register(ModelSpec {
            provider: "gemini".to_string(),
            id: "gemini-3.8-flash".to_string(),
            capabilities: ModelCapabilities::general_text_audio(),
        });
        registry.register(ModelSpec {
            provider: "gemini".to_string(),
            id: "gemini-3.1-flash-tts-preview".to_string(),
            capabilities: ModelCapabilities::tts(),
        });
        registry.register(ModelSpec {
            provider: "gemini".to_string(),
            id: "gemini-2.5-flash-preview-tts".to_string(),
            capabilities: ModelCapabilities::tts(),
        });
        registry.register(ModelSpec {
            provider: "gemini".to_string(),
            id: "gemini-2.5-pro-preview-tts".to_string(),
            capabilities: ModelCapabilities::tts(),
        });

        for (id, capabilities) in [
            ("gpt-4o-transcribe", ModelCapabilities::transcription()),
            ("gpt-4o-mini-transcribe", ModelCapabilities::transcription()),
            ("gpt-5", ModelCapabilities::translation()),
            ("gpt-4o-mini-tts", ModelCapabilities::tts()),
        ] {
            registry.register(ModelSpec {
                provider: "openai".to_string(),
                id: id.to_string(),
                capabilities,
            });
        }

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

        let normalized_provider = provider.to_ascii_lowercase();
        if let Some(spec) = self.get(&normalized_provider, model) {
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

    #[test]
    fn known_models_cover_current_fallback_chain() {
        let registry = ModelRegistry::standard();
        for model in [
            "gemini-3.8-flash",
            "gemini-2.5-flash-preview-tts",
            "gemini-2.5-pro-preview-tts",
        ] {
            let role = if model.contains("tts") {
                ModelRole::Tts
            } else {
                ModelRole::Translation
            };
            assert!(registry.validate("gemini", model, role).is_ok(), "{model}");
        }
    }

    #[test]
    fn known_openai_models_match_pipeline_roles() {
        let registry = ModelRegistry::standard();
        assert!(registry
            .validate("openai", "gpt-4o-transcribe", ModelRole::Transcription)
            .is_ok());
        assert!(registry
            .validate("openai", "gpt-5", ModelRole::Translation)
            .is_ok());
        assert!(registry
            .validate("openai", "gpt-4o-mini-tts", ModelRole::Tts)
            .is_ok());
        assert!(registry
            .validate("openai", "gpt-4o-mini-tts", ModelRole::Transcription)
            .is_err());
    }

    #[test]
    fn provider_validation_is_case_insensitive_for_known_models() {
        let registry = ModelRegistry::standard();
        assert!(registry
            .validate("GEMINI", "gemini-3.8-flash", ModelRole::Translation)
            .is_ok());
        assert!(registry
            .validate("Gemini", "gemini-3.5-transcribe", ModelRole::Transcription)
            .is_ok());
    }
}
