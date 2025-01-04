#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Rate limiting middleware for Warp
//! 
//! This crate provides RFC 6585 compliant rate limiting middleware for Warp web applications.
//! It supports in-memory rate limiting with configurable windows and limits.
//!


use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use warp::{Filter, Rejection, Reply};
use warp::http::header::HeaderValue;

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
    /// Build a `RateLimitConfig` with sensible defaults. In this 
    /// case, will rate-limit based on provided `max` requests 
    /// per minute (60 Earth seconds).
    /// ```rust
    /// use std::time::Duration;
    /// use warp_rate_limit::RateLimitConfig;
    /// let x = RateLimitConfig {
    ///     max_requests: 45,
    ///     window: Duration::from_secs(60),
    ///     ..Default::default()
    /// };
    /// assert_eq!(x, RateLimitConfig::max_per_minute(45))
    /// ```
    pub fn max_per_minute(max: u32) -> Self {
        Self {
            max_requests: max,
            window: Duration::from_secs(60),
            ..Default::default()
        }
    }

    /// A quick way to build a RateLimitConfig:
    /// ```rust
    /// use std::time::Duration;
    /// use warp_rate_limit::RateLimitConfig;
    /// let x = RateLimitConfig {
    ///     max_requests: 50,
    ///     window: Duration::from_secs(60),
    ///     ..Default::default()
    /// };
    /// assert_eq!(x, RateLimitConfig::max_per_window(50,60))
    /// ```
    pub fn max_per_window(max_requests: u32, window_seconds:u64) -> Self {
        Self {
            max_requests: max_requests,
            window: Duration::from_secs(window_seconds),
            ..Default::default()
        }
    }
}

/// Format options for the Retry-After header
#[derive(Clone, Debug, Default, PartialEq)]
pub enum RetryAfterFormat {
    /// HTTP-date format (RFC 7231)
    HttpDate,
    /// Number of seconds
    #[default]
    Seconds,
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

/// State for tracking rate limits
#[derive(Clone)]
struct RateLimiter {
    state: Arc<RwLock<HashMap<String, (Instant, u32)>>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    /// Creates a new rate limiter with the given configuration
    fn new(config: RateLimitConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Checks if a request should be rate limited
    async fn check_rate_limit(&self, key: &str) -> Result<u32, Rejection> {
        let mut state = self.state.write().await;
        let now = Instant::now();
        let current = state.get(key).copied();

        match current {
            Some((last_request, count)) => {
                if now.duration_since(last_request) > self.config.window {
                    // Window has passed, reset counter
                    state.insert(key.to_string(), (now, 1));
                    Ok(self.config.max_requests - 1)
                } else if count >= self.config.max_requests {
                    // Rate limit exceeded
                    let retry_after = self.config.window - now.duration_since(last_request);
                    let reset_time = Utc::now() + ChronoDuration::from_std(retry_after).unwrap();

                    Err(warp::reject::custom(RateLimitRejection {
                        retry_after,
                        limit: self.config.max_requests,
                        reset_time,
                        retry_after_format: self.config.retry_after_format.clone(),
                    }))
                } else {
                    // Increment counter
                    state.insert(key.to_string(), (last_request, count + 1));
                    Ok(self.config.max_requests - (count + 1))
                }
            }
            None => {
                // First request
                state.insert(key.to_string(), (now, 1));
                Ok(self.config.max_requests - 1)
            }
        }
    }
}

/// Creates a rate limiting filter with the given configuration
pub fn with_rate_limit(
    config: RateLimitConfig,
) -> impl Filter<Extract = (u32,), Error = Rejection> + Clone {
    let rate_limiter = RateLimiter::new(config);
    
    warp::filters::addr::remote()
        .map(move |addr: Option<std::net::SocketAddr>| {
            (
                rate_limiter.clone(),
                addr.map(|a| a.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
            )
        })
        .and_then(|(rate_limiter, ip):(RateLimiter, String)| async move {
            rate_limiter.check_rate_limit(&ip).await
        })
}

/// Creates a rate limit response with all required headers
pub fn create_rate_limit_response(
    rejection: &RateLimitRejection,
) -> Result<impl Reply, Box<dyn std::error::Error + Send + Sync + 'static>> {

    let retry_after = match rejection.retry_after_format {
        RetryAfterFormat::HttpDate => rejection.reset_time.to_rfc2822(),
        RetryAfterFormat::Seconds => rejection.retry_after.as_secs().to_string(),
    };

    let mut res = warp::http::Response::new(format!(
        "Rate limit exceeded. Try again at {}",
        rejection.reset_time.to_rfc2822()
    ));

    res.headers_mut().insert(warp::http::header::RETRY_AFTER, HeaderValue::from_str(&retry_after).unwrap());
    res.headers_mut().insert("X-RateLimit-Limit", HeaderValue::from_str(&rejection.limit.to_string()).unwrap());
    res.headers_mut().insert("X-RateLimit-Remaining", HeaderValue::from_str("0").unwrap());
    res.headers_mut().insert("X-RateLimit-Reset", HeaderValue::from_str(&rejection.reset_time.timestamp().to_string()).unwrap());

    Ok(warp::reply::with_status(Box::new(res), warp::http::StatusCode::TOO_MANY_REQUESTS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use warp::test::request;
    use tokio::time::sleep;
    use tokio::task::JoinSet;

    #[tokio::test]
    async fn test_basic_rate_limiting() {
        let config = RateLimitConfig {
            max_requests: 2,
            window: Duration::from_secs(5),
            retry_after_format: RetryAfterFormat::Seconds,
        };
        
        let route = with_rate_limit(config.clone())
            .map(|remaining: u32| remaining.to_string())
            .recover(|r:Rejection| async move {
                if r.find::<RateLimitRejection>().is_some() {
                    Ok(warp::reply::with_status(
                        "rate limited",
                        warp::http::StatusCode::TOO_MANY_REQUESTS
                    ))
                } else {
                    Err(r)
                }
            });
            
        // First request should succeed
        let response = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&route)
            .await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), "1");
        
        // Second request should succeed
        let response = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&route)
            .await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.body(), "0");
        
