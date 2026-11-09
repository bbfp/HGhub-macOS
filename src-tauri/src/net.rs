//! 网络层：与上游站点对话。
//!
//! 站点把全部 `/api/**` 调用收敛成 `POST /api/edge/request` 的信封，
//! 我们只实现这一个入口，其余接口都是「填 path」。
//!
//! # 代理（用户反馈「浏览器能开、HGhub 打不开」的根因就在这里）
//!
//! 客户端的流量**全部**走本文件的 reqwest，跟 WebView2 无关，所以它既不共享
//! 浏览器的代理设置，也不共享浏览器的证书库：
//!
//! * reqwest 只有开了 `system-proxy` feature 才会读 Windows 系统代理
//!   （`HKCU\...\Internet Settings` 的 `ProxyEnable`/`ProxyServer`/`ProxyOverride`）。
//!   本 crate 用 `default-features = false`，早先漏了这个 feature ⇒ 永远直连，
//!   只有 TUN/虚拟网卡这种网络层接管才「看起来能用」。
//! * 即使开了 `system-proxy`，PAC（`AutoConfigURL`）也不被支持，只认固定代理。
//! * Android 没有注册表可读，系统「Wi-Fi 代理」同样不被 reqwest 采用；
//!   安卓上只有 VPN/TUN 模式（VpnService 抓全流量）才有效。
//!
//! 排查手段：环境变量 `HGHUB_PROXY=http://127.0.0.1:7897` 可强制指定代理，
//! `net_diag` 命令会输出 DNS/HTTP/环境变量快照。

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const BASE: &str = crate::network::PRIMARY_DOMAIN;

pub const UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// 手动指定代理（排查 / 公司内网用）：`HGHUB_PROXY=http://127.0.0.1:7897`。
/// 不设时交给 reqwest 的 `system-proxy` 探测（注册表 + 环境变量）。
fn proxy_override() -> Option<String> {
    std::env::var("HGHUB_PROXY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn build_client(api: bool) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .gzip(true)
        .brotli(true)
        .pool_idle_timeout(Duration::from_secs(60));
    if api {
        // Do not let 307/308 redirects forward login bodies to another origin.
        // Media/CDN redirects continue to use the existing client behavior.
        builder = builder
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5));
    }
    if let Some(p) = proxy_override() {
        match reqwest::Proxy::all(&p) {
            Ok(proxy) => builder = builder.proxy(proxy),
            Err(_) => eprintln!("[net] 忽略无效的 HGHUB_PROXY：代理地址格式无效"),
        }
    }
    builder.build().expect("build http client")
}

pub static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| build_client(false));
pub(crate) static API_CLIENT: Lazy<reqwest::Client> =
    Lazy::new(|| build_client(cfg!(target_os = "macos")));

fn redact_proxy(value: &str) -> String {
    match url::Url::parse(value) {
        Ok(mut url) if url.has_host() => {
            let _ = url.set_password(None);
            let _ = url.set_username("");
            url.to_string()
        }
        _ => "[已设置，地址格式无效]".into(),
    }
}

fn proxy_env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|value| redact_proxy(&value))
}

/// reqwest 的 `Display` 只有「error sending request for url (...)」，
/// 真正的原因（dns error / connection refused / invalid peer certificate）
/// 藏在 `source()` 链里。展开成一行，界面和 debug.log 才有排查价值。
pub fn err_text(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        let t = s.to_string();
        if !t.is_empty() && !out.contains(&t) {
            out.push_str(" ← ");
            out.push_str(&t);
        }
        src = s.source();
    }
    out
}

