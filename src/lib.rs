//! Rate limiting middleware for Warp web applications.
//!
//! This crate provides a flexible rate limiting implementation that can be easily integrated
//! into Warp-based web services. It supports per-IP rate limiting with configurable time windows
//! and request limits.
//!
//! # Features
//!
//! - In memory, IP-based rate limiting
//! - Configurable time windows and request limits
//! - Thread-safe request tracking
//! - Idiomatic integration with existing Warp routes
//!

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, warn};
use warp::filters::BoxedFilter;
use warp::{Filter, Rejection, Reply};

/// Error types for rate limiting operations.
///
/// Currently only includes the `LimitExceeded` variant, but may be extended
/// in future versions to handle additional error cases.
#[derive(Debug)]
pub enum RateLimitError {
    /// Indicates that the client has exceeded their rate limit
    LimitExceeded,
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RateLimitError::LimitExceeded => write!(f, "rate limit exceeded"),
        }
    }
}

impl std::error::Error for RateLimitError {}
impl warp::reject::Reject for RateLimitError {}

/// Configuration options for the rate limiter.
///
/// This struct allows customization of the rate limiting behavior through
/// window size and maximum request count settings. More may be added in
/// a future version.
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Duration of the rate limiting window
    pub window_size: Duration,
    /// Maximum number of requests allowed within the window
    pub max_requests: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            window_size: Duration::from_secs(60),
            max_requests: 100,
        }
    }
}

#[derive(Debug)]
pub struct RateLimitData {
    pub requests: Vec<Instant>,
    pub window_size: Duration,
    pub max_requests: usize,
}

impl RateLimitData {
    fn new(config: &RateLimitConfig) -> Self {
        Self {
            requests: Vec::new(),
            window_size: config.window_size,
            max_requests: config.max_requests,
        }
    }

    

    fn is_rate_limited(&mut self, now: Instant) -> bool {
        self.requests.retain(|&time| now - time <= self.window_size);

        if self.requests.len() >= self.max_requests {
            warn!("Rate limit exceeded. Current requests: {}", self.requests.len());
            return true;
        }

        self.requests.push(now);
        debug!("Request accepted. Current requests: {}", self.requests.len());
        false
    }
}

/// The main rate limiter implementation.
///
/// `RateLimit` maintains a thread-safe state of request counts per IP address
/// and provides methods to configure and apply rate limiting to Warp routes.
/// The internal state is protected by a `tokio::sync::Mutex` and wrapped in an `Arc`,
/// making it safe to share across multiple threads in an async context.
#[derive(Clone)]
pub struct RateLimit {
    state: Arc<Mutex<HashMap<IpAddr, RateLimitData>>>,
    config: RateLimitConfig,
}

impl RateLimit {
    /// Creates a new rate limiter with default configuration.
    ///
    /// Default configuration allows 100 requests per minute.
    ///
    /// # Example
    ///
    /// ```rust
    /// use warp_rate_limit::RateLimit;
    ///
    /// let rate_limiter = RateLimit::new();
    /// ```
    pub fn new() -> Self {
        Self::with_config(RateLimitConfig::default())
    }

    /// Creates a new rate limiter with custom configuration.
    ///
    /// # Example
    ///
    /// ```rust
    /// use warp_rate_limit::{RateLimit, RateLimitConfig};
    /// use std::time::Duration;
    ///
    /// let config = RateLimitConfig {
    ///     window_size: Duration::from_secs(30),
    ///     max_requests: 50,
    /// };
    /// let rate_limiter = RateLimit::with_config(config);
    /// ```
    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Sets the time window for rate limiting.
    ///
    /// # Arguments
    ///
    /// * `window` - The duration of the rate limiting window
    ///
    /// # Example
    ///
    /// ```rust
    /// use warp_rate_limit::RateLimit;
    /// use std::time::Duration;
    ///
    /// let rate_limiter = RateLimit::new()
    ///     .with_window(Duration::from_secs(30));
    /// ```
    pub fn with_window(mut self, window: Duration) -> Self {
        self.config.window_size = window;
        self
    }

