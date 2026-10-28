//! Integration tests for dynamic PAT injection, rate limit elevation, and GET /rate_limit endpoint.

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use woderrise_github::{GitHubClient, RateLimitTracker};

#[test]
fn test_rate_limit_tracker_elevation_and_reversion() {
    let tracker = RateLimitTracker::new();

    // Verify initial unauthenticated defaults
    let initial_core = tracker.core_status();
    let initial_search = tracker.search_status();
    assert_eq!(initial_core.limit, 60);
    assert_eq!(initial_core.remaining, 60);
    assert_eq!(initial_search.limit, 10);
    assert_eq!(initial_search.remaining, 10);

    // Elevate to authenticated limits
    tracker.set_authenticated(true);
    let elevated_core = tracker.core_status();
    let elevated_search = tracker.search_status();
    assert_eq!(elevated_core.limit, 5000);
    assert_eq!(elevated_core.remaining, 5000);
    assert_eq!(elevated_search.limit, 30);
    assert_eq!(elevated_search.remaining, 30);

    // Revert back to unauthenticated limits
    tracker.set_authenticated(false);
    let reverted_core = tracker.core_status();
    let reverted_search = tracker.search_status();
    assert_eq!(reverted_core.limit, 60);
    assert_eq!(reverted_core.remaining, 60);
    assert_eq!(reverted_search.limit, 10);
    assert_eq!(reverted_search.remaining, 10);
}

#[tokio::test]
async fn test_client_dynamic_pat_injection_and_get_rate_limit() {
    let mock_server = MockServer::start().await;

    // 1. Initial request to /rate_limit with bearer token
    Mock::given(method("GET"))
        .and(path("/rate_limit"))
        .and(header("Authorization", "Bearer ghp_dynamic_token_12345"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "resources": {
                        "core": {
                            "limit": 5000,
                            "remaining": 4980,
                            "reset": 1761500000,
                            "used": 20
                        },
                        "search": {
                            "limit": 30,
                            "remaining": 28,
                            "reset": 1761500000,
                            "used": 2
                        }
                    },
                    "rate": {
                        "limit": 5000,
                        "remaining": 4980,
                        "reset": 1761500000,
                        "used": 20
                    }
                }))
                .append_header("x-ratelimit-resource", "core")
                .append_header("x-ratelimit-limit", "5000")
                .append_header("x-ratelimit-remaining", "4980")
                .append_header("x-ratelimit-reset", "1761500000"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    // Initialize client unauthenticated
    let mut client = GitHubClient::builder()
        .base_url(mock_server.uri())
        .cdn_base_url(mock_server.uri())
        .build()
        .expect("Client build failed");

    assert_eq!(client.rate_limiter().core_status().limit, 60);
    assert_eq!(client.rate_limiter().search_status().limit, 10);

    // Dynamically inject token
    client.set_auth_token(Some("ghp_dynamic_token_12345".to_string()));

    // Verify tracker immediately elevated
    assert_eq!(client.rate_limiter().core_status().limit, 5000);
    assert_eq!(client.rate_limiter().search_status().limit, 30);

    // Query get_rate_limit
    let (core_status, search_status) = client.get_rate_limit().await.expect("get_rate_limit ok");
    assert_eq!(core_status.limit, 5000);
    assert_eq!(core_status.remaining, 4980);
    assert_eq!(search_status.limit, 30);
    assert_eq!(search_status.remaining, 28);

    struct NoAuthHeaderMatcher;

    impl wiremock::Match for NoAuthHeaderMatcher {
        fn matches(&self, request: &wiremock::Request) -> bool {
            !request
                .headers
                .contains_key(wiremock::http::HeaderName::from_static("authorization"))
        }
    }

    // 2. Clear token and verify subsequent request has no Authorization header
    Mock::given(method("GET"))
        .and(path("/rate_limit"))
        .and(NoAuthHeaderMatcher)
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "resources": {
                "core": {
                    "limit": 60,
                    "remaining": 55,
                    "reset": 1761500000,
                    "used": 5
                },
                "search": {
                    "limit": 10,
                    "remaining": 9,
                    "reset": 1761500000,
                    "used": 1
                }
            },
            "rate": {
                "limit": 60,
                "remaining": 55,
                "reset": 1761500000,
                "used": 5
            }
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    client.set_auth_token(None);
    assert_eq!(client.rate_limiter().core_status().limit, 60);
    assert_eq!(client.rate_limiter().search_status().limit, 10);

    let (reverted_core, reverted_search) = client
        .get_rate_limit()
        .await
        .expect("get_rate_limit unauth ok");
    assert_eq!(reverted_core.limit, 60);
    assert_eq!(reverted_core.remaining, 55);
    assert_eq!(reverted_search.limit, 10);
    assert_eq!(reverted_search.remaining, 9);
}
