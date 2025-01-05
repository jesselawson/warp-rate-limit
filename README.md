# Rate-limiting Middleware for Warp

[![Crates.io](https://img.shields.io/crates/v/warp-rate-limit.svg)](https://crates.io/crates/warp-rate-limit)
[![Documentation](https://docs.rs/warp-rate-limit/badge.svg)](https://docs.rs/warp-rate-limit)

Rate-limiting middleware for Warp.  Implements [RFC 6585](https://datatracker.ietf.org/doc/html/rfc6585#section-4) for "429 Too many Requests" response and [RFC 7231](https://datatracker.ietf.org/doc/html/rfc7231#section-7.1.3) for Retry After header.

This crate provides [RFC 6585](https://datatracker.ietf.org/doc/html/rfc6585#section-4) 
and [RFC 7231](https://datatracker.ietf.org/doc/html/rfc7231#section-7.1.3) compliant 
in-memory rate limiting with configurable windows and limits as lightweight middleware for 
Warp web applications.
 
It provides a Filter you can add to your routes that exposes rate-limiting
information to your handlers, and a rate limited `Rejection` type for error recovery.
 
It does not yet provide persistence, nor is the HashMap that stores IPs bounded. Both 
of these may be changed in a future version.
 
# Quickstart
 
1. Include the crate:
 
`cargo add warp-rate-limit`

2. Define one or more rate limit configurations. Following are some 
examples of available builder methods. The variable names are arbitrary: 

```rust,no_run
// Limit: 60 requests per 60 Earth seconds
let public_routes_rate_limit = RateLimitConfig::default();

// Limit: 100 requests per 60 Earth seconds
let partner_routes_rate_limit = RateLimitConfig::max_per_minute(100);

// Limit: 10 requests per 20 Earth seconds
let static_route_limit = RateLimitConfig::max_per_window(10,20);
```

3. Add the rate-limiting `Filter` to your route, which exposes 
a `RateLimitInfo` struct to your handler:

```rust
let my_route = warp::path!("some_ratelimited_route")
    .and(warp::get())
    // - - -- --- ----- -------- ------------- ---------------------
    .and(with_rate_limit(public_routes_rate_limit.clone()))
    // - - -- --- ----- -------- ------------- ---------------------
    .and_then(your_request_handler)
    .recover(your_rejection_handler)

```

4. Use the `RateLimitInfo` data in your request handler. If you don't want 
to use rate-limiting information related to the IP address associated 
with this request, you can skip this part (but warp requires that you 
still account for the `RateLimitInfo` in your handler signature):

```rust
// Example route handler
async fn your_request_handler(rate_limit_info: RateLimitInfo) -> Result<impl Reply, Rejection> {
    // Create a base response
    let mut response = warp::reply::with_status(
        "Hello world", 
        StatusCode::OK
    ).into_response();

    // Optionally add rate limit headers to your response:
    let _ = add_rate_limit_headers(response.headers_mut(), &rate_limit_info);

    Ok(response)
}
```

5. Handle rate limit errors in your rejection handler: 

```rust,no_run
// Example rejection handler
async fn your_rejection_handler(rejection: Rejection) -> Result<impl Reply, Infallible> {
    // Somewhere in your rejection handling:
    if let Some(rate_limit_rejection) = rejection.find::<RateLimitRejection>() {
        // We have a rate limit rejection, so get some info about it: 
        let info = get_rate_limit_info(rate_limit_rejection);

        // Use that info to create a response:
        let message = format!(
            "Rate limit exceeded. Try again after {}.", 
            info.retry_after
        );

        // Let's build that response:
        let mut response = warp::reply::with_status(
            message,
            StatusCode::TOO_MANY_REQUESTS
        ).into_response();

        // Then, add the rate-limiting headers to that response:
        let _ = add_rate_limit_headers(response.headers_mut(), &rate_limit_info);

        Ok(response)    

    } else {
        // Handle other types of rejections, e.g.
        Ok(warp::reply::with_status(
            "Internal Server Error",
            StatusCode::INTERNAL_SERVER_ERROR,
        ).into_response())
    }
} 
```

## Builder methods

| Usage | Description | 
| :--   | :---        |
| `RateLimitConfig::default()` | Max requests: 60/minute |
| `RateLimitConfig::max_per_minute(x:u32)` | Max requests: `x`/minute |
| `RateLimitConfig::max_per_window(max:u32,window:u64)` | Max requests: `max`/`window` (in seconds) |

## Reference

* `with_rate_limit(config: RateLimitConfig)`: given your `RateLimitConfig`, injects a `Filter` 
  into your route that exposes a `RateLimitInfo` struct to your handler.
* `add_rate_limit_headers(&mut HeaderMap, &RateLimitInfo)`: given a mutable reference to the 
  headers of a [`Response`](https://docs.rs/warp/0.3.7/warp/reply/type.Response.html) (e.g., `response.headers_mut()`) 
  and a reference to a populated `RateLimitInfo` struct, adds headers related to rate-limiting to the `Response` reference 
  provided. Headers can be included in both successful replies (e.g., `200`) as well as rate-limited responses (e.g., `429`).
  The required `RateLimitInfo` struct comes from either the `Filter` that injects it into your handler, or manually in 
  your rejection recovery handler via `get_rate_limit_info()`.
* `get_rate_limit_info(&RateLimitRejection)`: given a [`Rejection`](https://docs.rs/warp/0.3.7/warp/reject/struct.Rejection.html)
  that includes a `RateLimitRejection` (e.g., `if let Some(rate_limited_rejection) = rejection.find::<RateLimitRejection>()`), 
  return a `RateLimitInfo` struct that contains information related to the currently rate-limited IP address. This is useful 
  for letting the requestor know that they are being rate-limited, as well as when their rate limit will be released. 

## Rate-limited headers

An example of headers provided in response to a rate-limited requesting IP:

```http
HTTP/1.1 429 Too Many Requests
retry-after: Wed, 1 Jan 2025 00:01:00 GMT
x-ratelimit-limit: 100
x-ratelimit-remaining: 0
x-ratelimit-reset: 1704067260
```

## Error handling

The Quickstart example shows a form of error handling appropriate in situations 
where you do not care to handle errors that may occur in this library. Following are more 
error-handling examples, straight from the `basic.rs` example:

```rust
// If you really don't care about the error, you can do something like this:
let _ = add_rate_limit_headers(response.headers_mut(), &info);

// If you care about the fact that it errored but not necessarily
// the specific error itself, you can do something like this:
if !add_rate_limit_headers(response.headers_mut(), &info).is_ok() {
    eprintln!("Failed to set headers");
}

// If you want full control over error handling, you can do something like this:
if let Err(e) = add_rate_limit_headers(response.headers_mut(), &info) {
    match e {
        RateLimitError::HeaderError(e) => {
            eprintln!("Failed to set rate limit headers due to invalid value: {}", e);
        }
        RateLimitError::Other(e) => {
            eprintln!("Unexpected error setting rate limit headers: {}", e);
        }
    }
}
```

You only need to call `add_rate_limit_headers()` once; the above example illustrates 
three different ways to do the same thing, with varying levels of library error recovery 
comfort.

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
