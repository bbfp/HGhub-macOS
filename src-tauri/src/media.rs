//! 媒体层：封面解密 + HLS 代理。
//!
//! 站点封面是 AES-128-CBC 密文，m3u8 内部分片/key 又可能被 WebView 的 CORS 拦住，
//! 所以图片与视频统一走本地自定义协议 `hg://localhost/...`。

use crate::net::fetch_bytes;
use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes128;
use once_cell::sync::Lazy;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Weak;
use std::sync::{Arc, Mutex};
use tauri::http::{Response, StatusCode};
use tauri::AppHandle;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const IMG_KEY: &[u8] = b"f5d965df75336270";
const IMG_IV: &[u8] = b"97b60394abc2fbe1";

static IMG_CACHE: Lazy<Mutex<HashMap<String, Arc<Vec<u8>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static PL_CACHE: Lazy<Mutex<HashMap<String, Arc<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static IMG_DIR: Lazy<Mutex<Option<PathBuf>>> = Lazy::new(|| Mutex::new(None));
type ImageGate = tokio::sync::Mutex<()>;
static IMG_INFLIGHT: Lazy<Mutex<HashMap<String, Weak<ImageGate>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn bind_cache(dir: PathBuf) {
    if std::fs::create_dir_all(&dir).is_ok() {
        *IMG_DIR.lock().unwrap() = Some(dir);
    }
}

fn image_cache_path(url: &str) -> Option<PathBuf> {
    IMG_DIR.lock().ok()?.as_ref().map(|dir| {
        dir.join(format!(
            "{:x}.img",
            Sha256::digest(crate::downloads::resource_id(url).as_bytes())
        ))
    })
}

const IMG_CACHE_MAX: usize = 240;
const PL_CACHE_MAX: usize = 60;

fn cache_put<V: Clone>(map: &Mutex<HashMap<String, V>>, key: String, val: V, max: usize) {
    let mut m = map.lock().unwrap();
    if m.len() >= max {
        if let Some(k) = m.keys().next().cloned() {
            m.remove(&k);
        }
    }
    m.insert(key, val);
}

/* ───────────────────────── AES ───────────────────────── */

fn aes_cbc_decrypt(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return None;
    }
    let cipher = Aes128::new_from_slice(IMG_KEY).ok()?;
    let mut prev = [0u8; 16];
    prev.copy_from_slice(IMG_IV);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut ct = [0u8; 16];
        ct.copy_from_slice(chunk);
        let mut block = ct;
        cipher.decrypt_block((&mut block).into());
        for i in 0..16 {
            out.push(block[i] ^ prev[i]);
        }
        prev = ct;
    }
    Some(out)
}

fn strip_padding(mut buf: Vec<u8>) -> Vec<u8> {
    let pad = *buf.last().unwrap_or(&0) as usize;
    if pad > 0
        && pad <= 16
        && buf.len() >= pad
        && buf[buf.len() - pad..].iter().all(|b| *b as usize == pad)
    {
        buf.truncate(buf.len() - pad);
    }
    buf
}

