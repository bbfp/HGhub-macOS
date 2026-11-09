//! Editable API origins. Requests use the startup-ranked origin order; safe
//! read failures may fail over while ambiguous mutations are never resubmitted.
//! Media URLs are independent.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tauri::Manager;
use tokio::sync::Mutex as AsyncMutex;
use url::{Host, Url};

pub const SCHEMA_VERSION: u32 = 1;
pub const PRIMARY_DOMAIN: &str = "https://huangguoai.com";
pub const DEFAULT_MIRRORS: &[&str] = &[
    "https://olfs.gitiecgad.cc",
    "https://y6wb.gitiecgad.cc",
    "https://omqefh.gitiecgad.cc",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkSettings {
    pub schema_version: u32,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSettingsView {
    pub schema_version: u32,
    pub domains: Vec<String>,
    pub active_domain: String,
    pub warning: Option<String>,
    pub probes: Vec<DomainProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainProbe {
    pub domain: String,
    pub ok: bool,
    pub status: Option<u16>,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTestResult {
    pub probes: Vec<DomainProbe>,
    pub first_working: Option<String>,
}

#[derive(Debug, Clone)]
struct NetworkState {
    settings: NetworkSettings,
    active_domain: Option<String>,
    ranked_domains: Option<Vec<String>>,
    warning: Option<String>,
    probes: Vec<DomainProbe>,
}

impl NetworkState {
    fn new(settings: NetworkSettings, warning: Option<String>) -> Self {
        Self {
            settings,
            active_domain: None,
            ranked_domains: None,
            warning,
            probes: Vec::new(),
        }
    }
}

static STATE: Lazy<RwLock<NetworkState>> =
    Lazy::new(|| RwLock::new(NetworkState::new(defaults(), None)));
static STARTUP_PROBE_LOCK: Lazy<AsyncMutex<()>> = Lazy::new(|| AsyncMutex::new(()));
type TokenProvider = Arc<dyn Fn() -> Option<String> + Send + Sync>;
static SESSION_TOKEN: Lazy<RwLock<Option<TokenProvider>>> = Lazy::new(|| RwLock::new(None));

pub fn bind_session_token(app: &tauri::AppHandle) {
    let handle = app.clone();
    let provider: TokenProvider =
        Arc::new(move || handle.state::<crate::api::Api>().session.token());
    *SESSION_TOKEN.write().expect("session token lock") = Some(provider);
}

pub fn session_token() -> Option<String> {
    SESSION_TOKEN
        .read()
        .ok()?
        .as_ref()
        .and_then(|provider| provider())
}

fn defaults() -> NetworkSettings {
    NetworkSettings {
        schema_version: SCHEMA_VERSION,
        domains: std::iter::once(PRIMARY_DOMAIN.to_string())
            .chain(DEFAULT_MIRRORS.iter().map(|domain| (*domain).to_string()))
            .collect(),
    }
}

pub fn normalize_domain(raw: &str) -> Result<String, String> {
    let input = raw.trim();
    if input.is_empty() {
        return Err("域名不能为空".into());
    }
    let candidate = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&candidate).map_err(|_| "域名格式无效".to_string())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err("只支持不带路径、参数或账号信息的 HTTPS 域名".into());
    }
    let Some(Host::Domain(host)) = url.host() else {
        return Err("请填写公网域名，不支持 IP 地址".into());
    };
    let host = host.to_ascii_lowercase();
    if !host.contains('.')
        || host.ends_with('.')
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err("请填写有效公网域名，不支持 localhost 或本地域名".into());
    }
    if url.port().is_some_and(|port| port != 443) {
        return Err("只支持 HTTPS 默认端口".into());
    }
    Ok(format!("https://{host}"))
}

pub fn normalize_domains(raw: &[String]) -> Result<Vec<String>, String> {
    let mut domains = Vec::new();
    for item in raw {
        let domain = normalize_domain(item)?;
        if !domains.contains(&domain) {
            domains.push(domain);
        }
    }
    if domains.is_empty() {
        return Err("至少保留一个 API 域名".into());
    }
    Ok(domains)
}

fn network_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(crate::data_dir(app)?.join("network.json"))
}

