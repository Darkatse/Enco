//! Verifies the mapper's HTTP trust boundary without making requests.
use super::{request_for, wire};

fn request(path: &str) -> wire::Request {
    wire::Request {
        method: "GET".into(),
        path: path.into(),
        headers: vec![],
        body: None,
        timeout_ms: None,
    }
}

#[test]
fn request_paths_stay_within_the_configured_service_and_base_path() {
    let base = reqwest::Url::parse("https://api.example/service/").unwrap();
    let valid = request_for(&base, request("/bot/getUpdates?offset=1")).unwrap();
    assert_eq!(
        valid.url,
        "https://api.example/service/bot/getUpdates?offset=1"
    );
    for path in [
        "https://other.example/a",
        "//other.example/a",
        "/../outside",
        "/%2e%2e/outside",
        "/a#fragment",
        "/\\other.example",
    ] {
        assert!(request_for(&base, request(path)).is_err(), "{path}");
    }
}
