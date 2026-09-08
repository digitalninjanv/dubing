use super::client::GeminiClient;
use crate::domain::DomainError;
use reqwest::header::HeaderMap;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct GeminiFileResponse {
    pub file: GeminiFileInfo,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GeminiFileInfo {
    pub name: String,
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: Option<String>,
    pub state: Option<String>,
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
        headers.insert(
            "X-Goog-Upload-Protocol",
            "multipart".parse().map_err(|e| {
                DomainError::Internal(format!("Failed to build upload headers: {}", e))
            })?,
        );

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

        let file_info = parsed.file;
        if file_info.state.as_deref() == Some("PROCESSING") {
            self.wait_for_active(&file_info.name).await
        } else {
            Ok(file_info)
        }
    }

    pub async fn wait_for_active(&self, file_name: &str) -> Result<GeminiFileInfo, DomainError> {
        let url = format!(
            "{}/v1beta/{}?key={}",
            self.client.base_url(),
            file_name,
            self.client.api_key()
        );

        for _ in 0..15 {
            let resp = self.client.http().get(&url).send().await;
            if let Ok(response) = resp {
                if response.status().is_success() {
                    if let Ok(data) = response.json::<GeminiFileResponse>().await {
                        if data.file.state.as_deref() == Some("ACTIVE") {
                            return Ok(data.file);
                        } else if data.file.state.as_deref() == Some("FAILED") {
                            return Err(DomainError::PermanentApiError(
                                "Uploaded audio processing failed on Google servers".to_string(),
                            ));
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }

        // Fallback: fetch final state or return
        let final_resp = self.client.http().get(&url).send().await;
        if let Ok(resp) = final_resp {
            if let Ok(data) = resp.json::<GeminiFileResponse>().await {
                return Ok(data.file);
            }
        }

        Err(DomainError::TransientError(
            "Timed out waiting for audio file processing on Gemini".to_string(),
        ))
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
