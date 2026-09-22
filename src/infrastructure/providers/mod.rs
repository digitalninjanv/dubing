pub mod model_registry;

use self::model_registry::{ModelRegistry, ModelRole};
use crate::application::ports::{SpeechSynthesizer, SpeechTranscriber, TextTranslator};
use crate::config::AppSettings;
use crate::domain::DomainError;
use crate::infrastructure::gemini::{
    GeminiClient, GeminiSynthesizer, GeminiTranscriber, GeminiTranslator,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct PipelineProviders {
    pub transcriber: Arc<dyn SpeechTranscriber>,
    pub translator: Arc<dyn TextTranslator>,
    pub synthesizer: Arc<dyn SpeechSynthesizer>,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderRegistry {
    models: ModelRegistry,
}

impl ProviderRegistry {
    pub fn standard() -> Self {
        Self {
            models: ModelRegistry::standard(),
        }
    }

    pub fn build(
        &self,
        settings: &AppSettings,
        api_key: impl Into<String>,
        cancel_token: Option<CancellationToken>,
        status_callback: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    ) -> Result<PipelineProviders, DomainError> {
        self.validate_provider(&settings.providers.transcriber)?;
        self.validate_provider(&settings.providers.translator)?;
        self.validate_provider(&settings.providers.tts)?;

        self.models.validate(
            &settings.providers.transcriber,
            &settings.models.transcriber,
            ModelRole::Transcription,
        )?;
        self.models.validate(
            &settings.providers.translator,
            &settings.models.translator,
            ModelRole::Translation,
        )?;
        self.models.validate(
            &settings.providers.tts,
            &settings.models.tts,
            ModelRole::Tts,
        )?;

        for fallback in &settings.models.transcriber_fallbacks {
            self.models.validate(
                &settings.providers.transcriber,
                fallback,
                ModelRole::Transcription,
            )?;
        }
        for fallback in &settings.models.translator_fallbacks {
            self.models.validate(
                &settings.providers.translator,
                fallback,
                ModelRole::Translation,
            )?;
        }
        for fallback in &settings.models.tts_fallbacks {
            self.models
                .validate(&settings.providers.tts, fallback, ModelRole::Tts)?;
        }

        let mut client = GeminiClient::new(api_key);
        if let Some(token) = cancel_token {
            client = client.with_cancel_token(token);
        }
        if let Some(callback) = status_callback {
            client = client.with_status_callback(callback);
        }

        Ok(PipelineProviders {
            transcriber: Arc::new(GeminiTranscriber::new_with_fallbacks(
                client.clone(),
                settings.models.transcriber.clone(),
                settings.models.transcriber_fallbacks.clone(),
            )),
            translator: Arc::new(GeminiTranslator::new_with_fallbacks(
                client.clone(),
                settings.models.translator.clone(),
                settings.models.translator_fallbacks.clone(),
            )),
            synthesizer: Arc::new(GeminiSynthesizer::new_with_fallbacks(
                client,
                settings.models.tts.clone(),
                settings.models.tts_fallbacks.clone(),
            )),
        })
    }

    fn validate_provider(&self, provider: &str) -> Result<(), DomainError> {
        if provider.eq_ignore_ascii_case("gemini") {
            Ok(())
        } else {
            Err(DomainError::UnsupportedProvider(provider.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderRegistry;
    use crate::config::AppSettings;

    #[test]
    fn default_provider_configuration_is_buildable() {
        let registry = ProviderRegistry::standard();
        let settings = AppSettings::default();
        let result = registry.build(&settings, "test-key", None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn unknown_provider_is_rejected_before_network_use() {
        let registry = ProviderRegistry::standard();
        let mut settings = AppSettings::default();
        settings.providers.translator = "unknown".to_string();
        let result = registry.build(&settings, "test-key", None, None);
        assert!(result.is_err());
    }
}
