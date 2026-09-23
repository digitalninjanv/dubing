pub mod model_registry;

use self::model_registry::{ModelRegistry, ModelRole};
use crate::application::ports::{SpeechSynthesizer, SpeechTranscriber, TextTranslator};
use crate::config::AppSettings;
use crate::domain::DomainError;
use crate::infrastructure::gemini::{
    GeminiClient, GeminiSynthesizer, GeminiTranscriber, GeminiTranslator,
};
use crate::infrastructure::openai::{
    OpenAiClient, OpenAiSynthesizer, OpenAiTranscriber, OpenAiTranslator,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type ProviderStatusCallback = Arc<dyn Fn(&str) + Send + Sync>;

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
        status_callback: Option<ProviderStatusCallback>,
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

        let api_key = api_key.into();
        let needs_gemini = [
            &settings.providers.transcriber,
            &settings.providers.translator,
            &settings.providers.tts,
        ]
        .iter()
        .any(|provider| provider.eq_ignore_ascii_case("gemini"));
        if needs_gemini && api_key.trim().is_empty() {
            return Err(DomainError::AuthenticationFailed);
        }

        let gemini_client = {
            let mut client = GeminiClient::new(api_key);
            if let Some(token) = cancel_token.clone() {
                client = client.with_cancel_token(token);
            }
            if let Some(callback) = status_callback.clone() {
                client = client.with_status_callback(callback);
            }
            client
        };

        let openai_key = std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty());
        let openai_client = openai_key.map(OpenAiClient::new);

        let transcriber: Arc<dyn SpeechTranscriber> =
            match settings.providers.transcriber.to_ascii_lowercase().as_str() {
                "gemini" => Arc::new(GeminiTranscriber::new_with_fallbacks(
                    gemini_client.clone(),
                    settings.models.transcriber.clone(),
                    settings.models.transcriber_fallbacks.clone(),
                )),
                "openai" => Arc::new(OpenAiTranscriber::new(
                    openai_client
                        .clone()
                        .ok_or_else(|| DomainError::AuthenticationFailed)?,
                    settings.models.transcriber.clone(),
                )),
                provider => return Err(DomainError::UnsupportedProvider(provider.to_string())),
            };

        let translator: Arc<dyn TextTranslator> =
            match settings.providers.translator.to_ascii_lowercase().as_str() {
                "gemini" => Arc::new(GeminiTranslator::new_with_fallbacks_and_limits(
                    gemini_client.clone(),
                    settings.models.translator.clone(),
                    settings.models.translator_fallbacks.clone(),
                    settings.runtime.translation_batch_size,
                    settings.runtime.translation_concurrency,
                )),
                "openai" => Arc::new(OpenAiTranslator::new(
                    openai_client
                        .clone()
                        .ok_or_else(|| DomainError::AuthenticationFailed)?,
                    settings.models.translator.clone(),
                )),
                provider => return Err(DomainError::UnsupportedProvider(provider.to_string())),
            };

        let synthesizer: Arc<dyn SpeechSynthesizer> =
            match settings.providers.tts.to_ascii_lowercase().as_str() {
                "gemini" => Arc::new(GeminiSynthesizer::new_with_fallbacks(
                    gemini_client,
                    settings.models.tts.clone(),
                    settings.models.tts_fallbacks.clone(),
                )),
                "openai" => Arc::new(OpenAiSynthesizer::new(
                    openai_client.ok_or_else(|| DomainError::AuthenticationFailed)?,
                    settings.models.tts.clone(),
                )),
                provider => return Err(DomainError::UnsupportedProvider(provider.to_string())),
            };

        Ok(PipelineProviders {
            transcriber,
            translator,
            synthesizer,
        })
    }

    fn validate_provider(&self, provider: &str) -> Result<(), DomainError> {
        if provider.eq_ignore_ascii_case("gemini") || provider.eq_ignore_ascii_case("openai") {
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
