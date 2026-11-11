use super::{
    config::{Config, Engine},
    fetch::{self, Reading},
    search,
};
use serde_json::json;

mod samples;

#[test]
fn invalid_configuration_is_rejected() {
    for invalid in [
        r#"{"search":["exa","exa"]}"#,
        r#"{"search":["jina"]}"#,
        r#"{"urls":{"other":"https://service.net"}}"#,
        r#"{"urls":{"jina":"file:///tmp/page"}}"#,
    ] {
        assert!(Config::parse(invalid).is_err());
    }
}

#[test]
fn an_untitled_result_is_returned_with_the_others() {
    let found = search::found(Engine::Exa, samples::EXA_SEARCH.as_bytes()).unwrap();
    assert_eq!(
        serde_json::to_value(found).unwrap(),
        json!([
            {
                "title": "Free Trial and Free Tier Services and Products | Google Cloud",
                "url": "https://cloud.google.com/free",
                "published": "2026-11-04T00:00:00.000Z",
                "summary": "New customers get $300 in free credits to fully explore Google Cloud.\nAll customers get free usage of 20+ products, up to monthly limits."
            },
            {
                "title": null,
                "url": "https://docs.cloud.google.com/free/docs/free-cloud-features",
                "published": "2026-11-10T00:00:00.000Z",
                "summary": null
            }
        ])
    );
}

#[test]
fn a_result_outside_its_engine_contract_fails_the_search() {
    let body = json!({"results": [{"title": "Untitled", "highlights": []}]}).to_string();
    let failure = search::found(Engine::Exa, body.as_bytes()).err().unwrap();
    assert_eq!(failure.code, "tool.failed");
    assert!(
        failure
            .message
            .starts_with("exa search response does not match its contract: missing field `url`"),
        "{}",
        failure.message
    );
}

#[test]
fn jina_reads_a_page_without_a_blank_warning_or_leaves_it_to_exa() {
    let url = "https://www.w3.org/Icons/w3c_home.png";
    let Reading::Page(page) = fetch::jina(url, samples::JINA_IMAGE.as_bytes()).unwrap() else {
        panic!("Jina read the image");
    };
    assert_eq!(
        page,
        format!(
            "Source: Jina Reader\nURL: {url}\n\nA 72x48 small image, likely a logo, icon or avatar"
        )
    );
    let missing = "https://example.com/does-not-exist-enco-probe";
    let Reading::Unavailable(reason) =
        fetch::jina(missing, samples::JINA_NOT_FOUND.as_bytes()).unwrap()
    else {
        panic!("the page answered with an error");
    };
    assert_eq!(reason.message, "page returned HTTP 404");
}

#[test]
fn exa_says_why_it_could_not_retrieve_a_page() {
    let url = "https://example.com/does-not-exist-enco-probe";
    let failure = fetch::exa(
        url,
        "page returned HTTP 404",
        samples::EXA_CONTENTS_FAILED.as_bytes(),
    )
    .err()
    .unwrap();
    assert_eq!(
        failure.message,
        "could not retrieve the page: CRAWL_NOT_FOUND (HTTP 404)"
    );
}
