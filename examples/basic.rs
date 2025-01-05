use std::convert::Infallible;
use warp::{Filter, Rejection, Reply, http::StatusCode};
use warp_rate_limit::*;

#[tokio::main]
async fn main() {
    // Let's set up a rate limit configuration of 5 requests per 30 seconds:
    let rate_limit = RateLimitConfig {
        max_requests: 5,
        window: std::time::Duration::from_secs(30),
        retry_after_format: RetryAfterFormat::HttpDate,
    };

    // We'll have a single route, /hello, that will be rate limited:
    let hello = warp::path!("hello")
        .and(warp::get())
        .and(with_rate_limit(rate_limit.clone())) // Here we add the rate-limiter
        .and_then(handle_request)                 // Hand-off to route handler
        .recover(handle_rejection);               // Idiomatic warp rejection handling

    println!("Server running on http://127.0.0.1:3030");
    println!("Try these commands:");
    println!("  curl http://127.0.0.1:3030/hello");
    println!("  # Run it multiple times to see rate limiting in action");

    warp::serve(hello)
        .run(([127, 0, 0, 1], 3030))
        .await;
}

// This is the handler for the /hello path in this example.
async fn handle_request(rate_limit_info: RateLimitInfo) -> Result<impl Reply, Rejection> {
    // Create the base response
    let mut response = warp::reply::with_status(
        "Hello, World!", 
        StatusCode::OK
    ).into_response();

    // (Optional) Let's add some headers related to rate-limiting so that consumers of 
    // our /hello endpoint have that info available:
    if let Err(e) = add_rate_limit_headers(response.headers_mut(), &rate_limit_info) {
        match e {
            RateLimitError::HeaderError(e) => {
                eprintln!("Failed to set rate limit headers due to invalid value: {}", e);
            }
            RateLimitError::Other(e) => {
                eprintln!("Unexpected error setting rate limit headers: {}", e);
            }
        }
    }

    Ok(response)
}

// Here is an example of a rejection handler that we've included some of 
// this library's affordances in:
async fn handle_rejection(rejection: Rejection) -> Result<impl Reply, Infallible> {
    
    // let's handle rate limit rejections specifically:
    if let Some(rate_limit_rejection) = rejection.find::<RateLimitRejection>() {
        // We have a rate limit rejection -- so let's get some info about it: 
        let info = get_rate_limit_info(rate_limit_rejection);
        
        // Let's use that info to create a response:
        let message = format!(
            "Rate limit exceeded. Try again after {}.", 
            info.retry_after
        );
        
        // Let's build that response:
        let mut response = warp::reply::with_status(
            message,
            StatusCode::TOO_MANY_REQUESTS
        ).into_response();

        // Then, let's add the rate-limiting headers to that response:

        // If you really don't care about the error, you can do something like this:
        // let _ = add_rate_limit_headers(response.headers_mut(), &info);

        // If you care about the fact that it errored but not necessarily
        // the specific error itself, you can do something like this:
        // if !add_rate_limit_headers(response.headers_mut(), &info).is_ok() {
        //     eprintln!("Failed to set headers");
        // }

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

        // Finally, let's produce the response:
        Ok(response)
        
    } else {
        // Handle other types of rejections
        Ok(warp::reply::with_status(
            "Internal Server Error",
            StatusCode::INTERNAL_SERVER_ERROR,
        ).into_response())
    }
}