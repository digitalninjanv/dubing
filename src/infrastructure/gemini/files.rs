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
        // Validate file exists and is non-empty without loading it fully into RAM.
        let meta = tokio::fs::metadata(file_path).await.map_err(|e| {
            DomainError::InvalidAudio(format!("Failed to stat audio file for upload: {}", e))
        })?;
        if meta.len() == 0 {
            return Err(DomainError::InvalidAudio(
                "Audio file is empty (0 bytes)".to_string(),
            ));
        }

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
        let path_owned = file_path.to_path_buf();

        // Streaming upload with manual retry (F1): reopen file per attempt
        // via ReaderStream -> Part::stream, avoiding a full clone of the
        // file buffer on every retry (reqwest docs: seanmonstar/reqwest
        // stream file upload). We retry via the client's backoffs.
        let backoffs = [3u64, 8, 15, 25, 35];
        let mut last_err: Option<crate::domain::DomainError> = None;
        let mut response_opt: Option<reqwest::Response> = None;
        for (attempt, delay_secs) in backoffs.iter().enumerate() {
            let file = match tokio::fs::File::open(&path_owned).await {
                Ok(f) => f,
                Err(e) => {
                    return Err(crate::domain::DomainError::InvalidAudio(format!(
                        "Failed to open audio file for upload: {}",
                        e
                    )))
                }
            };
            let stream = tokio_util::io::ReaderStream::new(file);
            let body = reqwest::Body::wrap_stream(stream);
            let part = reqwest::multipart::Part::stream(body)
                .file_name(file_name.clone())
                .mime_str(&mime_string)
                .unwrap_or_else(|_| reqwest::multipart::Part::stream(reqwest::Body::from(vec![])));
            let form = reqwest::multipart::Form::new()
                .text(
                    "metadata",
                    format!(r#"{{"file": {{"displayName": "{}"}}}}"#, file_name),
                )
                .part("file", part);
            let res = self
                .client
                .http()
                .post(&url)
                .headers(headers.clone())
                .multipart(form)
                .send()
                .await;
            match res {
                Ok(resp) if resp.status().is_success() => {
                    response_opt = Some(resp);
                    break;
                }
                Ok(resp)
                    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                        || resp.status().is_server_error() =>
                {
                    let status = resp.status();
                    tracing::warn!(
                        "Gemini File Upload retryable status {} on attempt {}/{}; waiting {}s",
                        status,
                        attempt + 1,
                        backoffs.len(),
                        delay_secs
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(*delay_secs)).await;
                    last_err = Some(crate::domain::DomainError::TransientError(format!(
                        "Gemini File Upload status {}",
                        status
                    )));
                    continue;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    let snip = if body.len() > 600 {
                        format!("{}… ({} bytes truncated)", &body[..600], body.len() - 600)
                    } else {
                        body
                    };
                    return Err(crate::domain::DomainError::PermanentApiError(format!(
                        "File upload failed with status {}: {}",
                        status, snip
                    )));
                }
                Err(e) if e.is_timeout() || e.is_connect() => {
                    tracing::warn!(
                        "File upload network error on attempt {}/{}: {}; retrying in {}s",
                        attempt + 1,
                        backoffs.len(),
                        e,
                        delay_secs
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(*delay_secs)).await;
                    last_err = Some(crate::domain::DomainError::TransientError(format!(
                        "upload network: {}",
                        e
                    )));
                    continue;
                }
                Err(e) => {
                    return Err(crate::domain::DomainError::PermanentApiError(format!(
                        "upload failed: {}",
                        e
                    )));
                }
            }
        }
        let response = response_opt.ok_or_else(|| {
            last_err.unwrap_or_else(|| {
                crate::domain::DomainError::TransientError(
                    "File upload max retries exceeded".to_string(),
                )
            })
        })?;

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

        // Reuse the pooled client (connection reuse) and send the key via
        // header as well so server-side URL logs never see the secret alone.
        let api_key = self.client.api_key().to_string();
        let _ = self
            .client
            .http()
            .delete(&url)
            .header("x-goog-api-key", api_key)
            .send()
            .await;
        Ok(())
    }
}
