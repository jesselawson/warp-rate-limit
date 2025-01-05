#![forbid(unsafe_code)]
//! This crate provides RFC 6585 compliant in-memory rate limiting with 
//! configurable windows and limits as lightweight middleware for 
//! Warp web applications.
//! 
//! It provides a Filter you add to your routes that exposes rate-limiting
//! information to your handlers, and a Rejection Type for error recovery.
//! 
//! It does not yet provide persistence, nor is the HashMap that stores IPs
//! bounded. Both of these may be changed in a future version. 
//! 
//! # Quickstart
//! 
//! 1. Include the crate:
//! 
//! `cargo add warp-rate-limit`
//! 
//! 2. Define one or more rate limit configurations. Following are some 
//! examples of available builder methods. The variable names are arbitrary: 
//! 
//! ```rust,no_run,ignore
//! // Limit: 60 requests per 60 Earth seconds
//! let public_routes_rate_limit = RateLimitConfig::default();
//! 
//! // Limit: 100 requests per 60 Earth seconds
//! let parter_routes_rate_limit = RateLimitConfig::max_per_minute(100);
//! 
//! // Limit: 10 requests per 20 Earth seconds
//! let static_route_limit = RateLimitConfig::max_per_window(10,20);
//! ```
//! 
//! 3. Use rate limiting information in request handler. If you don't want 
//! to use rate-limiting information related to the IP address associated 
//! with this request, you can skip this part. 
//! 
//! ```rust,no_run,ignore
//! // Example route handler
//! async fn hande_request(rate_limit_info: RateLimitInfo) -> Result<impl Reply, Rejection> {
//!     // Create a base response
//!     let mut response = warp::reply::with_status(
//!         "Hello world", 
//!         StatusCode::OK
//!     ).into_response();
//! 
//!     // Optionally add rate limit headers to your response.
//!     if let Err(e) = add_rate_limit_headers(response.headers_mut(), &rate_limit_info) {
//!         match e {
//!             RateLimitError::HeaderError(e) => {
//!                 eprintln!("Failed to set rate limit headers due to invalid value: {}", e);
//!             }
//!             RateLimitError::Other(e) => {
//!                 eprintln!("Unexpected error setting rate limit headers: {}", e);
//!             }
//!         }
//!     } 
//! 
//!     // You could also replace the above `if let Err(e)` block with:
//!     // let _ = add_rate_limit_headers(response.headers_mut(), &rate_limit_info);
//! 
//!     Ok(response)
//! }
//! ```
//! 
//! 4. Handle rate limit errors in your rejection handler: 
//! 
//! ```rust,no_run,ignore
//! // Example rejection handler
//! async fn handle_rejection(rejection: Rejection) -> Result<impl Reply, Infallible> {
//!     // Somewhere in your rejection handling:
//!     if let Some(rate_limit_rejection) = rejection.find::<RateLimitRejection>() {
//!         // We have a rate limit rejection -- so let's get some info about it: 
//!         let info = get_rate_limit_info(rate_limit_rejection);
//! 
//!         // Let's use that info to create a response:
//!         let message = format!(
//!             "Rate limit exceeded. Try again after {}.", 
//!             info.retry_after
//!         );
//! 
//!         // Let's build that response:
//!         let mut response = warp::reply::with_status(
//!             message,
//!             StatusCode::TOO_MANY_REQUESTS
//!         ).into_response();
//! 
//!         // Then, let's add the rate-limiting headers to that response:
//!         if let Err(e) = add_rate_limit_headers(response.headers_mut(), &info) {
//!             // Whether or not you use the specific RateLimitError in 
//!             // your handler, consider handling errors explicitly here. 
//!             // Again, though, you're free to `if let _ = add_rate_limit_headers(...` 
//!             // if you don't care about these errors.
//!             match e {
//!                 RateLimitError::HeaderError(e) => {
//!                     eprintln!("Failed to set rate limit headers due to invalid value: {}", e);
//!                 }
//!                 RateLimitError::Other(e) => {
//!                     eprintln!("Unexpected error setting rate limit headers: {}", e);
//!                 }
//!             }
//!         }
//! 
//!         Ok(response)    
//!     } else {
//!         // Handle other types of rejections, e.g.
//!         Ok(warp::reply::with_status(
//!             "Internal Server Error",
//!             StatusCode::INTERNAL_SERVER_ERROR,
//!         ).into_response())
//!     }
//! } 
//! ```

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use warp::{
    http::header::{self, HeaderMap, HeaderValue},
    reject, Filter, Rejection
};

pub use chrono;
pub use serde;

/// Configuration for the rate limiter
#[derive(Clone, Debug, PartialEq)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed within the window
    pub max_requests: u32,
    /// Time window for rate limiting
    pub window: Duration,
    /// Format for Retry-After header (RFC 7231 Date or Seconds)
    pub retry_after_format: RetryAfterFormat,
}

