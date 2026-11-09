use super::*;

#[test]
fn domains_accept_bare_hosts_and_deduplicate_https_origins() {
    let domains = normalize_domains(&[
        " OLFS.gitiecgad.cc ".into(),
        "https://olfs.gitiecgad.cc/".into(),
        "https://y6wb.gitiecgad.cc:443".into(),
    ])
    .unwrap();
    assert_eq!(
        domains,
        vec!["https://olfs.gitiecgad.cc", "https://y6wb.gitiecgad.cc"]
    );
    assert_eq!(defaults().domains.len(), 4);
    assert!(normalize_domains(&[]).is_err());
}

#[test]
fn domains_reject_non_https_paths_credentials_and_local_addresses() {
    for value in [
        "http://example.com",
        "https://example.com/api",
        "https://example.com?q=1",
        "https://user:password@example.com",
        "https://example.com:8443",
        "https://example.com#fragment",
        "https://127.0.0.1",
        "https://[::1]",
        "localhost",
        "api.localhost",
        "printer.local",
        "example.com.",
        "invalid",
    ] {
        assert!(normalize_domain(value).is_err(), "accepted {value}");
    }
}

#[test]
fn successful_read_domain_is_reused_without_changing_account_domain() {
    let domains = defaults().domains;
    let mirror = &domains[2];
    let reads = ordered_domains(&domains, Some(mirror), true);
    assert_eq!(&reads[0], mirror);
    assert_eq!(reads.len(), domains.len());
    assert_eq!(
        ordered_domains(&domains, Some(mirror), false),
        vec![domains[0].clone()]
    );
    assert_eq!(
        ordered_domains(&domains, Some("https://removed.example.com"), true),
        domains
    );
}

#[test]
fn rank_probes_puts_fast_working_domains_first_and_keeps_configured_ties() {
    let configured = vec![
        "https://slow.example.com".to_string(),
        "https://failed.example.com".to_string(),
        "https://fast.example.com".to_string(),
        "https://tie.example.com".to_string(),
    ];
    let ranked = rank_probes(
        &configured,
        vec![
            DomainProbe {
                domain: configured[0].clone(),
                ok: true,
                status: Some(200),
                elapsed_ms: 180,
                error: None,
            },
            DomainProbe {
                domain: configured[1].clone(),
                ok: false,
                status: None,
                elapsed_ms: 5,
                error: Some("连接失败".into()),
            },
            DomainProbe {
                domain: configured[2].clone(),
                ok: true,
                status: Some(200),
                elapsed_ms: 30,
                error: None,
            },
            DomainProbe {
                domain: configured[3].clone(),
                ok: true,
                status: Some(200),
                elapsed_ms: 30,
                error: None,
            },
        ],
    );
    assert_eq!(
        ranked
            .iter()
            .map(|probe| probe.domain.as_str())
            .collect::<Vec<_>>(),
        vec![
            "https://fast.example.com",
            "https://tie.example.com",
            "https://slow.example.com",
            "https://failed.example.com",
        ]
    );
}

#[test]
fn damaged_network_settings_are_preserved_and_future_schema_is_not_overwritten() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local/tests")
        .join(format!("network-{}", crate::downloads::unique_stamp()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("network.json");
    std::fs::write(&path, b"{broken").unwrap();
    let loaded = load_settings_file(&path);
    assert!(loaded.warning.is_some());
    assert_eq!(loaded.settings, defaults());
    let backup = std::fs::read_dir(&dir)
        .unwrap()
        .map(Result::unwrap)
        .find(|entry| entry.file_name().to_string_lossy().contains("corrupt-"))
        .unwrap();
    assert_eq!(std::fs::read(backup.path()).unwrap(), b"{broken");
    let future =
        br#"{"schema_version":99,"domains":["https://future.example.com"],"new_field":true}"#;
    std::fs::write(&path, future).unwrap();
    let loaded = load_settings_file(&path);
    assert!(loaded.warning.is_some());
    assert_eq!(std::fs::read(&path).unwrap(), future);
    let future_shape =
        br#"{"schema_version":99,"domains":{"primary":"https://future.example.com"}}"#;
    std::fs::write(&path, future_shape).unwrap();
    let loaded = load_settings_file(&path);
    assert!(loaded.warning.is_some());
    assert_eq!(std::fs::read(&path).unwrap(), future_shape);
    let custom = NetworkSettings {
        schema_version: 1,
        domains: vec!["https://y6wb.gitiecgad.cc".into()],
    };
    assert!(write_settings_file(&path, &custom).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), future_shape);
    std::fs::remove_file(&path).unwrap();
    write_settings_file(&path, &custom).unwrap();
    assert_eq!(load_settings_file(&path).settings, custom);
}

#[test]
fn mirrors_are_used_only_on_macos() {
    let custom = vec![
        "https://user.example.com".into(),
        "https://backup.example.com".into(),
    ];
    assert_eq!(
        platform_domains(false, &custom, Some(&custom[1]), true),
        vec![PRIMARY_DOMAIN.to_string()]
    );
    assert_eq!(
        platform_domains(true, &custom, Some(&custom[1]), true)[0],
        custom[1]
    );
}

#[test]
fn site_origin_rewrite_preserves_signed_path_and_query_but_leaves_cdn_alone() {
    let source = "https://huangguoai.com/video/117/play?auth_key=abc%2F123&host=huangguoai.com";
    assert!(is_site_origin(source));
    let rewritten = replace_site_origin(source, "https://olfs.gitiecgad.cc").unwrap();
    assert_eq!(
        rewritten,
        "https://olfs.gitiecgad.cc/video/117/play?auth_key=abc%2F123&host=huangguoai.com"
    );
    let cdn = "https://yd-lphu-hls.hzjvlo.cn/videos/a.m3u8?auth_key=abc";
    assert!(!is_site_origin(cdn));
    assert_eq!(
        routed_urls(cdn, true),
        vec![(current_domain(), cdn.to_string())]
    );
}

#[tokio::test]
#[ignore = "live read-only mirror probes; requires external network"]
async fn live_default_domains_return_video_lists() {
    let result = probe_domains(defaults().domains).await.unwrap();
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    assert!(
        result.probes.iter().skip(1).all(|probe| probe.ok),
        "one or more user-provided mirrors failed"
    );
    let feed = crate::net::edge("GET", "/api/videos?page=1&size=1", None, None)
        .await
        .unwrap();
    assert!(feed["items"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    println!("active_read_domain={}", current_domain());
}