        // Third request should fail
        let response = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&route)
            .await;

        assert_eq!(response.status(), 429);
        assert!(response.headers().contains_key(warp::http::header::RETRY_AFTER));
        
        // Wait for window to pass
        sleep(Duration::from_secs(5)).await;
        
        // Request should succeed again
        let response = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&route)
            .await;
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_multiple_ips() {
        let config = RateLimitConfig {
            max_requests: 1,
            window: Duration::from_secs(5),
            retry_after_format: RetryAfterFormat::HttpDate,
        };
        
        let route = with_rate_limit(config.clone())
            .map(|remaining: u32| remaining.to_string())
            .recover(|r:Rejection| async move {
                if r.find::<RateLimitRejection>().is_some() {
                    Ok(warp::reply::with_status(
                        "rate limited",
                        warp::http::StatusCode::TOO_MANY_REQUESTS
                    ))
                } else {
                    Err(r)
                }
            });
            
        // First IP succeeds
        let response = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&route)
            .await;
        assert_eq!(response.status(), 200);
        
        // First IP fails
        let response = request()
            .remote_addr("127.0.0.1:1234".parse().unwrap())
            .reply(&route)
            .await;
        assert_eq!(response.status(), 429);
        
        // Second IP succeeds
        let response = request()
            .remote_addr("127.0.0.2:1234".parse().unwrap())
            .reply(&route)
            .await;
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        let config = RateLimitConfig {
            max_requests: 5,
            window: Duration::from_secs(1),
            retry_after_format: RetryAfterFormat::Seconds,
        };
        
        let route = with_rate_limit(config.clone())
            .map(|remaining: u32| remaining.to_string())
            .recover(|r:Rejection| async move {
                if r.find::<RateLimitRejection>().is_some() {
                    Ok(warp::reply::with_status(
                        "rate limited",
                        warp::http::StatusCode::TOO_MANY_REQUESTS
                    ))
                } else {
                    Err(r)
                }
            });
            
        let mut set = JoinSet::new();
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
        while let Some(Ok(resp)) = set.join_next().await {
            if resp.status() == 200 {
                success_count += 1;
            }
        }
        assert_eq!(success_count, 5);
    }
}