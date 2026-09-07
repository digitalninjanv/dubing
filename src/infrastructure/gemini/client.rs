use crate::domain::DomainError;
use reqwest::{Client, Response, StatusCode};
use std::time::Duration;
use tracing::warn;

#[derive(Clone)]
pub struct GeminiClient {
    http: Client,
    api_key: String,
    base_url: String,
    backoffs: Vec<u64>,
}

impl GeminiClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(120))
                .tcp_nodelay(true)
                .tcp_keepalive(Duration::from_secs(60))
                .pool_max_idle_per_host(25)
                .pool_idle_timeout(Duration::from_secs(120))
                .http2_adaptive_window(true)
                .build()
                .unwrap_or_default(),
            api_key: api_key.into(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            backoffs: vec![2, 5, 10, 20],
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

    pub fn http(&self) -> &Client {
        &self.http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub async fn post_with_retry<F, Fut>(
        &self,
        operation_name: &str,
        make_request: F,
    ) -> Result<Response, DomainError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<Response, reqwest::Error>>,
    {
        let mut last_error = None;

        for (attempt, delay_secs) in self.backoffs.iter().enumerate() {
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

                        // Add random jitter (250ms - 1250ms) to eliminate thundering herd / retry stampedes
                        let jitter_ms = (std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .subsec_nanos() as u64
                            % 1000)
                            + 250;
                        let sleep_duration = Duration::from_millis(base_delay * 1000 + jitter_ms);

                        warn!(
                            "Gemini API returned retryable status {} on attempt {} for {}. Retrying in {:.2}s...",
                            status,
                            attempt + 1,
                            operation_name,
                            sleep_duration.as_secs_f64()
                        );
                        if sleep_duration.as_millis() > 0 {
                            tokio::time::sleep(sleep_duration).await;
                        }
                        last_error = Some(DomainError::TransientError(format!(
                            "Gemini API returned status {}",
                            status
                        )));
                        continue;
                    }

                    // Permanent 4xx client errors
                    let body = response.text().await.unwrap_or_default();
                    return Err(DomainError::PermanentApiError(format!(
                        "API request failed with status {}: {}",
                        status, body
                    )));
                }
                Err(err) => {
                    if err.is_timeout() || err.is_connect() {
                        warn!(
                            "Network timeout/connect error on attempt {} for {}: {}. Retrying in {}s...",
                            attempt + 1, operation_name, err, delay_secs
                        );
                        if *delay_secs > 0 {
                            tokio::time::sleep(Duration::from_secs(*delay_secs)).await;
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
