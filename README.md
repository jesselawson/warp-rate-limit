# Rate-limiting Middleware for Warp

[![Crates.io](https://img.shields.io/crates/v/warp-rate-limit.svg)](https://crates.io/crates/warp-rate-limit)
[![Documentation](https://docs.rs/warp-rate-limit/badge.svg)](https://docs.rs/warp-rate-limit)

A rate-limiting middleware for the Warp web framework.

**Features**:

- In memory, IP-based rate limiting
- Configurable time windows and request limits
- Thread-safe request tracking
- Idiomatic integration with existing Warp routes

## Usage

You'll first need to include this crate into your project. 

Add this to your `Cargo.toml`:

```toml
[dependencies]
warp-rate-limit = "0.1"
```

Or, run:

```zsh
cargo add warp-rate-limit
```

Then, you'll need to:
1. Create a rate limiter, and
2. Add the rate limiter to a route.

Here's an example:

```rust
use std::time::Duration;
use warp::{Filter, Reply};
use warp_rate_limit::RateLimit;

#[tokio::main]
async fn main() {
    // STEP 1. Create a rate limiter that allows, at most,
    // 5 requests per every 30 seconds:
    let rate_limit = RateLimit::new()
        .with_window(Duration::from_secs(30))
        .with_max_requests(5)
        .into_filter(); // Don't forget this part!

    // STEP 2. Add the rate limiter to a route:
    let route = warp::path("hello")
        .and(rate_limit)
        .map(|_| "Hello, World!");

    warp::serve(route).run(([127, 0, 0, 1], 3030)).await;
}
```

## Troubleshooting

If you get the following error:

```
the trait bound `RateLimit: warp::filter::FilterBase` is not satisfied
```

Then you likely need to add `into_filter()` to your rate limiter. For example:

```
let public_rate_limit = RateLimit::new().with_max_requests(10).into_filter();
let partner_rate_limit = RateLimit::new().with_max_requests(200).into_filter();
```


## Configuration Options

See the [documentation](https://docs.rs/warp-rate-limit).

## Designed to be Small

For most basic web applications these limitations are acceptable, but if you need more advanced features, consider using a dedicated rate-limiting solution with persistent storage.

- **IP-based only**: Rate limiting is performed based on IP address. This may not be suitable if your application is behind a proxy or if you need to rate limit by other identifiers (e.g., API keys, user IDs).
- **In-memory storage**: Rate limit data is stored in memory using a `HashMap`. This means:
  - Rate limit data is not persisted across server restarts
  - May not be suitable for high-scale deployments with multiple instances
  - Memory usage grows with the number of unique IPs making requests
- **Single window**: Only supports a single fixed-time window. Does not support more complex rate limiting strategies like sliding windows or token bucket algorithms.
- **No clustering support**: When running multiple instances of your application, each instance maintains its own rate limit counters. This could allow more requests than intended in a distributed setup.
- **No advanced configuration**: This is intended to be a simple, small solution for basic rate limiting, and as such, there is no support for per-route rate limiting, burst allowances, customr response headers, or allow/deny list support.

If any of these limitations are a blocker for you, consider augmenting the crate and submitting a PR.


## License

[MIT license](http://opensource.org/licenses/MIT).

## Contributing

Contributions are welcome and appreciated. Submit a PR when you're ready.