fn write_settings_file(path: &Path, settings: &NetworkSettings) -> Result<(), String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if value
                    .get("schema_version")
                    .and_then(serde_json::Value::as_u64)
                    != Some(SCHEMA_VERSION as u64)
                {
                    return Err(
                        "此域名配置来自其他版本，当前版本无法覆盖，请使用对应版本修改。".into(),
                    );
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法检查已有 API 域名配置：{error}")),
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("json.tmp-{}", crate::downloads::unique_stamp()));
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(&data).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn load_settings_file(path: &Path) -> NetworkState {
    let (settings, mut warning, save_defaults) = match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(value)
                if value
                    .get("schema_version")
                    .and_then(serde_json::Value::as_u64)
                    == Some(SCHEMA_VERSION as u64) =>
            {
                match serde_json::from_value::<NetworkSettings>(value)
                    .map_err(|error| error.to_string())
                    .and_then(|settings| normalize_domains(&settings.domains))
                {
                    Ok(domains) => (
                        NetworkSettings {
                            schema_version: SCHEMA_VERSION,
                            domains,
                        },
                        None,
                        false,
                    ),
                    Err(error) => (
                        defaults(),
                        Some(format!(
                            "API 域名设置无效，已保留原文件并使用默认列表：{error}"
                        )),
                        false,
                    ),
                }
            }
            Ok(_) => (
                defaults(),
                Some("API 域名设置来自其他版本，已保留原文件并使用默认列表。".into()),
                false,
            ),
            Err(error) => {
                let backup = path.with_file_name(format!(
                    "network.json.corrupt-{}",
                    crate::downloads::unique_stamp()
                ));
                match std::fs::rename(path, &backup) {
                    Ok(()) => (
                        defaults(),
                        Some(format!(
                            "API 域名设置损坏，原文件已保留在 {}：{error}",
                            backup.display()
                        )),
                        true,
                    ),
                    Err(rename_error) => (
                        defaults(),
                        Some(format!(
                            "API 域名设置损坏，无法备份，已保留原文件：{error}；{rename_error}"
                        )),
                        false,
                    ),
                }
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (defaults(), None, true),
        Err(error) => (
            defaults(),
            Some(format!("无法读取 API 域名设置，已使用默认列表：{error}")),
            false,
        ),
    };
    if save_defaults {
        if let Err(error) = write_settings_file(path, &settings) {
            let message = format!("无法保存 API 域名设置：{error}");
            warning = Some(warning.map_or(message.clone(), |old| format!("{old}；{message}")));
        }
    }
    NetworkState::new(settings, warning)
}

pub fn initialize(app: &tauri::AppHandle) {
    let state = match network_path(app) {
        Ok(path) => load_settings_file(&path),
        Err(error) => NetworkState::new(
            defaults(),
            Some(format!("无法加载域名配置，已使用默认列表：{error}")),
        ),
    };
    *STATE.write().expect("network state lock") = state;
}

fn ordered_domains(domains: &[String], active: Option<&str>, allow_fallback: bool) -> Vec<String> {
    if !allow_fallback {
        return domains.iter().take(1).cloned().collect();
    }
    let mut ordered = domains.to_vec();
    if let Some(index) =
        active.and_then(|active| ordered.iter().position(|domain| domain == active))
    {
        let active = ordered.remove(index);
        ordered.insert(0, active);
    }
    ordered
}

pub fn request_domains(allow_fallback: bool) -> Vec<String> {
    let state = STATE.read().expect("network state lock");
    let domains = state
        .ranked_domains
        .as_deref()
        .unwrap_or(&state.settings.domains);
    platform_domains(
        cfg!(target_os = "macos"),
        domains,
        state.active_domain.as_deref(),
        allow_fallback,
    )
}