/// Format options for the Retry-After header
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum RetryAfterFormat {
    /// HTTP-date format (RFC 7231)
    #[default]
    HttpDate,
    /// Number of seconds
    Seconds,
}

/// Information about the current rate limit status
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// Time until the rate limit resets
    pub retry_after: String,
    /// Maximum requests allowed in the window
    pub limit: u32,
    /// Remaining requests in the current window
    pub remaining: u32,
    /// Unix timestamp when the rate limit resets
    pub reset_timestamp: i64,
    /// Format used for retry-after header
    pub retry_after_format: RetryAfterFormat,
}

/// Custom rejection type for rate limiting
#[derive(Debug)]
pub struct RateLimitRejection {
    /// Duration until the client can retry
    pub retry_after: Duration,
    /// Maximum requests allowed in the window
    pub limit: u32,
    /// Unix timestamp when the rate limit resets
    pub reset_time: DateTime<Utc>,
    /// Format to use for Retry-After header
    pub retry_after_format: RetryAfterFormat,
}

impl warp::reject::Reject for RateLimitRejection {}

/// Sensible (opinionated) defaults
impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 60, // 60 req/min baseline
            window: Duration::from_secs(60),
            retry_after_format: RetryAfterFormat::HttpDate,
        }
    }
}

/// Factory methods for quickly building a rate limiter
impl RateLimitConfig {
    /// Build a `RateLimitConfig` with sensible defaults for requests per minute
    pub fn max_per_minute(max: u32) -> Self {
        Self {
            max_requests: max,
            window: Duration::from_secs(60),
            ..Default::default()
        }
    }

    /// Build a `RateLimitConfig` with custom window size in seconds
    pub fn max_per_window(max_requests: u32, window_seconds: u64) -> Self {
        Self {
            max_requests,
            window: Duration::from_secs(window_seconds),
            ..Default::default()
        }
    }
}

/// Errors that can occur during rate limiting logic
#[derive(Debug)]
pub enum RateLimitError {
    /// Failed to set rate limit headers
    HeaderError(warp::http::header::InvalidHeaderValue),
    /// Other unexpected errors
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RateLimitError::HeaderError(e) => write!(f, "Failed to set rate limit header: {}", e),
            RateLimitError::Other(e) => write!(f, "Rate limit error: {}", e),
        }
    }
}

impl std::error::Error for RateLimitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RateLimitError::HeaderError(e) => Some(e),
            RateLimitError::Other(e) => Some(&**e),
        }
    }
}