/// 网络自检：DNS / HTTP / 代理相关环境变量，用于远程定位「打不开」。
pub async fn diagnose() -> Value {
    let active = crate::network::current_domain();
    let host = url::Url::parse(&active)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default();

    // fake-ip（198.18.0.0/15）说明 DNS 被 TUN 接管：关掉 TUN 后这个地址根本不可路由。
    let dns = {
        let host = host.clone();
        tokio::task::spawn_blocking(move || {
            use std::net::ToSocketAddrs;
            match (host.as_str(), 443u16).to_socket_addrs() {
                Ok(iter) => json!(iter.map(|a| a.ip().to_string()).collect::<Vec<_>>()),
                Err(e) => json!(format!("解析失败: {e}")),
            }
        })
        .await
        .unwrap_or(Value::Null)
    };

    let started = Instant::now();
    let http = match API_CLIENT.get(format!("{active}/")).send().await {
        Ok(r) => json!({
            "status": r.status().as_u16(),
            "ms": started.elapsed().as_millis() as u64,
        }),
        Err(e) => json!({
            "error": err_text(&e),
            "ms": started.elapsed().as_millis() as u64,
        }),
    };

    json!({
        "base": BASE,
        "active_base": active,
        "network": crate::network::settings_view(),
        "host": host,
        "dns": dns,
        "http": http,
        "env": {
            "HTTP_PROXY": proxy_env("HTTP_PROXY"),
            "HTTPS_PROXY": proxy_env("HTTPS_PROXY"),
            "ALL_PROXY": proxy_env("ALL_PROXY"),
            "NO_PROXY": std::env::var("NO_PROXY").ok(),
            "HGHUB_PROXY": proxy_override().map(|value| redact_proxy(&value)),
        },
    })
}

/// Failures carry a retry decision separately from their display text.
#[derive(Debug)]
pub(crate) struct EdgeFailure {
    pub message: String,
    pub status: Option<u16>,
    retryable: bool,
}
impl EdgeFailure {
    fn transport(error: reqwest::Error, status: Option<u16>, method: &str) -> Self {
        let unknown_write = method != "GET" && !error.is_connect() && !error.is_builder();
        let message = if unknown_write {
            format!("提交结果未知，请刷新确认后再操作。{}", err_text(&error))
        } else {
            err_text(&error)
        };
        Self {
            message,
            status,
            retryable: if method == "GET" {
                !error.is_builder() && !error.is_redirect()
            } else {
                error.is_connect()
            },
        }
    }
    fn terminal(message: String, status: Option<u16>) -> Self {
        Self {
            message,
            status,
            retryable: false,
        }
    }
}

pub(crate) struct EdgeResponse {
    pub status: u16,
    pub data: Value,
}

pub(crate) async fn edge_once(
    client: &reqwest::Client,
    domain: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
    token: Option<&str>,
) -> Result<EdgeResponse, EdgeFailure> {
    let (encoded, ctype) = match body {
        Some(value) => (
            B64.encode(
                serde_json::to_vec(value)
                    .map_err(|error| EdgeFailure::terminal(error.to_string(), None))?,
            ),
            "application/json",
        ),
        None => (String::new(), ""),
    };
    let envelope = json!({"method": method, "path": path, "body": encoded, "content_type": ctype});
    let mut request = client
        .post(format!("{domain}/api/edge/request"))
        .header("Accept", "application/json")
        .header("Referer", format!("{domain}/"))
        .json(&envelope);
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    if cfg!(target_os = "macos") && (method == "GET" || path.ends_with("loginByPassword")) {
        request = request.timeout(Duration::from_secs(10));
    }
    let response = request
        .send()
        .await
        .map_err(|error| EdgeFailure::transport(error, None, method))?;
    let status = response.status();
    if !status.is_success() {
        let retryable = match status.as_u16() {
            300..=399 => method == "GET" || path.ends_with("loginByPassword"),
            500..=599 => method == "GET" || path.ends_with("loginByPassword"),
            401 | 403 => token.is_some(),
            _ => false,
        };
        let message =
            if method != "GET" && status.is_server_error() && !path.ends_with("loginByPassword") {
                format!("提交结果未知，请刷新确认后再操作。HTTP {status}")
            } else if matches!(status.as_u16(), 401 | 403) {
                "401".into()
            } else {
                format!("HTTP {status}")
            };
        return Err(EdgeFailure {
            message,
            status: Some(status.as_u16()),
            retryable,
        });
    }
    let text = response
        .text()
        .await
        .map_err(|error| EdgeFailure::transport(error, Some(status.as_u16()), method))?;
    let data = if text.trim().is_empty() && method != "GET" {
        Value::Null
    } else {
        // Do not log raw response bodies: account responses may contain tokens.
        let value: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(error) if method == "GET" => {
                return Err(EdgeFailure {
                    message: format!("接口未返回有效 JSON：{error}"),
                    status: Some(status.as_u16()),
                    retryable: true,
                });
            }
            Err(error) => {
                return Err(EdgeFailure::terminal(
                    format!("接口未返回有效 JSON：{error}"),
                    Some(status.as_u16()),
                ))
            }
        };
        if value.get("status").and_then(Value::as_i64) != Some(1) {
            return Err(EdgeFailure::terminal(
                value
                    .get("msg")
                    .and_then(Value::as_str)
                    .filter(|message| !message.is_empty())
                    .unwrap_or("接口返回失败")
                    .to_string(),
                Some(status.as_u16()),
            ));
        }
        value.get("data").cloned().unwrap_or(Value::Null)
    };
    Ok(EdgeResponse {
        status: status.as_u16(),
        data,
    })
}

