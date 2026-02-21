//! Resilience utilities — Retry with exponential backoff
//!
//! Provides a resilient HTTP client wrapper that handles transient failures
//! with configurable retry logic, exponential backoff, and jitter.

use reqwest::{Client, Response, StatusCode};
use std::time::Duration;
use tracing::{debug, warn};

use crate::provider::ProviderError;

// ─── Retry Configuration ───────────────────────────────────────────

/// Configuration for retry behavior on transient failures
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (0 = no retries)
    pub max_retries: u32,
    /// Initial backoff delay in milliseconds
    pub initial_backoff_ms: u64,
    /// Maximum backoff delay in milliseconds
    pub max_backoff_ms: u64,
    /// Multiplier applied to backoff after each retry
    pub backoff_multiplier: f64,
    /// Whether to add random jitter to prevent thundering herd
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff_ms: 1000,
            max_backoff_ms: 30_000,
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// No retries — fail immediately
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Aggressive retry for critical operations
    pub fn aggressive() -> Self {
        Self {
            max_retries: 5,
            initial_backoff_ms: 500,
            max_backoff_ms: 60_000,
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

// ─── Resilient Client ──────────────────────────────────────────────

/// HTTP client wrapper with retry logic and exponential backoff
pub struct ResilientClient {
    client: Client,
    config: RetryConfig,
}

impl ResilientClient {
    pub fn new(config: RetryConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client, config }
    }

    /// Get a reference to the inner reqwest Client (for building requests)
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Execute a request with retry logic
    ///
    /// Retries on transient errors (429, 5xx, timeouts) with exponential
    /// backoff and optional jitter. Non-retryable errors (4xx except 429)
    /// fail immediately.
    pub async fn execute(
        &self,
        request_builder: reqwest::RequestBuilder,
    ) -> Result<Response, ProviderError> {
        let mut last_error =
            ProviderError::RequestFailed("No attempts made".to_string());

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                let delay = self.compute_backoff(attempt);
                debug!(
                    attempt = attempt,
                    delay_ms = delay.as_millis() as u64,
                    "Retrying request after backoff"
                );
                tokio::time::sleep(delay).await;
            }

            // We need to clone the request builder for each attempt.
            // Since RequestBuilder can't be cloned, we use try_clone on the built request.
            let result = match request_builder.try_clone() {
                Some(cloned) => cloned.send().await,
                None => {
                    // If we can't clone (e.g., streaming body), try once
                    if attempt == 0 {
                        return request_builder.send().await.map_err(|e| {
                            ProviderError::RequestFailed(e.to_string())
                        });
                    } else {
                        return Err(last_error);
                    }
                }
            };

            match result {
                Ok(response) => {
                    let status = response.status();

                    if status.is_success() {
                        return Ok(response);
                    }

                    if Self::is_retryable_status(status) {
                        // Check for Retry-After header on 429
                        if status == StatusCode::TOO_MANY_REQUESTS {
                            if let Some(retry_after) =
                                self.parse_retry_after(&response)
                            {
                                debug!(
                                    retry_after_secs = retry_after.as_secs(),
                                    "Respecting Retry-After header"
                                );
                                tokio::time::sleep(retry_after).await;
                            }
                        }

                        let body = response.text().await.unwrap_or_default();
                        last_error = ProviderError::ApiError(format!(
                            "HTTP {} (attempt {}/{}): {}",
                            status.as_u16(),
                            attempt + 1,
                            self.config.max_retries + 1,
                            truncate_body(&body, 200)
                        ));
                        warn!(
                            status = status.as_u16(),
                            attempt = attempt + 1,
                            max = self.config.max_retries + 1,
                            "Retryable error, will retry"
                        );
                        continue;
                    }

                    // Non-retryable HTTP error — fail immediately
                    let body = response.text().await.unwrap_or_default();
                    return Err(ProviderError::ApiError(format!(
                        "HTTP {}: {}",
                        status.as_u16(),
                        truncate_body(&body, 500)
                    )));
                }
                Err(e) => {
                    if e.is_timeout() || e.is_connect() {
                        last_error = ProviderError::RequestFailed(format!(
                            "Network error (attempt {}/{}): {}",
                            attempt + 1,
                            self.config.max_retries + 1,
                            e
                        ));
                        warn!(
                            attempt = attempt + 1,
                            max = self.config.max_retries + 1,
                            error = %e,
                            "Transient network error, will retry"
                        );
                        continue;
                    }

                    // Non-transient error — fail immediately
                    return Err(ProviderError::RequestFailed(e.to_string()));
                }
            }
        }

        // All retries exhausted
        Err(last_error)
    }

    /// Compute backoff delay for a given attempt number
    pub fn compute_backoff(&self, attempt: u32) -> Duration {
        let base_ms = self.config.initial_backoff_ms as f64
            * self.config.backoff_multiplier.powi(attempt as i32 - 1);
        let capped_ms = base_ms.min(self.config.max_backoff_ms as f64);

        let final_ms = if self.config.jitter {
            // Simple jitter: random value between 0 and capped_ms
            // Using a basic PRNG seeded from attempt + time to avoid rand dependency
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as f64;
            let jitter_factor = (seed % 1000.0) / 1000.0;
            capped_ms * jitter_factor
        } else {
            capped_ms
        };

        Duration::from_millis(final_ms.max(100.0) as u64)
    }

    /// Check if an HTTP status code indicates a transient/retryable error
    pub fn is_retryable_status(status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS          // 429
            | StatusCode::INTERNAL_SERVER_ERROR     // 500
            | StatusCode::BAD_GATEWAY               // 502
            | StatusCode::SERVICE_UNAVAILABLE       // 503
            | StatusCode::GATEWAY_TIMEOUT           // 504
            | StatusCode::REQUEST_TIMEOUT // 408
        )
    }

    /// Parse the Retry-After header value
    fn parse_retry_after(&self, response: &Response) -> Option<Duration> {
        response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
    }
}