#[derive(Clone)]
struct RateLimiter {
    state: Arc<RwLock<HashMap<String, (Instant, u32)>>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    fn new(config: RateLimitConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    async fn check_rate_limit(&self, key: &str) -> Result<RateLimitInfo, Rejection> {
        let mut state = self.state.write().await;
        let now = Instant::now();
        let current = state.get(key).copied();

        match current {
            Some((last_request, count)) => {
                if now.duration_since(last_request) > self.config.window {
                    // Window has passed, reset counter
                    state.insert(key.to_string(), (now, 1));
                    Ok(self.create_info(self.config.max_requests - 1, now))
                } else if count >= self.config.max_requests {
                    // Rate limit exceeded
                    let retry_after = self.config.window - now.duration_since(last_request);
                    let reset_time = Utc::now() + ChronoDuration::from_std(retry_after).unwrap();

                    Err(reject::custom(RateLimitRejection {
                        retry_after,
                        limit: self.config.max_requests,
                        reset_time,
                        retry_after_format: self.config.retry_after_format.clone(),
                    }))
                } else {
                    // Increment counter
                    state.insert(key.to_string(), (last_request, count + 1));
                    Ok(self.create_info(
                        self.config.max_requests - (count + 1),
                        last_request,
                    ))
                }
            }
            None => {
                // First request
                state.insert(key.to_string(), (now, 1));
                Ok(self.create_info(self.config.max_requests - 1, now))
            }
        }
    }

    fn create_info(&self, remaining: u32, start: Instant) -> RateLimitInfo {
        let reset_time = start + self.config.window;
        let retry_after = match self.config.retry_after_format {
            RetryAfterFormat::HttpDate => {
                (Utc::now() + ChronoDuration::from_std(self.config.window).unwrap()).to_rfc2822()
            }
            RetryAfterFormat::Seconds => self.config.window.as_secs().to_string(),
        };

        RateLimitInfo {
            retry_after,
            limit: self.config.max_requests,
            remaining,
            reset_timestamp: (Utc::now() + ChronoDuration::from_std(reset_time.duration_since(start)).unwrap()).timestamp(),
            retry_after_format: self.config.retry_after_format.clone(),
        }
    }
}

/// Creates a rate limiting filter with the given configuration
pub fn with_rate_limit(
    config: RateLimitConfig,
) -> impl Filter<Extract = (RateLimitInfo,), Error = Rejection> + Clone {
    let rate_limiter = RateLimiter::new(config);

    warp::filters::addr::remote()
        .map(move |addr: Option<std::net::SocketAddr>| {
            (
                rate_limiter.clone(),
                addr.map(|a| a.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
            )
        })
        .and_then(|(rate_limiter, ip): (RateLimiter, String)| async move {
            rate_limiter.check_rate_limit(&ip).await
        })
}

/// Adds rate limit headers to a response
pub fn add_rate_limit_headers(
    headers: &mut HeaderMap,
    info: &RateLimitInfo,
) -> Result<(), RateLimitError> {
    headers.insert(header::RETRY_AFTER, 
        HeaderValue::from_str(&info.retry_after).map_err(RateLimitError::HeaderError)?);
    headers.insert(
        "X-RateLimit-Limit",
        HeaderValue::from_str(&info.limit.to_string()).map_err(RateLimitError::HeaderError)?,
    );
    headers.insert(
        "X-RateLimit-Remaining",
        HeaderValue::from_str(&info.remaining.to_string()).map_err(RateLimitError::HeaderError)?,
    );
    headers.insert(
        "X-RateLimit-Reset",
        HeaderValue::from_str(&info.reset_timestamp.to_string()).map_err(RateLimitError::HeaderError)?,
    );
    Ok(())
}

/// Gets rate limit information from a rejection
pub fn get_rate_limit_info(rejection: &RateLimitRejection) -> RateLimitInfo {
    let retry_after = match rejection.retry_after_format {
        RetryAfterFormat::HttpDate => rejection.reset_time.to_rfc2822(),
        RetryAfterFormat::Seconds => rejection.retry_after.as_secs().to_string(),
    };

    RateLimitInfo {
        retry_after,
        limit: rejection.limit,
        remaining: 0,
        reset_timestamp: rejection.reset_time.timestamp(),
        retry_after_format: rejection.retry_after_format.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::task::JoinSet;
    use warp::Reply;
    use warp::{
        test::request,
        http::StatusCode,
        Filter,
    };
    use std::convert::Infallible;

    // Helper function to create a test rate limiter with rejection handling
    async fn create_test_route(
        config: RateLimitConfig,
    ) -> impl Filter<Extract = impl Reply, Error = Infallible> + Clone {
        with_rate_limit(config)
            .map(|info: RateLimitInfo| info.remaining.to_string())
            .recover(|rejection: Rejection| async move {
                if let Some(rate_limit) = rejection.find::<RateLimitRejection>() {
                    let info = get_rate_limit_info(rate_limit);
                    let mut resp = warp::reply::with_status(
                        "Rate limit exceeded",
                        StatusCode::TOO_MANY_REQUESTS,
                    ).into_response();
                    add_rate_limit_headers(resp.headers_mut(), &info).unwrap();
                    Ok(resp)
                } else {
                    Ok(warp::reply::with_status(
                        "Internal error", 
                        StatusCode::INTERNAL_SERVER_ERROR,
                    ).into_response())
                }
            })
    }

    #[test]
    fn test_config_builders() {
        // Test max_per_minute builder
        let per_minute = RateLimitConfig::max_per_minute(60);
        assert_eq!(per_minute.window, Duration::from_secs(60));
        assert_eq!(per_minute.max_requests, 60);
        assert_eq!(per_minute.retry_after_format, RetryAfterFormat::HttpDate);

        // Test max_per_window builder
        let custom = RateLimitConfig::max_per_window(30, 120);
        assert_eq!(custom.window, Duration::from_secs(120));
        assert_eq!(custom.max_requests, 30);
        assert_eq!(custom.retry_after_format, RetryAfterFormat::HttpDate);

        // Test default config
        let default = RateLimitConfig::default();
        assert_eq!(default.window, Duration::from_secs(60));
        assert_eq!(default.max_requests, 60);
        assert_eq!(default.retry_after_format, RetryAfterFormat::HttpDate);
    }

    #[tokio::test]
    async fn test_comprehensive_rate_limit_rejection() {
        let config = RateLimitConfig {
            max_requests: 1,
            window: Duration::from_secs(5),
            retry_after_format: RetryAfterFormat::Seconds,
        };

        let route = create_test_route(config.clone()).await;

        // First request succeeds
        let resp1 = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&route)
            .await;
        assert_eq!(resp1.status(), 200);
        assert_eq!(resp1.body(), "0"); // Last remaining request

        // Second request gets rejected with proper headers
        let resp2 = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&route)
            .await;
        
        assert_eq!(resp2.status(), 429);
        
        // Verify rate limit headers exist and have correct format
        let headers = resp2.headers();
        assert!(headers.contains_key(header::RETRY_AFTER));
        assert!(headers.contains_key("X-RateLimit-Limit"));
        assert!(headers.contains_key("X-RateLimit-Remaining"));
        assert!(headers.contains_key("X-RateLimit-Reset"));
        
        // Verify header values
        assert_eq!(headers.get("X-RateLimit-Limit").unwrap(), "1");
        assert_eq!(headers.get("X-RateLimit-Remaining").unwrap(), "0");
        
        // Verify Retry-After is a number of seconds
        let retry_after = headers.get(header::RETRY_AFTER).unwrap().to_str().unwrap();
        assert!(retry_after.parse::<u64>().is_ok());
    }

    #[tokio::test]
    async fn test_retry_after_formats() {
        // Test HttpDate format
        let http_date_config = RateLimitConfig {
            max_requests: 1,
            window: Duration::from_secs(15),
            retry_after_format: RetryAfterFormat::HttpDate,
        };

        let http_date_route = create_test_route(http_date_config).await;

        // Trigger rate limit with HttpDate format
        let _ = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&http_date_route)
            .await;
        
        let resp_http = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&http_date_route)
            .await;

