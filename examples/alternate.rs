use std::time::Duration;
use warp::Filter;
use warp_rate_limit::*;

#[tokio::main]
async fn main() {
    // Create two rate-limiting groups: one for public, one for partners:
    let public_rate_limit = RateLimitConfig::max_per_window(1, 5);
    let partner_rate_limit = RateLimitConfig::max_per_window(5,5);

    let public_route = warp::path!("api" / "public")
        .and(with_rate_limit(public_rate_limit.clone()))
        // The map below is a substitute for your handler (e.g., `.and_then(some_handler)`):
        .map(|remaining: u32| {
            warp::reply::json(&serde_json::json!({
                "message": "Success",
                "remaining_requests": remaining
            }))
        })
        .recover(handle_rate_limit_rejection);

    let other_public_route = warp::path!("api" / "also_public")
        .and(with_rate_limit(public_rate_limit.clone()))
        // The map below is a substitute for your handler (e.g., `.and_then(some_handler)`):
        .map(|remaining: u32| {
            warp::reply::json(&serde_json::json!({
                "message": "Success",
                "remaining_requests": remaining
            }))
        })
        .recover(handle_rate_limit_rejection);

    let partner_route = warp::path!("api" / "partner")
        .and(with_rate_limit(partner_rate_limit.clone()))
        // The map below is a substitute for your handler (e.g., `.and_then(some_handler)`):
        .map(|remaining: u32| {
            warp::reply::json(&serde_json::json!({
                "message": "Success",
                "remaining_requests": remaining
            }))
        })
        .recover(handle_rate_limit_rejection);

    let other_partner_route = warp::path!("api" / "also_partner")
        .and(with_rate_limit(partner_rate_limit.clone()))
        // The map below is a substitute for your handler (e.g., `.and_then(some_handler)`):
        .map(|remaining: u32| {
            warp::reply::json(&serde_json::json!({
                "message": "Success",
                "remaining_requests": remaining
            }))
        })
        .recover(handle_rate_limit_rejection);

    let routes = public_route 
        .or(other_public_route)
        .or(partner_route)
        .or(other_partner_route);

    println!("Server running on http://127.0.0.1:3030");
    println!("To see a rate-limited response:");
    println!("Issue more than one request/5 seconds here: `curl -i http://127.0.0.1:3030/api/public`");
    println!("-OR-");
    println!("Issue more than five request/5 seconds here: `curl -i http://127.0.0.1:3030/api/partner`");
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}