/// Truncate a string for error messages
fn truncate_body(body: &str, max_len: usize) -> &str {
    if body.len() <= max_len {
        body
    } else {
        &body[..max_len]
    }
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_defaults() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_backoff_ms, 1000);
        assert_eq!(config.max_backoff_ms, 30_000);
        assert_eq!(config.backoff_multiplier, 2.0);
        assert!(config.jitter);
    }

    #[test]
    fn test_retry_config_none() {
        let config = RetryConfig::none();
        assert_eq!(config.max_retries, 0);
    }

    #[test]
    fn test_retry_config_aggressive() {
        let config = RetryConfig::aggressive();
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.initial_backoff_ms, 500);
    }

    #[test]
    fn test_is_retryable_status() {
        assert!(ResilientClient::is_retryable_status(
            StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(ResilientClient::is_retryable_status(
            StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(ResilientClient::is_retryable_status(
            StatusCode::BAD_GATEWAY
        ));
        assert!(ResilientClient::is_retryable_status(
            StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(ResilientClient::is_retryable_status(
            StatusCode::GATEWAY_TIMEOUT
        ));
        assert!(ResilientClient::is_retryable_status(
            StatusCode::REQUEST_TIMEOUT
        ));

        // Non-retryable
        assert!(!ResilientClient::is_retryable_status(
            StatusCode::BAD_REQUEST
        ));
        assert!(!ResilientClient::is_retryable_status(
            StatusCode::UNAUTHORIZED
        ));
        assert!(!ResilientClient::is_retryable_status(StatusCode::FORBIDDEN));
        assert!(!ResilientClient::is_retryable_status(StatusCode::NOT_FOUND));
        assert!(!ResilientClient::is_retryable_status(StatusCode::OK));
    }

    #[test]
    fn test_backoff_computation_no_jitter() {
        let client = ResilientClient::new(RetryConfig {
            jitter: false,
            initial_backoff_ms: 1000,
            backoff_multiplier: 2.0,
            max_backoff_ms: 30_000,
            ..Default::default()
        });

        // Attempt 1: 1000ms
        let d1 = client.compute_backoff(1);
        assert_eq!(d1.as_millis(), 1000);

        // Attempt 2: 2000ms
        let d2 = client.compute_backoff(2);
        assert_eq!(d2.as_millis(), 2000);

        // Attempt 3: 4000ms
        let d3 = client.compute_backoff(3);
        assert_eq!(d3.as_millis(), 4000);
    }

    #[test]
    fn test_backoff_capped_at_max() {
        let client = ResilientClient::new(RetryConfig {
            jitter: false,
            initial_backoff_ms: 10_000,
            backoff_multiplier: 10.0,
            max_backoff_ms: 30_000,
            ..Default::default()
        });

        // Attempt 2: 10000 * 10 = 100_000 → capped at 30_000
        let d = client.compute_backoff(2);
        assert_eq!(d.as_millis(), 30_000);
    }

    #[test]
    fn test_backoff_with_jitter_bounded() {
        let client = ResilientClient::new(RetryConfig {
            jitter: true,
            initial_backoff_ms: 1000,
            backoff_multiplier: 2.0,
            max_backoff_ms: 30_000,
            ..Default::default()
        });

        // With jitter, result should be between 100ms (min) and 1000ms (max for attempt 1)
        let d = client.compute_backoff(1);
        assert!(d.as_millis() >= 100);
        assert!(d.as_millis() <= 1000);
    }

    #[test]
    fn test_truncate_body() {
        assert_eq!(truncate_body("short", 10), "short");
        assert_eq!(truncate_body("hello world", 5), "hello");
    }

    #[test]
    fn test_resilient_client_creation() {
        let client = ResilientClient::new(RetryConfig::default());
        // Just verify it doesn't panic
        let _ = client.inner();
    }
}