async fn edge_at(
    client: &reqwest::Client,
    domains: &[String],
    method: &str,
    path: &str,
    body: Option<&Value>,
    token: Option<&str>,
    on_probe: impl FnMut(crate::network::DomainProbe),
) -> Result<Value, String> {
    edge_at_validated(
        client,
        domains,
        method,
        path,
        body,
        token,
        on_probe,
        |_| true,
        "",
        false,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn edge_at_validated(
    client: &reqwest::Client,
    domains: &[String],
    method: &str,
    path: &str,
    body: Option<&Value>,
    token: Option<&str>,
    mut on_probe: impl FnMut(crate::network::DomainProbe),
    validate: impl Fn(&EdgeResponse) -> bool,
    invalid_message: &str,
    retry_errors: bool,
) -> Result<Value, String> {
    let count = domains.len();
    let mut last = String::from("没有可用的 API 域名");
    for domain in domains.iter().take(count) {
        let started = Instant::now();
        let result = edge_once(client, domain, method, path, body, token)
            .await
            .map_err(|mut error| {
                if retry_errors {
                    error.retryable = true;
                }
                error
            })
            .and_then(|reply| {
                if validate(&reply) {
                    Ok(reply)
                } else {
                    Err(EdgeFailure {
                        message: invalid_message.to_string(),
                        status: Some(reply.status),
                        retryable: true,
                    })
                }
            });
        on_probe(crate::network::DomainProbe {
            domain: domain.clone(),
            ok: result.is_ok(),
            status: result
                .as_ref()
                .map(|reply| Some(reply.status))
                .unwrap_or_else(|error| error.status),
            elapsed_ms: started.elapsed().as_millis() as u64,
            error: result.as_ref().err().map(|error| error.message.clone()),
        });
        match result {
            Ok(reply) => return Ok(reply.data),
            Err(error) if !error.retryable => return Err(error.message),
            Err(error) => {
                last = if token.is_some() && matches!(error.status, Some(401 | 403)) {
                    format!("{domain}：账号在此域名不可用")
                } else {
                    format!("{domain}：{}", error.message)
                };
            }
        }
    }
    Err(format!("已尝试 {count} 个 API 域名，均未连接成功。{last}"))
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(crate) fn playback_url(value: &Value) -> Option<String> {
    non_empty_string(value.pointer("/streams/main"))
        .or_else(|| non_empty_string(value.get("video_url")))
}

pub(crate) fn playback_url_hevc(value: &Value) -> Option<String> {
    non_empty_string(value.pointer("/streams/hevc"))
        .or_else(|| non_empty_string(value.get("video_url_hevc")))
}

pub(crate) async fn fetch_playback(
    id: i64,
    episode: u32,
    token: Option<&str>,
) -> Result<Value, String> {
    crate::network::ensure_startup_probe().await?;
    let modern_path = format!("/api/videos/{id}/episodes/{episode}/playback");
    let body = json!({});
    let domains = crate::network::request_domains(true);
    let modern = edge_at_validated(
        &API_CLIENT,
        &domains,
        "POST",
        &modern_path,
        Some(&body),
        token,
        |probe| crate::network::record_probe(probe, true),
        |reply| playback_url(&reply.data).is_some(),
        "播放接口未返回可用地址",
        true,
    )
    .await;
    match modern {
        Ok(value) => Ok(value),
        Err(modern_error) => {
            // Keep older mirrors usable while they migrate to the current
            // episode playback contract.
            let legacy_path = format!("/api/videos/{id}/play?ep={episode}&page_texts=0");
            let legacy = edge_at_validated(
                &API_CLIENT,
                &domains,
                "GET",
                &legacy_path,
                None,
                token,
                |probe| crate::network::record_probe(probe, true),
                |reply| playback_url(&reply.data).is_some(),
                "旧播放接口未返回播放地址",
                true,
            )
            .await;
            match legacy {
                Ok(value) => Ok(value),
                Err(legacy_error) => Err(format!("{modern_error}；旧播放接口：{legacy_error}")),
            }
        }
    }
}

/// Every configured site origin is a candidate. A request that has not reached
/// a server may move to the next origin; ambiguous writes stop with an unknown
/// result so the app never duplicates a mutation.
pub async fn edge(
    method: &str,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> Result<Value, String> {
    crate::network::ensure_startup_probe().await?;
    let domains = crate::network::request_domains(true);
    edge_at(
        &API_CLIENT,
        &domains,
        method,
        path,
        body.as_ref(),
        token,
        |probe| crate::network::record_probe(probe, true),
    )
    .await
}

/// Used by background downloads, which need the current persisted session
/// without copying a token into a task or receipt.
pub async fn edge_current(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let token = crate::network::session_token();
    edge(method, path, body, token.as_deref()).await
}

/// 拉原始字节（图片 / m3u8 / 分片）。
pub async fn fetch_bytes(url: &str) -> Result<(Option<String>, Vec<u8>), String> {
    fetch_bytes_routed(url).await
}

pub async fn fetch_bytes_routed(raw_url: &str) -> Result<(Option<String>, Vec<u8>), String> {
    let site_url = crate::network::is_site_origin(raw_url);
    if site_url {
        crate::network::ensure_startup_probe().await?;
    }
    let candidates = crate::network::routed_urls(raw_url, true);
    let mut last = String::from("网络请求失败");
    // Media may legitimately redirect to a CDN. It never carries the API
    // bearer token, so it uses the redirect-following media client.
    let client = &*CLIENT;
    for (origin, url) in candidates {
        let referer = crate::network::referer_for(&origin);
        let mut attempts = if site_url { 1 } else { 3 };
        while attempts > 0 {
            attempts -= 1;
            let request = client.get(&url).header("Referer", &referer);
            match request.send().await {
                Ok(response) if response.status().is_success() => {
                    let ctype = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_owned);
                    let bytes = match response.bytes().await {
                        Ok(bytes) => bytes.to_vec(),
                        Err(error) => {
                            last = err_text(&error);
                            continue;
                        }
                    };
                    if site_url {
                        crate::network::record_probe(
                            crate::network::DomainProbe {
                                domain: origin.clone(),
                                ok: true,
                                status: Some(200),
                                elapsed_ms: 0,
                                error: None,
                            },
                            true,
                        );
                    }
                    return Ok((ctype, bytes));
                }
                Ok(response) => {
                    let status = response.status();
                    last = format!("HTTP {status}");
                    if status.is_client_error() {
                        return Err(last);
                    }
                }
                Err(error) => {
                    last = err_text(&error);
                    if site_url || attempts == 0 {
                        break;
                    }
                }
            }
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手动跑：`cargo test --lib net::tests::diag -- --nocapture`
    /// 关掉 TUN、只留系统代理时跑一遍，能直接看出客户端有没有走代理。
    #[tokio::test]
    #[ignore = "manual network diagnostic"]
    async fn diag() {
        println!(
            "{}",
            serde_json::to_string_pretty(&diagnose().await).expect("json")
        );
    }
}

#[cfg(test)]
#[path = "net_tests.rs"]
mod failover_tests;
