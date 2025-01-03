# Rate-limiting Middleware for Warp

[![Crates.io](https://img.shields.io/crates/v/warp-rate-limit.svg)](https://crates.io/crates/warp-rate-limit)
[![Documentation](https://docs.rs/warp-rate-limit/badge.svg)](https://docs.rs/warp-rate-limit)

Rate-limiting middleware for Warp. Designed to be boring. Implements [RFC 6585](https://datatracker.ietf.org/doc/html/rfc6585#section-4) for "429 Too many Requests" response and [RFC 7231](https://datatracker.ietf.org/doc/html/rfc7231#section-7.1.3) for Retry After header.

## Quickstart

First:

`cargo add warp-rate-limit`

Then:

```rust
use warp_rate_limit::*;

let route = warp::path!("api/some_rate_limited_endpoint")
    // Add a rate limiter to your route:
    .and(with_rate_limit(RateLimitConfig::default())) // Default: 60 reqs/min
    // ...
    // Add rate limit error handler:
    .recover(handle_rate_limit_rejection);
```

## Usage

Add the crate to your project so that your `Cargo.toml` has:

```toml
[dependencies]
warp-rate-limit = "0.2"
```

Then, decide how you want to rate-limit your routes. 

To rate-limit a route, you'll need to do two things:

1. Add the rate-limiting middlware to your route via the `.and` warp Filter method (`[1]`)
2. Include the rate-limiting rejection handler via the `.recover` warp Filter method (`[2]`)

```rust
use warp::Filter;
use warp_rate_limit::{RateLimitConfig, with_rate_limit, handle_rate_limit_rejection};

// Using sensible defaults
let route = warp::path!("api/auth/login")
    .and(with_rate_limit(RateLimitConfig::default())) // [1]
    .recover(handle_rate_limit_rejection);            // [2]

// Using the max_per_minute builder configuration:
let route = warp::path!("api/customers/list")
    .and(with_rate_limit(RateLimitConfig::max_per_minute(50)))  // 50 req/min
    .recover(handle_rate_limit_rejection);

// Using a pre-defined set of rate_limits:
let public_rate_limit = RateLimitConfig::default();
let partner_rate_limit = RateLimitConfig::max_per_minute(120);

let some_public_route = warp::path!("api/public/some")
    .and(with_rate_limit( public_rate_limit.clone() )) 
    .recover( handle_rate_limit_rejection );            

// Using a custom per_minute configuration:
let route = warp::path!("api/customers/list")
    .and(with_rate_limit(partner_rate_limit.clone()))
    .recover(handle_rate_limit_rejection);
```

## Builder methods

| Usage | Description | 
| :--   | :---        |
| `RateLimitConfig::default()` | Max requests: 60/minute |
| `RateLimitConfig::max_per_minute(x:u32)` | Max requests: `x`/minute |
| `RateLimitConfig::max_per_window(max:u32,window:u64)` | Max requests: `max`/`window` (in seconds) |

## Features

- IP-based rate limiting
- RFC 7231 compliant headers
- Zero unsafe code
- Concurrent request handling
- Built-in rejection handling

## Response Headers

When rate limited (429 Too Many Requests):
```http
HTTP/1.1 429 Too Many Requests
retry-after: Wed, 1 Jan 2025 00:01:00 GMT
x-ratelimit-limit: 100
x-ratelimit-remaining: 0
x-ratelimit-reset: 1704067260
```

## Configuration

Full control if/when you need it, though I recommend using the builders unless
you absolutely need to control whether the `retry-after` header uses a date 
or seconds:

```rust
let config = RateLimitConfig {
    max_requests: 5,
    window: Duration::from_secs(30),  // 5 requests/30s
    retry_after_format: RetryAfterFormat::Seconds,
};
```

## Complete Example

```rust
use warp::Filter;
use warp_rate_limit::{RateLimitConfig, with_rate_limit, handle_rate_limit_rejection};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let hello = warp::path!("hello")
        .and(with_rate_limit(RateLimitConfig::default()))
        .map(|remaining: u32| format!("Hello! {} requests remaining", remaining))
        .recover(handle_rate_limit_rejection);

    warp::serve(hello)
        .run(([127, 0, 0, 1], 3030))
        .await;
}
```

Also check out the examples included in this repo.

## Testing

Run the test suite:
```bash
cargo test
```

Try the examples:
```bash
cargo run --example basic
```

## License

Released under MIT License.

```
LICENSE

Copyright (c) 2024 Jesse Lawson.

Permission is hereby granted, free of charge, to any person obtaining a copy 
of this software and associated documentation files (the "Software"), to deal 
in the Software without restriction, including without limitation the rights to 
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of 
the Software, and to permit persons to whom the Software is furnished to do so, 
subject to the following conditions:

The above copyright notice and this permission notice shall be included 
in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS 
OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, 
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE 
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER 
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, 
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE 
SOFTWARE.
```

Issues and PRs welcome.