fn image_kind(buf: &[u8]) -> Option<&'static str> {
    if buf.len() < 12 {
        return None;
    }
    if buf[0] == 0xFF && buf[1] == 0xD8 {
        return Some("image/jpeg");
    }
    if buf.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Some("image/png");
    }
    if buf.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    if buf.starts_with(b"RIFF") && &buf[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// 取封面：命中缓存直接返回，否则下载并按需解密。
pub async fn load_image(url: &str) -> Result<(String, Arc<Vec<u8>>), String> {
    // docs/API.md §8.5 documents expiring auth_key on image URLs too. Keep
    // every other parameter so different image variants remain distinct.
    let identity = crate::downloads::resource_id(url);
    if let Some(hit) = IMG_CACHE.lock().unwrap().get(&identity).cloned() {
        let kind = image_kind(&hit).unwrap_or("image/jpeg");
        return Ok((kind.to_string(), hit));
    }
    let gate = {
        let mut inflight = IMG_INFLIGHT.lock().unwrap();
        inflight.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = inflight.get(&identity).and_then(Weak::upgrade) {
            gate
        } else {
            let gate = Arc::new(ImageGate::new(()));
            inflight.insert(identity.clone(), Arc::downgrade(&gate));
            gate
        }
    };
    let _guard = gate.lock().await;
    if let Some(hit) = IMG_CACHE.lock().unwrap().get(&identity).cloned() {
        let kind = image_kind(&hit).ok_or("封面格式无效")?;
        return Ok((kind.to_string(), hit));
    }
    if let Some(path) = image_cache_path(url) {
        if let Ok(bytes) = tokio::fs::read(path).await {
            if let Some(kind) = image_kind(&bytes) {
                let kind = kind.to_string();
                let arc = Arc::new(bytes);
                cache_put(&IMG_CACHE, identity.clone(), arc.clone(), IMG_CACHE_MAX);
                return Ok((kind, arc));
            }
        }
    }
    let (_, bytes) = fetch_bytes(url).await?;
    let plain = if image_kind(&bytes).is_some() {
        bytes
    } else {
        let dec = aes_cbc_decrypt(&bytes).ok_or_else(|| "封面解密失败".to_string())?;
        strip_padding(dec)
    };
    let kind = image_kind(&plain)
        .ok_or("下载内容不是可读取的封面图片")?
        .to_string();
    if let Some(path) = image_cache_path(url) {
        if let Err(error) = crate::downloads::atomic_bytes(&path, &plain).await {
            eprintln!("[media] 封面缓存写入失败：{error}");
        }
    }
    let arc = Arc::new(plain);
    cache_put(&IMG_CACHE, identity.clone(), arc.clone(), IMG_CACHE_MAX);
    Ok((kind, arc))
}

/* ───────────────────────── HLS ───────────────────────── */

fn enc(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

fn dec(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().to_string()
}

fn absolutize(base: &str, target: &str) -> String {
    if target.starts_with("http://") || target.starts_with("https://") {
        return target.to_string();
    }
    if let Ok(b) = url::Url::parse(base) {
        if let Ok(j) = b.join(target) {
            return j.to_string();
        }
    }
    target.to_string()
}

fn rewrite_uri_attr(line: &str, prefix: &str) -> String {
    // #EXT-X-KEY:...URI="xxx" / #EXT-X-MAP:URI="xxx"
    if let Some(start) = line.find("URI=\"") {
        let rest = &line[start + 5..];
        if let Some(end) = rest.find('"') {
            let uri = &rest[..end];
            let new = format!("{prefix}/seg?u={}", enc(uri));
            return format!("{}{}{}", &line[..start + 5], new, &rest[end..]);
        }
    }
    line.to_string()
}

/// 取 m3u8 并把 key / 分片地址改写到本地协议。
pub async fn load_playlist(url: &str, prefix: &str) -> Result<Arc<String>, String> {
    if let Some(hit) = PL_CACHE.lock().unwrap().get(url).cloned() {
        return Ok(hit);
    }
    let (_, bytes) = fetch_bytes(url).await?;
    let text = String::from_utf8_lossy(&bytes).to_string();

    let mut out = String::with_capacity(text.len() + 512);
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            out.push('\n');
            continue;
        }
        if line.starts_with('#') {
            if line.starts_with("#EXT-X-KEY") || line.starts_with("#EXT-X-MAP") {
                // 先把相对 URI 绝对化再改写
                let absolute = absolutize_uri_in_attr(url, line);
                out.push_str(&rewrite_uri_attr(&absolute, prefix));
            } else {
                out.push_str(line);
            }
        } else {
            let absolute = absolutize(url, line);
            out.push_str(&format!("{prefix}/seg?u={}", enc(&absolute)));
        }
        out.push('\n');
    }

    let arc = Arc::new(out);
    cache_put(&PL_CACHE, url.to_string(), arc.clone(), PL_CACHE_MAX);
    Ok(arc)
}

fn absolutize_uri_in_attr(base: &str, line: &str) -> String {
    if let Some(start) = line.find("URI=\"") {
        let rest = &line[start + 5..];
        if let Some(end) = rest.find('"') {
            let uri = &rest[..end];
            let abs = absolutize(base, uri);
            return format!("{}{}{}", &line[..start + 5], abs, &rest[end..]);
        }
    }
    line.to_string()
}

/// 清空封面 / 播放列表内存缓存
pub(crate) fn clear_memory_caches() {
    if let Ok(mut g) = IMG_CACHE.lock() {
        g.clear();
    }
    if let Ok(mut g) = PL_CACHE.lock() {
        g.clear();
    }
}

pub fn clear_caches() {
    clear_memory_caches();
    if let Ok(dir) = IMG_DIR.lock() {
        if let Some(dir) = dir.as_ref() {
            let _ = std::fs::remove_dir_all(dir);
            let _ = std::fs::create_dir_all(dir);
        }
    }
}

/* ───────────────────────── 协议响应 ───────────────────────── */

