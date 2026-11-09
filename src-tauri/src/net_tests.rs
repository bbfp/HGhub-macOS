use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

struct Fixture {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone, Copy)]
enum Behavior {
    Normal,
    Disconnect,
    Delay(Duration),
    Truncate,
}

impl Fixture {
    fn start(status: u16, body: &'static str, location: Option<String>) -> Self {
        Self::with_behavior(status, body, location, Behavior::Normal)
    }
    fn with_behavior(
        status: u16,
        body: &'static str,
        location: Option<String>,
        behavior: Behavior,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let scheme = if matches!(behavior, Behavior::Disconnect) {
            "https"
        } else {
            "http"
        };
        let base = format!("{scheme}://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen, stopping) = (requests.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                let (mut socket, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(3));
                        continue;
                    }
                };
                if matches!(behavior, Behavior::Disconnect) {
                    seen.lock()
                        .unwrap()
                        .push("TLS handshake closed before request".into());
                    continue;
                }
                // Darwin inherits O_NONBLOCK from the listening socket. A
                // temporary WouldBlock must not be mistaken for request EOF.
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).unwrap_or(0);
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .and_then(|value| value.parse::<usize>().ok())
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                    if bytes.len() > 65536 {
                        break;
                    }
                }
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&bytes).into_owned());
                let redirect = location
                    .as_ref()
                    .map(|url| format!("Location: {url}\r\n"))
                    .unwrap_or_default();
                if let Behavior::Delay(delay) = behavior {
                    std::thread::sleep(delay);
                }
                let length = body.len()
                    + if matches!(behavior, Behavior::Truncate) {
                        1000
                    } else {
                        0
                    };
                let reply = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n{redirect}Connection: close\r\n\r\n{body}");
                let _ = socket.write_all(reply.as_bytes());
            }
        });
        Self {
            base,
            requests,
            stop,
            worker: Some(worker),
        }
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap()
}
const OK: &str = r#"{"status":1,"data":{"items":[{"id":117}]}}"#;

#[test]
fn playback_url_accepts_current_stream_shape_and_legacy_shape() {
    let current = json!({
        "streams": {
            "main": "https://cdn.example/current.m3u8",
            "hevc": "https://cdn.example/current-hevc.m3u8"
        }
    });
    assert_eq!(
        playback_url(&current).as_deref(),
        Some("https://cdn.example/current.m3u8")
    );
    assert_eq!(
        playback_url_hevc(&current).as_deref(),
        Some("https://cdn.example/current-hevc.m3u8")
    );

    let legacy = json!({ "video_url": "https://cdn.example/legacy.m3u8" });
    assert_eq!(
        playback_url(&legacy).as_deref(),
        Some("https://cdn.example/legacy.m3u8")
    );
    assert!(playback_url(&json!({ "streams": { "main": "" } })).is_none());
}

