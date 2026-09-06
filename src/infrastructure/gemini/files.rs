use super::client::GeminiClient;
use crate::domain::DomainError;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct GeminiFileResponse {
    pub file: GeminiFileInfo,
}

#[derive(Debug, Deserialize)]
pub struct GeminiFileInfo {
    pub name: String,
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: Option<String>,
}

#[derive(Clone)]
pub struct GeminiFilesApi {
    client: GeminiClient,
}

impl GeminiFilesApi {
    pub fn new(client: GeminiClient) -> Self {
        Self { client }
    }

    pub async fn upload_file(
        &self,
        file_path: &Path,
        mime_type: &str,
    ) -> Result<GeminiFileInfo, DomainError> {
        let file_bytes = tokio::fs::read(file_path).await.map_err(|e| {
            DomainError::InvalidAudio(format!("Failed to read audio file for upload: {}", e))
        })?;

        let url = format!(
            "{}/upload/v1beta/files?key={}",
            self.client.base_url(),
            self.client.api_key()
        );

        let mut headers = HeaderMap::new();
        headers.insert("X-Goog-Upload-Protocol", "multipart".parse().unwrap());

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.mp3")
            .to_string();

        let mime_string = mime_type.to_string();

        let response = self
            .client
            .post_with_retry("Gemini File Upload", || {
                let bytes = file_bytes.clone();
                let name = file_name.clone();
                let mime = mime_string.clone();
                let hdrs = headers.clone();
                let target_url = url.clone();

                let http_client = self.client.http().clone();
                async move {
                    let form = reqwest::multipart::Form::new()
                        .text(
                            "metadata",
                            format!(r#"{{"file": {{"displayName": "{}"}}}}"#, name),
                        )
                        .part(
                            "file",
                            reqwest::multipart::Part::bytes(bytes)
                                .file_name(name)
                                .mime_str(&mime)
                                .unwrap_or_else(|_| reqwest::multipart::Part::bytes(Vec::new())),
                        );

                    http_client
                        .post(&target_url)
                        .headers(hdrs)
                        .multipart(form)
                        .send()
                        .await
                }
            })
            .await?;

        let parsed: GeminiFileResponse = response.json().await.map_err(|e| {
            DomainError::PermanentApiError(format!("Failed to parse upload response: {}", e))
        })?;

        Ok(parsed.file)
    }

    pub async fn delete_file(&self, file_name: &str) -> Result<(), DomainError> {
        let url = format!(
            "{}/v1beta/{}?key={}",
            self.client.base_url(),
            file_name,
            self.client.api_key()
        );

        let _ = reqwest::Client::new().delete(&url).send().await;
        Ok(())
    }
}
