use crate::domain::DomainError;
use reqwest::{Client, Response, StatusCode};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub type RetryStatusCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Shared HTTP client: one connection pool per process instead of a fresh
/// TLS/pool setup per `GeminiClient::new` (which is built per job/click).
fn shared_http_client() -> Client {
    use std::sync::OnceLock;
    static HTTP: OnceLock<Client> = OnceLock::new();
    HTTP.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(120))
            .tcp_nodelay(true)
            .tcp_keepalive(Duration::from_secs(60))
            .pool_max_idle_per_host(25)
            .pool_idle_timeout(Duration::from_secs(120))
            .http2_adaptive_window(true)
            .build()
            .unwrap_or_default()
    })
    .clone()
}

#[derive(Clone)]
pub struct GeminiClient {
    http: Client,
    api_key: String,
    base_url: String,
    backoffs: Vec<u64>,
    status_callback: Option<RetryStatusCallback>,
    cancel_token: Option<CancellationToken>,
}

impl GeminiClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: shared_http_client(),
            api_key: api_key.into(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            backoffs: vec![3, 8, 15, 25, 35],
            status_callback: None,
            cancel_token: None,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_backoffs(mut self, backoffs: Vec<u64>) -> Self {
        self.backoffs = backoffs;
        self
    }

    pub fn with_status_callback(mut self, cb: RetryStatusCallback) -> Self {
        self.status_callback = Some(cb);
        self
    }

    /// Attach a cancellation token so retry/backoff sleeps abort instantly
    /// on user cancel instead of hanging up to 35s.
    pub fn with_cancel_token(mut self, token: CancellationToken) -> Self {
        self.cancel_token = Some(token);
        self
    }

    pub fn set_status_callback(&mut self, cb: RetryStatusCallback) {
        self.status_callback = Some(cb);
    }

    pub fn status_callback(&self) -> Option<&RetryStatusCallback> {
        self.status_callback.as_ref()
    }

    pub fn http(&self) -> &Client {
        &self.http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Non-destructive 0-token validation of the API key by querying /v1beta/models
    pub async fn test_connection(&self) -> Result<Vec<String>, DomainError> {
        if self.api_key.trim().is_empty() {
            return Err(DomainError::AuthenticationFailed);
        }

        let url = format!("{}/v1beta/models", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("x-goog-api-key", self.api_key.clone())
            .send()
            .await
            .map_err(|e| {
                DomainError::TransientError(format!("Network connection failed: {}", e))
            })?;

        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(DomainError::AuthenticationFailed);
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(DomainError::TransientError(
                "Gemini API rate limit or quota exceeded (HTTP 429)".to_string(),
            ));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(DomainError::Internal(format!(
                "Gemini API returned HTTP {}: {}",
                status, body
            )));
        }

        #[derive(serde::Deserialize)]
        struct ModelItem {
            name: String,
        }
        #[derive(serde::Deserialize)]
        struct ModelsListResponse {
            models: Option<Vec<ModelItem>>,
        }

        let parsed: ModelsListResponse = resp
            .json()
            .await
            .map_err(|e| DomainError::Internal(format!("Failed to parse models JSON: {}", e)))?;

        let model_names = parsed
            .models
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.name.replace("models/", ""))
            .collect();

        Ok(model_names)
    }

    /// Retry with optional cancellation (F10). When a token is supplied,
    /// the per-second countdown uses `select!` so a user Cancel resolves
    /// instantly instead of waiting the full 3–35s backoff.
    pub async fn post_with_retry<F, Fut>(
        &self,
        operation_name: &str,
        make_request: F,
    ) -> Result<Response, DomainError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<Response, reqwest::Error>>,
    {
        let cancel = self.cancel_token.clone();
        self.post_with_retry_cancel(operation_name, make_request, cancel)
            .await
    }

    pub async fn post_with_retry_cancel<F, Fut>(
        &self,
        operation_name: &str,
        make_request: F,
        cancel: Option<CancellationToken>,
    ) -> Result<Response, DomainError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<Response, reqwest::Error>>,
    {
        let mut last_error = None;
        let total_attempts = self.backoffs.len();

        for (attempt, delay_secs) in self.backoffs.iter().enumerate() {
            let current_attempt = attempt + 1;
            match make_request().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        return Ok(response);
                    }

                    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                        return Err(DomainError::AuthenticationFailed);
                    }

                    // Check for rate limit or server error (retryable)
                    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                        // Respect Retry-After header if provided by Google Gateway
                        let retry_after_secs = response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0);

                        let base_delay = if retry_after_secs > 0 {
                            retry_after_secs.max(*delay_secs)
                        } else {
                            *delay_secs
                        };

                        let total_sleep_ms = if base_delay > 0 {
                            // Add small jitter (100ms - 500ms) to eliminate thundering herd / retry stampedes
                            let jitter_ms = (std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .subsec_nanos()
                                as u64
                                % 400)
                                + 100;
                            base_delay * 1000 + jitter_ms
                        } else {
                            0
                        };

                        let sleep_duration = Duration::from_millis(total_sleep_ms);

                        warn!(
                            "Gemini API returned retryable status {} on attempt {}/{} for {}. Retrying in {:.2}s...",
                            status,
                            current_attempt,
                            total_attempts,
                            operation_name,
                            sleep_duration.as_secs_f64()
                        );

                        if total_sleep_ms > 0 {
                            let mut remaining_ms = total_sleep_ms;
                            while remaining_ms > 0 {
                                if let Some(ref tok) = cancel {
                                    if tok.is_cancelled() {
                                        return Err(DomainError::Cancelled);
                                    }
                                }
                                let remaining_secs = remaining_ms.div_ceil(1000);
                                if let Some(ref cb) = self.status_callback {
                                    cb(&format!(
                                        "Gemini rate limit ({}): Waiting {}s for quota reset (attempt {}/{})...",
                                        status, remaining_secs, current_attempt, total_attempts
                                    ));
                                }
                                let step = remaining_ms.min(1000);
                                if let Some(ref tok) = cancel {
                                    tokio::select! {
                                        _ = tok.cancelled() => return Err(DomainError::Cancelled),
                                        _ = tokio::time::sleep(Duration::from_millis(step)) => {},
                                    }
                                } else {
                                    tokio::time::sleep(Duration::from_millis(step)).await;
                                }
                                remaining_ms = remaining_ms.saturating_sub(step);
                            }

                            if let Some(ref cb) = self.status_callback {
                                cb(&format!(
                                    "Retrying {} (attempt {}/{})...",
                                    operation_name,
                                    current_attempt + 1,
                                    total_attempts
                                ));
                            }
                        }

                        last_error = Some(DomainError::TransientError(format!(
                            "Gemini API returned status {}",
                            status
                        )));
                        continue;
                    }

                    // Permanent 4xx client errors — truncate body to avoid
                    // leaking large transcripts/prompts into logs (F10).
                    let body = response.text().await.unwrap_or_default();
                    let body_snip = if body.len() > 600 {
                        format!("{}… ({} bytes truncated)", &body[..600], body.len() - 600)
                    } else {
                        body
                    };
                    return Err(DomainError::PermanentApiError(format!(
                        "API request failed with status {}: {}",
                        status, body_snip
                    )));
                }
                Err(err) => {
                    if err.is_timeout() || err.is_connect() {
                        let total_sleep_ms = delay_secs * 1000;
                        warn!(
                            "Network timeout/connect error on attempt {}/{} for {}: {}. Retrying in {}s...",
                            current_attempt, total_attempts, operation_name, err, delay_secs
                        );

                        if total_sleep_ms > 0 {
                            let mut remaining_ms = total_sleep_ms;
                            while remaining_ms > 0 {
                                if let Some(ref tok) = cancel {
                                    if tok.is_cancelled() {
                                        return Err(DomainError::Cancelled);
                                    }
                                }
                                let remaining_secs = remaining_ms.div_ceil(1000);
                                if let Some(ref cb) = self.status_callback {
                                    cb(&format!(
                                        "Network connection issue: Waiting {}s before retry (attempt {}/{})...",
                                        remaining_secs, current_attempt, total_attempts
                                    ));
                                }
                                let step = remaining_ms.min(1000);
                                if let Some(ref tok) = cancel {
                                    tokio::select! {
                                        _ = tok.cancelled() => return Err(DomainError::Cancelled),
                                        _ = tokio::time::sleep(Duration::from_millis(step)) => {},
                                    }
                                } else {
                                    tokio::time::sleep(Duration::from_millis(step)).await;
                                }
                                remaining_ms = remaining_ms.saturating_sub(step);
                            }

                            if let Some(ref cb) = self.status_callback {
                                cb(&format!(
                                    "Reconnecting {} (attempt {}/{})...",
                                    operation_name,
                                    current_attempt + 1,
                                    total_attempts
                                ));
                            }
                        }

                        last_error = Some(DomainError::TransientError(format!(
                            "Network error: {}",
                            err
                        )));
                        continue;
                    }

                    return Err(DomainError::PermanentApiError(format!(
                        "Network failure: {}",
                        err
                    )));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            DomainError::TransientError("Max retry attempts exceeded".to_string())
        }))
    }
}