#[tokio::test]
async fn validated_playback_reads_skip_empty_success_and_try_next_domain() {
    let empty = Fixture::start(200, r#"{"status":1,"data":{"streams":{"main":""}}}"#, None);
    let working = Fixture::start(
        200,
        r#"{"status":1,"data":{"streams":{"main":"https://cdn.example/ok.m3u8"}}}"#,
        None,
    );
    let body = json!({});
    let data = edge_at_validated(
        &client(),
        &[empty.base.clone(), working.base.clone()],
        "POST",
        "/api/videos/1/episodes/1/playback",
        Some(&body),
        None,
        |_| {},
        |reply| playback_url(&reply.data).is_some(),
        "播放接口未返回可用地址",
        true,
    )
    .await
    .unwrap();
    assert_eq!(
        playback_url(&data).as_deref(),
        Some("https://cdn.example/ok.m3u8")
    );
    assert_eq!(empty.count(), 1);
    assert_eq!(working.count(), 1);
}

#[tokio::test]
async fn anonymous_reads_try_transport_then_5xx_then_success_in_order() {
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let unavailable = format!("http://{}", dead.local_addr().unwrap());
    drop(dead);
    let failing = Fixture::start(503, "unavailable", None);
    let working = Fixture::start(200, OK, None);
    let unused = Fixture::start(200, OK, None);
    let domains = vec![
        unavailable,
        failing.base.clone(),
        working.base.clone(),
        unused.base.clone(),
    ];
    let mut probes = Vec::new();
    let data = edge_at(
        &client(),
        &domains,
        "GET",
        "/api/videos?page=1",
        None,
        None,
        |probe| probes.push(probe),
    )
    .await
    .unwrap();
    assert_eq!(data["items"][0]["id"], 117);
    assert_eq!(
        probes.iter().map(|probe| &probe.domain).collect::<Vec<_>>(),
        domains[..3].iter().collect::<Vec<_>>()
    );
    assert_eq!(unused.count(), 0);
    let request = working.requests.lock().unwrap()[0].clone();
    assert!(request.to_ascii_lowercase().contains("referer:"));
    assert!(!request.to_ascii_lowercase().contains("authorization:"));
}

#[tokio::test]
async fn client_and_business_errors_do_not_fall_back() {
    for (status, body) in [
        (401, "unauthorized"),
        (403, "forbidden"),
        (404, "missing"),
        (429, "rate limit"),
        (200, r#"{"status":0,"msg":"bad request"}"#),
        (200, "<html>challenge</html>"),
    ] {
        let first = Fixture::start(status, body, None);
        let second = Fixture::start(200, OK, None);
        let result = edge_at(
            &client(),
            &[first.base.clone(), second.base.clone()],
            "GET",
            "/api/videos",
            None,
            None,
            |_| {},
        )
        .await;
        if status == 200 && body == "<html>challenge</html>" {
            assert!(
                result.is_ok(),
                "HTML gateway should fail over to the second configured origin"
            );
            assert_eq!(second.count(), 1);
        } else {
            assert!(result.is_err(), "accepted {status}/{body}");
            assert_eq!(second.count(), 0);
        }
        assert_eq!(first.count(), 1);
    }
}

#[tokio::test]
async fn writes_and_token_reads_never_retry_or_send_to_a_second_domain() {
    for method in ["POST", "DELETE"] {
        let first = Fixture::start(503, "server error", None);
        let second = Fixture::start(200, OK, None);
        let body = json!({"password":"test-only-password"});
        let result = edge_at(
            &client(),
            &[first.base.clone(), second.base.clone()],
            method,
            "/api/account/login",
            Some(&body),
            None,
            |_| {},
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            first.count(),
            1,
            "write must not retry after uncertain server response"
        );
        assert_eq!(second.count(), 0, "credentials must stay on first domain");
    }
    let first = Fixture::start(503, "server error", None);
    let second = Fixture::start(200, OK, None);
    let result = edge_at(
        &client(),
        &[first.base.clone(), second.base.clone()],
        "GET",
        "/api/account/me",
        None,
        Some("test-token"),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(result["items"][0]["id"], 117);
    assert_eq!(first.count(), 1);
    assert_eq!(second.count(), 1);
    assert!(second.requests.lock().unwrap()[0].contains("Bearer test-token"));
}

#[tokio::test]
async fn authenticated_401_checks_each_configured_origin_once_and_explains_account_scope() {
    let first = Fixture::start(401, "unauthorized", None);
    let second = Fixture::start(401, "unauthorized", None);
    let error = edge_at(
        &client(),
        &[first.base.clone(), second.base.clone()],
        "GET",
        "/api/account/me",
        None,
        Some("test-token"),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(error.contains("账号在此域名不可用"));
    assert_eq!(first.count(), 1);
    assert_eq!(second.count(), 1);
}

#[tokio::test]
async fn login_redirect_fails_over_only_to_the_next_configured_origin() {
    let second = Fixture::start(200, OK, None);
    let first = Fixture::start(307, "", Some(format!("{}/api/edge/request", second.base)));
    let body = json!({"password":"test-only-password"});
    let result = edge_at(
        &client(),
        &[first.base.clone(), second.base.clone()],
        "POST",
        "/api/account/loginByPassword",
        Some(&body),
        Some("test-token"),
        |_| {},
    )
    .await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(first.count(), 1);
    assert_eq!(second.count(), 1);
    let forwarded = second.requests.lock().unwrap()[0].clone();
    let forwarded_lower = forwarded.to_ascii_lowercase();
    assert!(
        forwarded_lower.contains("authorization: bearer test-token"),
        "{forwarded}"
    );
    assert!(forwarded_lower.contains("content-length:"));
}

#[tokio::test]
async fn authenticated_reads_fail_over_after_a_connect_or_response_failure() {
    let first = Fixture::with_behavior(200, OK, None, Behavior::Disconnect);
    let second = Fixture::start(200, OK, None);
    let result = edge_at(
        &client(),
        &[first.base.clone(), second.base.clone()],
        "GET",
        "/api/account/me",
        None,
        Some("test-token"),
        |_| {},
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(first.count(), 1);
    assert_eq!(second.count(), 1);
    assert!(second.requests.lock().unwrap()[0].contains("Bearer test-token"));
    let second_before = second.count();
    let truncated = Fixture::with_behavior(200, OK, None, Behavior::Truncate);
    assert!(edge_at(
        &client(),
        &[truncated.base.clone(), second.base.clone()],
        "GET",
        "/api/account/me",
        None,
        Some("test-token"),
        |_| {}
    )
    .await
    .is_ok());
    assert_eq!(
        truncated.count(),
        1,
        "response body failure is not a connect failure"
    );
    assert_eq!(second.count(), second_before + 1);
}

#[tokio::test]
async fn write_timeout_or_incomplete_response_is_unknown_and_never_resubmitted() {
    let short_client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(50))
        .build()
        .unwrap();
    for behavior in [
        Behavior::Delay(Duration::from_millis(200)),
        Behavior::Truncate,
    ] {
        let first = Fixture::with_behavior(200, OK, None, behavior);
        let second = Fixture::start(200, OK, None);
        let error = edge_at(
            &short_client,
            &[first.base.clone(), second.base.clone()],
            "POST",
            "/api/comment",
            None,
            Some("test-token"),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(error.contains("结果未知"), "{error}");
        assert_eq!(first.count(), 1);
        assert_eq!(second.count(), 0);
    }
}

#[test]
fn proxy_diagnostics_omit_usernames_and_passwords() {
    assert_eq!(
        redact_proxy("http://test-user:test-password@127.0.0.1:1082"),
        "http://127.0.0.1:1082/"
    );
    assert!(!redact_proxy("invalid test-password proxy").contains("test-password"));
    assert!(!redact_proxy("invalid:test-password").contains("test-password"));
}
