use std::time::Duration;
use warp::Filter;
use warp_rate_limit::{RateLimit, handle_rejection};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let rate_limit = RateLimit::new()
        .with_window(Duration::from_secs(30))
        .with_max_requests(5)
        .into_filter();

    let hello = warp::path("hello")
        .and(rate_limit.clone())
        .map(|_| "Hello, World!");

    let api = warp::path("api")
        .and(rate_limit)
        .map(|_| warp::reply::json(&serde_json::json!({
            "status": "success",
            "message": "API response"
        })));

    let routes = hello
        .or(api)
        .recover(handle_rejection);

    println!("Server running at http://127.0.0.1:3030");
    println!("Try:");
    println!("  - http://127.0.0.1:3030/hello");
    println!("  - http://127.0.0.1:3030/api");

    warp::serve(routes)
        .run(([127, 0, 0, 1], 3030))
        .await;
}