fn respond(status: u16, ctype: &str, body: Vec<u8>, cache: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::OK))
        .header("Content-Type", ctype)
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", cache)
        .body(body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

/// 统一入口：`hg://localhost/img?u=` / `hls?u=` / `seg?u=`
const MAX_RANGE_BYTES: u64 = 8 * 1024 * 1024;

fn byte_range(range: &str, total: u64) -> Result<(u64, u64), ()> {
    if total == 0 {
        return Err(());
    }
    let (first, last) = range
        .strip_prefix("bytes=")
        .and_then(|raw| raw.split_once('-'))
        .ok_or(())?;
    let (start, end) = if first.is_empty() {
        let length = last.parse::<u64>().map_err(|_| ())?;
        if length == 0 {
            return Err(());
        }
        (total.saturating_sub(length), total - 1)
    } else {
        let start = first.parse::<u64>().map_err(|_| ())?;
        let end = if last.is_empty() {
            total - 1
        } else {
            last.parse::<u64>().map_err(|_| ())?.min(total - 1)
        };
        (start, end)
    };
    if start >= total || start > end {
        return Err(());
    }
    Ok((start, end.min(start.saturating_add(MAX_RANGE_BYTES - 1))))
}

async fn file_response(path: &std::path::Path, range: Option<&str>) -> Response<Vec<u8>> {
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return respond(404, "text/plain", b"file not found".to_vec(), "no-store");
    };
    let Ok(metadata) = file.metadata().await else {
        return respond(404, "text/plain", b"file not found".to_vec(), "no-store");
    };
    if !metadata.is_file() {
        return respond(404, "text/plain", b"file not found".to_vec(), "no-store");
    }
    let total = metadata.len();
    let (start, length, status) = match range {
        Some(raw) => match byte_range(raw, total) {
            Ok((start, end)) => (start, end - start + 1, 206),
            Err(()) => {
                return Response::builder()
                    .status(416)
                    .header("Content-Range", format!("bytes */{total}"))
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Vec::new())
                    .unwrap()
            }
        },
        None => (0, total, 200),
    };
    if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return respond(500, "text/plain", b"file seek failed".to_vec(), "no-store");
    }
    let mut body = Vec::new();
    if file.take(length).read_to_end(&mut body).await.is_err() {
        return respond(500, "text/plain", b"file read failed".to_vec(), "no-store");
    }
    let mut builder = Response::builder()
        .status(status)
        .header("Content-Type", "video/mp4")
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .header("Content-Length", body.len().to_string());
    if status == 206 {
        builder = builder.header(
            "Content-Range",
            format!("bytes {start}-{}/{total}", start + length - 1),
        );
    }
    builder
        .body(body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

async fn local_file(app: &AppHandle, target: &str, range: Option<&str>) -> Response<Vec<u8>> {
    let Ok(file) = tokio::fs::canonicalize(target).await else {
        return respond(404, "text/plain", b"file not found".to_vec(), "no-store");
    };
    let Ok(tasks) = crate::downloads::load_tasks(app).await else {
        return respond(
            503,
            "text/plain",
            b"download records unavailable".to_vec(),
            "no-store",
        );
    };
    let mut allowed = false;
    for task in tasks {
        let expected = crate::downloads::episode_path_for_read(
            std::path::Path::new(&task.root_dir),
            task.drama_id,
            &task.drama_title,
            task.episode,
        );
        if tokio::fs::canonicalize(expected)
            .await
            .is_ok_and(|expected| expected == file)
        {
            allowed = true;
            break;
        }
    }
    if !allowed {
        return respond(
            403,
            "text/plain",
            b"file is not a registered episode".to_vec(),
            "no-store",
        );
    }
    file_response(&file, range).await
}

pub async fn handle(
    app: Option<&AppHandle>,
    path: &str,
    query: &str,
    range: Option<&str>,
    scheme: &str,
) -> Response<Vec<u8>> {
    let target = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("u="))
        .map(dec)
        .unwrap_or_default();

    if target.is_empty() {
        return respond(400, "text/plain", b"missing u".to_vec(), "no-store");
    }

    if path == "/file" {
        return match app {
            Some(app) => local_file(app, &target, range).await,
            None => respond(
                500,
                "text/plain",
                b"app handle unavailable".to_vec(),
                "no-store",
            ),
        };
    }

    let prefix = if scheme == "hg" {
        "hg://localhost".to_string()
    } else {
        format!("{scheme}://hg.localhost")
    };

    match path {
        "/img" => match load_image(&target).await {
            Ok((kind, bytes)) => {
                respond(200, &kind, bytes.as_ref().clone(), "public, max-age=86400")
            }
            Err(e) => respond(502, "text/plain", e.into_bytes(), "no-store"),
        },
        "/hls" => match load_playlist(&target, &prefix).await {
            Ok(text) => respond(
                200,
                "application/vnd.apple.mpegurl",
                text.as_bytes().to_vec(),
                "no-store",
            ),
            Err(e) => respond(502, "text/plain", e.into_bytes(), "no-store"),
        },
        "/seg" => match fetch_bytes(&target).await {
            Ok((ctype, bytes)) => respond(
                200,
                ctype.as_deref().unwrap_or("application/octet-stream"),
                bytes,
                "public, max-age=3600",
            ),
            Err(e) => respond(502, "text/plain", e.into_bytes(), "no-store"),
        },
        _ => respond(404, "text/plain", b"not found".to_vec(), "no-store"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ranges_read_only_requested_bytes_and_report_invalid_ranges() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.local/tests/range");
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let path = directory.join("large.mp4");
        let file = tokio::fs::File::create(&path).await.unwrap();
        file.set_len(20 * 1024 * 1024).await.unwrap();
        let response = file_response(&path, Some("bytes=100-")).await;
        assert_eq!(response.status(), 206);
        assert_eq!(response.body().len(), MAX_RANGE_BYTES as usize);
        assert_eq!(
            response.headers()["content-range"],
            "bytes 100-8388707/20971520"
        );
        let response = file_response(&path, Some("bytes=-100")).await;
        assert_eq!(response.body().len(), 100);
        assert_eq!(
            response.headers()["content-range"],
            "bytes 20971420-20971519/20971520"
        );
        for range in [
            "bytes=3-1",
            "bytes=-0",
            "bytes=x-",
            "bytes=2-x",
            "bytes=0-2,4-6",
            "bytes=20971520-",
            "nonsense",
        ] {
            let response = file_response(&path, Some(range)).await;
            assert_eq!(response.status(), 416, "invalid range accepted: {range}");
            assert_eq!(response.headers()["content-range"], "bytes */20971520");
        }
    }

    #[tokio::test]
    #[ignore = "requires reachable upstream source site"]
    async fn live_play_chain() {
        use crate::net::edge;
        let play = edge("GET", "/api/videos/117/play?ep=3&page_texts=0", None, None)
            .await
            .expect("play api");
        let url = play["video_url"].as_str().expect("video_url").to_string();
        println!("play url: {}", &url[..url.len().min(90)]);
        let (_, bytes) = fetch_bytes(&url).await.expect("fetch playlist");
        let text = String::from_utf8_lossy(&bytes);
        println!(
            "raw playlist len={} head={}",
            text.len(),
            &text[..text.len().min(160)]
        );
        let q = format!("u={}", enc(&url));
        let resp = handle(None, "/hls", &q, None, "http").await;
        let body = String::from_utf8_lossy(resp.body()).to_string();
        println!("proxied status={} len={}", resp.status(), body.len());
        assert!(body.contains("#EXTM3U"));
        // 取一个分片验证
        let seg_line = body
            .lines()
            .find(|l| !l.starts_with('#') && l.contains("/seg?u="))
            .expect("seg line")
            .to_string();
        println!("seg line: {}", &seg_line[..seg_line.len().min(120)]);
        let seg_q = seg_line.split("?u=").nth(1).unwrap();
        let seg = handle(None, "/seg", &format!("u={seg_q}"), None, "http").await;
        println!("segment status={} len={}", seg.status(), seg.body().len());
        assert_eq!(seg.status(), 200);
        assert!(seg.body().len() > 1000);
    }

    #[tokio::test]
    #[ignore = "requires reachable upstream source site"]
    async fn playlist_rewrite_works() {
        let m3u8 = "https://yd-lphu-hls.hzjvlo.cn/videos5/247f3c9862e8e1537ade0b20c45ac962/247f3c9862e8e1537ade0b20c45ac962.m3u8?auth_key=1793700503-0-0-b9575bb11258cf40cd5499c052fc5820&v=3&time=0&via=hgdj&via_bm=dx";
        let q = format!("u={}", enc(m3u8));
        let resp = handle(None, "/hls", &q, None, "http").await;
        let body = String::from_utf8_lossy(resp.body()).to_string();
        println!("status={} len={}", resp.status(), body.len());
        println!("--- head ---\n{}", &body[..body.len().min(600)]);
        assert!(body.contains("#EXTM3U"), "not a playlist: {body}");
        assert!(
            body.contains("hg.localhost/seg?u="),
            "segments not rewritten"
        );
    }

    #[tokio::test]
    #[ignore = "requires reachable upstream source site"]
    async fn cover_decrypt_works() {
        let url = "https://pic.drleao.cn/upload_01/upload/20261029/2026102918545028336.jpg?auth_key=1793707703-0-0-1cbd9389b0b69141dd741f16bb71397a";
        match load_image(url).await {
            Ok((kind, bytes)) => {
                println!("kind={kind} len={}", bytes.len());
                assert!(kind.starts_with("image/"));
            }
            Err(e) => panic!("image failed: {e}"),
        }
    }
}