    /// Sets the maximum number of requests allowed within the window.
    ///
    /// # Arguments
    ///
    /// * `max_requests` - Maximum number of requests allowed
    ///
    /// # Example
    ///
    /// ```rust
    /// use warp_rate_limit::RateLimit;
    /// let rate_limiter = RateLimit::new()
    ///     .with_max_requests(50);
    /// ```
    pub fn with_max_requests(mut self, max_requests: usize) -> Self {
        self.config.max_requests = max_requests;
        self
    }

    /// Converts the rate limiter into a Warp filter.
    ///
    /// This method creates a Warp filter that can be composed with other filters
    /// to add rate limiting to a route.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use warp::Filter;
    /// use warp_rate_limit::RateLimit;
    ///
    /// let rate_limiter = RateLimit::new();
    ///
    /// let route = warp::path!("api" / "endpoint")
    ///     .and(rate_limiter.into_filter())
    ///     .map(|_| "Hello, World!");
    /// ```
    ///
    /// Below is an example with defaults (100 requests per 60 seconds):
    ///
    /// ```rust
    /// use warp_rate_limit::RateLimit;
    /// let rate_limit = RateLimit::new().into_filter();
    /// ```
    pub fn into_filter(self) -> BoxedFilter<(RateLimitData,)> {
        let rate_limiter = self;

        warp::any()
            .map(move || rate_limiter.clone())
            .and(warp::filters::addr::remote())
            .and_then(|rate_limiter: RateLimit, addr: Option<std::net::SocketAddr>| async move {
                let ip = addr.map(|a| a.ip()).unwrap_or_else(|| "0.0.0.0".parse().unwrap());
                let now = Instant::now();

                let mut state = rate_limiter.state.lock().await;
                let rate_limit_data = state
                    .entry(ip)
                    .or_insert_with(|| RateLimitData::new(&rate_limiter.config));

                    if rate_limit_data.is_rate_limited(now) {
                        return Err(warp::reject::custom(RateLimitError::LimitExceeded));
                    }

                    // Create info to pass downstream
            let info = RateLimitData {
                max_requests: rate_limit_data.max_requests,
                requests: rate_limit_data.requests.clone(),
                window_size: rate_limit_data.window_size
            };


                Ok::<_, Rejection>(info)
            })
            .boxed()
    }
}

/// Handles rate limit rejection responses.
///
/// This function can be used with Warp's `recover` method to provide
/// proper error responses when rate limits are exceeded.
///
/// # Example
///
/// The following creates a default filter (100 requests per 60 seconds)
/// on two endpoints, `/api` and `/endpoint`:
///
/// ```rust
/// use warp_rate_limit::{RateLimit,handle_rejection};
/// use warp::Filter;
///
/// let rate_limiter = RateLimit::new().into_filter();
/// let route = warp::path!("api" / "endpoint")
///     .and(rate_limiter)
///     .map(|_| "Hello, World!")
///     .recover(handle_rejection);
/// ```
pub async fn handle_rejection(err: Rejection) -> Result<impl Reply, Rejection> {
    if let Some(RateLimitError::LimitExceeded) = err.find() {
        Ok(warp::reply::with_status(
            "Rate limit exceeded. Please try again later.",
            warp::http::StatusCode::TOO_MANY_REQUESTS,
        ))
    } else {
        Err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use warp::test::request;

    #[tokio::test]
    async fn test_basic_rate_limiting() {
        let rate_limit = RateLimit::new()
            .with_window(Duration::from_secs(1))
            .with_max_requests(2)
            .into_filter();

        // First two requests should succeed
        let resp1 = request()
            .remote_addr(SocketAddr::from(([127, 0, 0, 1], 1234)))
            .filter(&rate_limit)
            .await;
        assert!(resp1.is_ok());

        let resp2 = request()
            .remote_addr(SocketAddr::from(([127, 0, 0, 1], 1234)))
            .filter(&rate_limit)
            .await;
        assert!(resp2.is_ok());

        // Third request should fail
        let resp3 = request()
            .remote_addr(SocketAddr::from(([127, 0, 0, 1], 1234)))
            .filter(&rate_limit)
            .await;
        assert!(resp3.is_err());
    }
}
