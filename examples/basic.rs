use std::time::Duration;
use warp::{reject::Rejection, Filter};
use warp_rate_limit::*;

#[tokio::main]
async fn main() {
    // Configure rate limiting: 5 reqs/30s
    let rate_limit = RateLimitConfig {
        max_requests: 5,
        window: Duration::from_secs(30),
        retry_after_format: RetryAfterFormat::HttpDate,
    };

    // Protected endpoint
    let route = warp::path!("api" / "rate_limited")
        .and(with_rate_limit(rate_limit)) // Don't forget to .clone() if you are using this more than once!
        .map(|remaining: u32| {
            warp::reply::json(&serde_json::json!({
                "message": "Success",
                "remaining_requests": remaining
            }))
        })
        .recover(|rejection: Rejection| async move {
            if rejection.find::<RateLimitRejection>().is_some() {
                Ok(warp::reply::with_status(
                    "Rate limit exceeded",
                warp::http::StatusCode::TOO_MANY_REQUESTS))
            } else {
                Err(rejection)
            }
        });



    println!("Server running on http://127.0.0.1:3030");
    println!("To see a rate-limited response, issue a request more than five times.");
    println!("Example: `curl -i http://127.0.0.1:3030/api/rate_limited`");
    warp::serve(route).run(([127, 0, 0, 1], 3030)).await;
}