fn platform_domains(
    is_macos: bool,
    domains: &[String],
    active: Option<&str>,
    _allow_fallback: bool,
) -> Vec<String> {
    if is_macos {
        ordered_domains(domains, active, true)
    } else {
        vec![PRIMARY_DOMAIN.to_string()]
    }
}

/// A site origin is the primary site or one of the user-configured aliases.
/// CDN hosts returned by the API are deliberately excluded.
pub fn is_site_origin(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https" || url.username() != "" || url.password().is_some() {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let primary = Url::parse(PRIMARY_DOMAIN).expect("primary domain");
    if host == primary.host_str().unwrap_or_default() {
        return url.port().is_none_or(|port| port == 443);
    }
    let state = STATE.read().expect("network state lock");
    state.settings.domains.iter().any(|domain| {
        Url::parse(domain).ok().and_then(|origin| {
            origin
                .host_str()
                .map(|value| value.eq_ignore_ascii_case(&host))
        }) == Some(true)
            && url.port().is_none_or(|port| port == 443)
    })
}

/// Replace only the parsed origin. Path, query, and fragment are preserved;
/// hostnames appearing inside a query are never touched.
pub fn replace_site_origin(raw: &str, target_origin: &str) -> Option<String> {
    if !is_site_origin(raw) {
        return None;
    }
    let source = Url::parse(raw).ok()?;
    let _target = Url::parse(target_origin).ok()?;
    let suffix = source[url::Position::BeforePath..].to_string();
    Some(format!("{}{}", target_origin.trim_end_matches('/'), suffix))
}

/// Return site-origin candidates in active-first order, or the original URL
/// when it is an independent media/CDN URL.
pub fn routed_urls(raw: &str, allow_fallback: bool) -> Vec<(String, String)> {
    if !is_site_origin(raw) {
        return vec![(current_domain(), raw.to_string())];
    }
    let domains = request_domains(allow_fallback);
    domains
        .into_iter()
        .filter_map(|domain| replace_site_origin(raw, &domain).map(|url| (domain, url)))
        .collect()
}

pub fn referer_for(origin: &str) -> String {
    format!("{}/", origin.trim_end_matches('/'))
}

pub fn current_domain() -> String {
    request_domains(true)
        .into_iter()
        .next()
        .unwrap_or_else(|| PRIMARY_DOMAIN.to_string())
}

pub fn settings_view() -> NetworkSettingsView {
    let state = STATE.read().expect("network state lock");
    NetworkSettingsView {
        schema_version: state.settings.schema_version,
        active_domain: state
            .ranked_domains
            .as_ref()
            .and_then(|domains| domains.first().cloned())
            .or_else(|| state.active_domain.clone())
            .or_else(|| state.settings.domains.first().cloned())
            .unwrap_or_else(|| PRIMARY_DOMAIN.to_string()),
        domains: state.settings.domains.clone(),
        warning: state.warning.clone(),
        probes: state.probes.clone(),
    }
}

pub fn record_probe(probe: DomainProbe, prefer_success: bool) {
    let mut state = STATE.write().expect("network state lock");
    if !state.settings.domains.contains(&probe.domain) {
        return;
    }
    if probe.ok && prefer_success && state.ranked_domains.is_none() {
        state.active_domain = Some(probe.domain.clone());
    }
    state.probes.retain(|old| old.domain != probe.domain);
    state.probes.push(probe);
}

pub fn set_domains(
    app: &tauri::AppHandle,
    raw: Vec<String>,
) -> Result<NetworkSettingsView, String> {
    let settings = NetworkSettings {
        schema_version: SCHEMA_VERSION,
        domains: normalize_domains(&raw)?,
    };
    let mut state = STATE
        .write()
        .map_err(|_| "API 域名状态锁失败".to_string())?;
    write_settings_file(&network_path(app)?, &settings)?;
    *state = NetworkState::new(settings, None);
    drop(state);
    Ok(settings_view())
}