        // Verify HttpDate format
        let retry_after_http = resp_http.headers().get(header::RETRY_AFTER).unwrap().to_str().unwrap();
        assert!(!retry_after_http.is_empty()); // RFC2822 date contains GMT
        
        // Test Seconds format
        let seconds_config = RateLimitConfig {
            max_requests: 1,
            window: Duration::from_secs(5),
            retry_after_format: RetryAfterFormat::Seconds,
        };

        let seconds_route = create_test_route(seconds_config).await;

        // Trigger rate limit with Seconds format
        let _ = request()
            .remote_addr("127.0.0.2:1234".parse().unwrap())
            .reply(&seconds_route)
            .await;
        
        let resp_sec = request()
            .remote_addr("127.0.0.2:1234".parse().unwrap())
            .reply(&seconds_route)
            .await;

        // Verify Seconds format
        let retry_after_sec = resp_sec.headers().get(header::RETRY_AFTER).unwrap().to_str().unwrap();
        assert!(retry_after_sec.parse::<u64>().is_ok());
        assert!(retry_after_sec.parse::<u64>().unwrap() <= 5);
    }

    #[test]
    fn test_rate_limit_info_extraction() {
        let now = Utc::now();
        let rejection = RateLimitRejection {
            retry_after: Duration::from_secs(60),
            limit: 100,
            reset_time: now,
            retry_after_format: RetryAfterFormat::Seconds,
        };

        let info = get_rate_limit_info(&rejection);

        assert_eq!(info.limit, 100);
        assert_eq!(info.remaining, 0);
        assert_eq!(info.reset_timestamp, now.timestamp());
        assert_eq!(info.retry_after, "60");
        
        // Test with HttpDate format
        let rejection_http = RateLimitRejection {
            retry_after: Duration::from_secs(60),
            limit: 100,
            reset_time: now,
            retry_after_format: RetryAfterFormat::HttpDate,
        };

        let info_http = get_rate_limit_info(&rejection_http);
        assert!(!info_http.retry_after.is_empty()); // RFC2822 date format
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        let config = RateLimitConfig {
            max_requests: 5,
            window: Duration::from_secs(1),
            retry_after_format: RetryAfterFormat::Seconds,
        };

        let route = create_test_route(config.clone()).await;
        let mut set = JoinSet::new();

        // Launch 10 concurrent requests
        for _ in 0..10 {
            let route = route.clone();
            set.spawn(async move {
                request()
                    .remote_addr("127.0.0.1:1234".parse().unwrap())
                    .reply(&route)
                    .await
            });
        }

        let mut success_count = 0;
        let mut rate_limited_count = 0;

        while let Some(Ok(resp)) = set.join_next().await {
            match resp.status() {
                StatusCode::OK => success_count += 1,
                StatusCode::TOO_MANY_REQUESTS => rate_limited_count += 1,
                _ => panic!("Unexpected response status"),
            }
        }

        assert_eq!(success_count, 5, "Expected exactly 5 successful requests");
        assert_eq!(rate_limited_count, 5, "Expected exactly 5 rate-limited requests");
    }

    #[test]
    fn test_invalid_header_value_handling() {
        let mut headers = HeaderMap::new();
        let invalid_info = RateLimitInfo {
            retry_after: "invalid\u{0000}characters".to_string(),
            limit: 100,
            remaining: 50,
            reset_timestamp: 1234567890,
            retry_after_format: RetryAfterFormat::Seconds,
        };
        
        let result = add_rate_limit_headers(&mut headers, &invalid_info);
        assert!(matches!(result, Err(RateLimitError::HeaderError(_))));
    }
}