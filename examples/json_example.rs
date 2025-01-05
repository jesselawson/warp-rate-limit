use serde::Serialize;
use serde_json::json;
use std::convert::Infallible;
use warp::{Filter, Rejection, Reply, http::StatusCode};
use warp_rate_limit::*;

#[derive(Serialize)]
struct MyCustomError {
    error: String,
    code: u16
}

#[tokio::main]
async fn main() {
    // Configure rate limiting: 3 requests per 30 seconds
    let rate_limit = RateLimitConfig {
        max_requests: 3,
        window: std::time::Duration::from_secs(30),
        retry_after_format: RetryAfterFormat::Seconds,
    };

    // Create routes
    let api = warp::path!("api" / "data")
        .and(warp::get())
        .and(with_rate_limit(rate_limit.clone()))
        .and_then(handle_request)
        .recover(handle_rejection);

    println!("Server running on http://127.0.0.1:3030");
    println!("Try these commands:");
    println!("  curl -i http://127.0.0.1:3030/api/data");
    println!("  # Run it multiple times to see rate limiting in action");

    warp::serve(api)
        .run(([127, 0, 0, 1], 3030))
        .await;
}

async fn handle_request(rate_limit_info: RateLimitInfo) -> Result<impl Reply, Rejection> {
    // Create a JSON response with some data
    let response_data = json!({
        "status": "success",
        "data": {
            "message": "Hello, JSON World!",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }
    });

    // Create the response with JSON payload
    let mut response = warp::reply::with_status(
        warp::reply::json(&response_data),
        StatusCode::OK,
    ).into_response();

    // Add rate limit headers
    let _ = add_rate_limit_headers(response.headers_mut(), &rate_limit_info);

    Ok(response)
}

// For this example, our replies will be json replies, so let's construct 
// a json reply example using the information from our rate limit rejection:
async fn handle_rejection(rejection: Rejection) -> Result<impl Reply, Infallible> {
    if let Some(rate_limit_rejection) = rejection.find::<RateLimitRejection>() {
        
        // Grab the rate limit info:
        let info = get_rate_limit_info(&rate_limit_rejection);

        // Create a json response based on that info:
        let mut json_response = warp::reply::with_status(
            warp::reply::json(&MyCustomError {
                error: format!("Rate limit exceeded. Try again after {:?}", rate_limit_rejection.retry_after),
                code: StatusCode::TOO_MANY_REQUESTS.as_u16()
            }),
            StatusCode::TOO_MANY_REQUESTS
        ).into_response(); // Convert it into a Response 
        
        // Add the rate limit headers:
        let _ = add_rate_limit_headers(json_response.headers_mut(), &info);

        Ok(json_response)

    } else {
        // Handle other rejections with JSON
        Ok(warp::reply::with_status(
            warp::reply::json(&MyCustomError {
                error: format!("Something went wrong."),
                code: 500
            }),
            StatusCode::TOO_MANY_REQUESTS
        ).into_response())
    }
}