pub(crate) fn rank_probes(configured: &[String], mut probes: Vec<DomainProbe>) -> Vec<DomainProbe> {
    probes.sort_by(|left, right| {
        right
            .ok
            .cmp(&left.ok)
            .then_with(|| left.elapsed_ms.cmp(&right.elapsed_ms))
            .then_with(|| {
                let left_index = configured
                    .iter()
                    .position(|domain| domain == &left.domain)
                    .unwrap_or(usize::MAX);
                let right_index = configured
                    .iter()
                    .position(|domain| domain == &right.domain)
                    .unwrap_or(usize::MAX);
                left_index.cmp(&right_index)
            })
    });
    probes
}

async fn probe_one(domain: String) -> DomainProbe {
    let started = Instant::now();
    let result = crate::net::edge_once(
        &crate::net::API_CLIENT,
        &domain,
        "GET",
        "/api/videos?page=1&size=1",
        None,
        None,
    )
    .await;
    // HTTP 200 with HTML or a failing API envelope is not a working mirror.
    match result {
        Ok(reply)
            if reply
                .data
                .get("items")
                .is_some_and(|items| items.is_array()) =>
        {
            DomainProbe {
                domain,
                ok: true,
                status: Some(reply.status),
                elapsed_ms: started.elapsed().as_millis() as u64,
                error: None,
            }
        }
        Ok(reply) => DomainProbe {
            domain,
            ok: false,
            status: Some(reply.status),
            elapsed_ms: started.elapsed().as_millis() as u64,
            error: Some("接口未返回视频列表".into()),
        },
        Err(error) => DomainProbe {
            domain,
            ok: false,
            status: error.status,
            elapsed_ms: started.elapsed().as_millis() as u64,
            error: Some(error.message),
        },
    }
}

pub async fn probe_domains(raw: Vec<String>) -> Result<NetworkTestResult, String> {
    let domains = normalize_domains(&raw)?;
    let mut jobs = tokio::task::JoinSet::new();
    for domain in domains.iter().cloned() {
        jobs.spawn(probe_one(domain));
    }
    let mut probes = Vec::with_capacity(domains.len());
    while let Some(result) = jobs.join_next().await {
        if let Ok(probe) = result {
            probes.push(probe);
        }
    }
    let probes = rank_probes(&domains, probes);
    let first_working = probes
        .iter()
        .find(|probe| probe.ok)
        .map(|probe| probe.domain.clone());
    Ok(NetworkTestResult {
        probes,
        first_working,
    })
}

fn apply_startup_probe(domains: &[String], probes: Vec<DomainProbe>) {
    let ranked = rank_probes(domains, probes);
    let ordered = ranked
        .iter()
        .map(|probe| probe.domain.clone())
        .collect::<Vec<_>>();
    let active = ordered.first().cloned();
    let mut state = STATE.write().expect("network state lock");
    if state.settings.domains == domains {
        state.ranked_domains = Some(ordered);
        state.active_domain = active;
        state.probes = ranked;
    }
}

pub async fn ensure_startup_probe() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let domains = {
        let state = STATE.read().expect("network state lock");
        if state.ranked_domains.is_some() {
            return Ok(());
        }
        state.settings.domains.clone()
    };
    let _guard = STARTUP_PROBE_LOCK.lock().await;
    {
        let state = STATE.read().expect("network state lock");
        if state.ranked_domains.is_some() || state.settings.domains != domains {
            return Ok(());
        }
    }
    let result = probe_domains(domains.clone()).await?;
    apply_startup_probe(&domains, result.probes);
    Ok(())
}

#[tauri::command]
pub async fn network_settings() -> Result<NetworkSettingsView, String> {
    Ok(settings_view())
}

#[tauri::command]
pub async fn network_settings_set(
    app: tauri::AppHandle,
    domains: Vec<String>,
) -> Result<NetworkSettingsView, String> {
    set_domains(&app, domains)
}

#[tauri::command]
pub async fn network_settings_test(domains: Vec<String>) -> Result<NetworkTestResult, String> {
    probe_domains(domains).await
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
