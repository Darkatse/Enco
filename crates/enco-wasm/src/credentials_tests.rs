use super::*;

#[test]
fn credentials_are_origin_scoped_and_redaction_preserves_binary_data() {
    let credential = CallCredential {
        placements: vec![CallPlacement {
            origin: Origin::parse("https://api.example:443").unwrap(),
            destination: CallDestination::Header {
                header: HeaderName::from_static("x-api-key"),
                value: HeaderValue::from_static("a-secret"),
            },
        }],
        secret: "a-secret".into(),
        replacement: "[redacted:KEY]".into(),
    };
    let client = reqwest::Client::new();
    for (url, allowed) in [
        ("https://api.example/next", true),
        ("http://api.example/next", false),
        ("https://api.example:444/next", false),
        ("https://other.example/next", false),
    ] {
        let mut request = client
            .get(url)
            .header("x-api-key", "guest-value")
            .build()
            .unwrap();
        credential.attach(&mut request);
        assert_eq!(
            request.headers()["x-api-key"],
            if allowed { "a-secret" } else { "guest-value" }
        );
    }
    let mut body = b"\xffa-secret\x00a-secret\xfe".to_vec();
    credential.redact_body(&mut body);
    assert_eq!(body, b"\xff[redacted:KEY]\x00[redacted:KEY]\xfe");
}

#[test]
fn path_credentials_only_enter_declared_prefixes_at_the_bound_origin() {
    let credential = CallCredential {
        placements: vec![CallPlacement {
            origin: Origin::parse("https://api.example").unwrap(),
            destination: CallDestination::Path(vec!["/bot".into(), "/file/bot".into()]),
        }],
        secret: "42:test-secret".into(),
        replacement: "[redacted:TOKEN]".into(),
    };
    let client = reqwest::Client::new();
    for (url, expected) in [
        (
            "https://api.example/bot/getMe?x=1",
            "https://api.example/bot42:test-secret/getMe?x=1",
        ),
        (
            "https://api.example/file/bot/images/photo.jpg",
            "https://api.example/file/bot42:test-secret/images/photo.jpg",
        ),
        ("https://api.example/other", "https://api.example/other"),
        (
            "https://other.example/bot/getMe",
            "https://other.example/bot/getMe",
        ),
    ] {
        let mut request = client.get(url).build().unwrap();
        credential.attach(&mut request);
        assert_eq!(request.url().as_str(), expected);
    }
    let mut response = Ok(host::HttpResponse {
        status: 200,
        headers: vec![(
            "location".into(),
            "https://api.example/bot42:test-secret/a".into(),
        )],
        body: b"\xff42:test-secret\x00".to_vec(),
    });
    credential.redact(&mut response);
    let response = response.unwrap();
    assert_eq!(
        response.headers[0].1,
        "https://api.example/bot[redacted:TOKEN]/a"
    );
    assert_eq!(response.body, b"\xff[redacted:TOKEN]\x00");
}

#[test]
fn frame_credentials_only_fill_null_at_their_own_origin() {
    let credentials = vec![CallCredential {
        placements: vec![CallPlacement {
            origin: Origin::parse("wss://*.discord.gg").unwrap(),
            destination: CallDestination::Frame("/d/token".into()),
        }],
        secret: "secret\"value".into(),
        replacement: "[redacted:TOKEN]".into(),
    }];
    let gateway = Url::parse("wss://gateway.discord.gg/?v=10").unwrap();
    let frame = r#"{"op":2,"d":{"token":null}}"#;
    let attached = attach_frame(&credentials, &gateway, frame).unwrap();
    let value: serde_json::Value = serde_json::from_str(&attached).unwrap();
    assert_eq!(value["d"]["token"], "secret\"value");
    for url in [
        "wss://discord.gg/",
        "wss://cdn.example/",
        "https://gateway.discord.gg/",
    ] {
        assert_eq!(
            attach_frame(&credentials, &Url::parse(url).unwrap(), frame).unwrap(),
            frame
        );
    }
    assert!(attach_frame(&credentials, &gateway, r#"{"op":1,"d":42}"#).is_ok());
    assert!(attach_frame(&credentials, &gateway, r#"{"d":{"token":"guest"}}"#).is_err());
    assert!(attach_frame(&credentials, &gateway, "not JSON").is_err());
    let mut echoed = "secret\"value".to_owned();
    credentials[0].redact_text(&mut echoed);
    assert_eq!(echoed, "[redacted:TOKEN]");
}

#[test]
fn credential_declarations_validate_all_placements_once() {
    let mut declaration = enco_kernel::Credential {
        name: "token".into(),
        required: true,
        placements: vec![
            enco_kernel::Placement {
                origin: None,
                location: enco_kernel::Location::Header(enco_kernel::HeaderPlacement {
                    name: "Authorization".into(),
                    prefix: "Bot ".into(),
                }),
            },
            enco_kernel::Placement {
                origin: Some("wss://*.discord.gg".into()),
                location: enco_kernel::Location::Frame("/d/token".into()),
            },
        ],
    };
    assert_eq!(
        declarations(&[declaration.clone()]).unwrap()[0]
            .placements
            .len(),
        2
    );
    for pointer in ["d/token", "/d/~token", "/d/token~"] {
        declaration.placements[1].location = enco_kernel::Location::Frame(pointer.into());
        assert!(declarations(&[declaration.clone()]).is_err());
    }
    declaration.placements.clear();
    assert!(declarations(&[declaration]).is_err());
}

#[test]
fn caller_selected_secret_is_shared_with_fixed_recipient_placements() {
    let declared = declarations(&[enco_kernel::Credential {
        name: "token".into(),
        required: true,
        placements: vec![
            enco_kernel::Placement {
                origin: None,
                location: Location::Header(enco_kernel::HeaderPlacement {
                    name: "authorization".into(),
                    prefix: "Bot ".into(),
                }),
            },
            enco_kernel::Placement {
                origin: Some("wss://*.gateway.example".into()),
                location: Location::Frame("/token".into()),
            },
        ],
    }])
    .unwrap();
    let endpoint = ProviderSettings {
        base_url: "https://api.example/v1".into(),
        api_key_env: Some("CARGO_MANIFEST_DIR".into()),
        model: "test".into(),
        options: serde_json::json!({}),
    };
    let settings = PluginSettings {
        secrets: [("token".into(), "ENCO_UNMAPPED_FIXED_CREDENTIAL".into())].into(),
        ..Default::default()
    };
    let credentials = resolve(&declared, &settings, Some(&endpoint)).unwrap();
    let secret = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut request = reqwest::Client::new()
        .get("https://api.example/v1/account")
        .build()
        .unwrap();
    credentials[0].attach(&mut request);
    assert_eq!(request.headers()["authorization"], format!("Bot {secret}"));
    let frame = attach_frame(
        &credentials,
        &Url::parse("wss://one.gateway.example").unwrap(),
        r#"{"token":null}"#,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&frame).unwrap()["token"],
        secret
    );
    let mut cdn = reqwest::Client::new()
        .get("https://cdn.example/file")
        .build()
        .unwrap();
    credentials[0].attach(&mut cdn);
    assert!(!cdn.headers().contains_key("authorization"));
}
