use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes128;
use m3u8_rs::{Key, KeyMethod, Playlist};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use tokio::{io::AsyncReadExt, sync::Mutex};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Encryption {
    pub url: String,
    pub iv: [u8; 16],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    pub url: String,
    pub sequence: u64,
    pub duration: f32,
    pub encryption: Option<Encryption>,
}

#[derive(Clone, Debug)]
pub struct MediaPlan {
    pub target_duration: u64,
    pub media_sequence: u64,
    pub segments: Vec<Segment>,
    pub init: Option<Segment>,
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn resource_id(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return hash(raw.as_bytes());
    };
    // The source's API documentation identifies auth_key as an expiring signature.
    // Keep all content selection parameters; their meaning is not interchangeable.
    let pairs: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key != "auth_key")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    hash(url.as_str().as_bytes())
}

fn absolute(base: &str, uri: &str) -> Result<String, String> {
    let url = Url::parse(base)
        .and_then(|base| base.join(uri))
        .map_err(|_| "HLS 地址无效")?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err("HLS 资源必须使用 HTTP(S)".into());
    }
    Ok(url.to_string())
}

fn iv(value: Option<&str>, sequence: u64) -> Result<[u8; 16], String> {
    let Some(raw) = value else {
        return Ok((sequence as u128).to_be_bytes());
    };
    let raw = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(raw);
    if raw.len() > 32 || raw.is_empty() || !raw.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("HLS IV 格式无效".into());
    }
    u128::from_str_radix(raw, 16)
        .map(u128::to_be_bytes)
        .map_err(|_| "HLS IV 格式无效".into())
}

fn encryption(key: Option<&Key>, base: &str, sequence: u64) -> Result<Option<Encryption>, String> {
    let Some(key) = key else {
        return Ok(None);
    };
    if key.method == KeyMethod::None {
        return Ok(None);
    }
    if key.method != KeyMethod::AES128 || key.keyformat.as_deref().is_some_and(|s| s != "identity")
    {
        return Err("暂不支持此视频加密方式".into());
    }
    Ok(Some(Encryption {
        url: absolute(base, key.uri.as_deref().ok_or("HLS 缺少密钥地址")?)?,
        iv: iv(key.iv.as_deref(), sequence)?,
    }))
}

pub fn parse_media(bytes: &[u8], base: &str) -> Result<MediaPlan, String> {
    let playlist = m3u8_rs::parse_playlist_res(bytes).map_err(|_| "无法读取 HLS 清单")?;
    let Playlist::MediaPlaylist(media) = playlist else {
        return Err("需要先选择 HLS 视频清晰度".into());
    };
    if !media.end_list || media.segments.is_empty() {
        return Err("暂不支持直播或空视频清单".into());
    }
    let mut current_key: Option<Key> = None;
    let mut init: Option<Segment> = None;
    let mut segments = Vec::new();
    // m3u8-rs 6.0.1 currently keeps `METHOD=NONE` as an unknown tag instead
    // of exposing it as `MediaSegment.key`. Track that reset from the raw
    // playlist so a previous AES key can never bleed into later segments.
    let mut clear_before = Vec::new();
    let mut clear_next = false;
    for line in bytes.split(|b| *b == b'\n') {
        let line = String::from_utf8_lossy(line);
        let line = line.trim();
        if line.starts_with("#EXT-X-KEY:") && line.contains("METHOD=NONE") {
            clear_next = true;
        } else if !line.is_empty() && !line.starts_with('#') {
            clear_before.push(clear_next);
            clear_next = false;
        }
    }
    for (index, seg) in media.segments.iter().enumerate() {
        if clear_before.get(index).copied().unwrap_or(false) {
            current_key = None;
        }
        if seg.byte_range.is_some() || seg.discontinuity {
            return Err("该视频需要尚未支持的分段格式".into());
        }
        if !seg.duration.is_finite() || seg.duration <= 0.0 {
            return Err("HLS 分片时长无效".into());
        }
        if clear_before.get(index).copied().unwrap_or(false) {
            current_key = None;
        } else if let Some(key) = &seg.key {
            current_key = Some(key.clone());
        }
        let sequence = media
            .media_sequence
            .checked_add(index as u64)
            .ok_or("HLS 序号无效")?;
        let enc = encryption(current_key.as_ref(), base, sequence)?;
        if let Some(map) = &seg.map {
            if map.byte_range.is_some() {
                return Err("暂不支持分段初始化范围".into());
            }
            let url = absolute(base, &map.uri)?;
            if init
                .as_ref()
                .is_some_and(|old| resource_id(&old.url) != resource_id(&url))
            {
                return Err("暂不支持播放中更换初始化片段".into());
            }
            if init.is_none() {
                if enc.is_some() && current_key.as_ref().and_then(|k| k.iv.as_ref()).is_none() {
                    return Err("加密初始化片段缺少 IV".into());
                }
                init = Some(Segment {
                    url,
                    sequence,
                    duration: 0.0,
                    encryption: enc.clone(),
                });
            }
        }
        segments.push(Segment {
            url: absolute(base, &seg.uri)?,
            sequence,
            duration: seg.duration,
            encryption: enc,
        });
    }
    Ok(MediaPlan {
        target_duration: media.target_duration,
        media_sequence: media.media_sequence,
        segments,
        init,
    })
}

pub fn decrypt(bytes: &[u8], key: &[u8], iv: &[u8; 16]) -> Result<Vec<u8>, String> {
    if key.len() != 16 || bytes.is_empty() || !bytes.len().is_multiple_of(16) {
        return Err("视频密钥或分片长度无效".into());
    }
    let cipher = Aes128::new_from_slice(key).map_err(|_| "视频密钥长度无效")?;
    let mut prev = *iv;
    let mut out = Vec::with_capacity(bytes.len());
    for part in bytes.chunks_exact(16) {
        let mut block = [0u8; 16];
        block.copy_from_slice(part);
        cipher.decrypt_block((&mut block).into());
        out.extend(block.iter().zip(prev).map(|(a, b)| a ^ b));
        prev.copy_from_slice(part);
    }
    let n = *out.last().unwrap() as usize;
    if n == 0 || n > 16 || !out[out.len() - n..].iter().all(|&b| b as usize == n) {
        return Err("视频分片解密校验失败".into());
    }
    out.truncate(out.len() - n);
    Ok(out)
}

pub async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    match crate::net::fetch_bytes(url).await {
        Ok((_, bytes)) => Ok(bytes),
        Err(error) => Err(error),
    }
}

pub async fn fetch_plan(url: &str) -> Result<MediaPlan, String> {
    let mut url = url.to_string();
    for _ in 0..3 {
        let bytes = fetch(&url).await?;
        match m3u8_rs::parse_playlist_res(&bytes).map_err(|_| "无法读取 HLS 清单")? {
            Playlist::MediaPlaylist(_) => return parse_media(&bytes, &url),
            Playlist::MasterPlaylist(master) => {
                let variant = master
                    .variants
                    .iter()
                    .filter(|variant| !variant.is_i_frame)
                    .max_by_key(|variant| variant.bandwidth)
                    .ok_or("视频没有可用清晰度")?;
                if variant.audio.as_ref().is_some_and(|group| {
                    master
                        .alternatives
                        .iter()
                        .any(|alt| &alt.group_id == group && alt.uri.is_some())
                }) {
                    return Err("暂不支持单独音轨的 HLS 视频".into());
                }
                url = absolute(&url, &variant.uri)?;
            }
        }
    }
    Err("视频清单嵌套过多".into())
}

pub type KeyCache = Arc<Mutex<HashMap<String, Vec<u8>>>>;

pub async fn fetch_segment(segment: &Segment, keys: &KeyCache) -> Result<Vec<u8>, String> {
    let raw = fetch(&segment.url).await?;
    let Some(encryption) = &segment.encryption else {
        return Ok(raw);
    };
    let mut keys = keys.lock().await;
    let key = if let Some(key) = keys.get(&encryption.url) {
        key.clone()
    } else {
        let key = fetch(&encryption.url).await?;
        if key.len() != 16 {
            return Err("视频密钥长度无效".into());
        }
        keys.insert(encryption.url.clone(), key.clone());
        key
    };
    decrypt(&raw, &key, &encryption.iv)
}

pub fn expired(error: &str) -> bool {
    ["HTTP 401", "HTTP 403", "HTTP 404", "HTTP 410"]
        .iter()
        .any(|code| error.contains(code))
}

pub fn iv_string(iv: &[u8; 16]) -> String {
    iv.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn local_name(index: usize, original: &str) -> String {
    let parsed = Url::parse(original).ok();
    let raw_path = parsed.as_ref().map(|url| url.path()).unwrap_or(original);
    let ext = Path::new(raw_path)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| e.len() <= 6)
        .unwrap_or("ts");
    format!("{index:06}.{ext}")
}

pub async fn write_playlist(plan: &MediaPlan, dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join("index.m3u8");
    let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:3\n");
    text.push_str(&format!(
        "#EXT-X-TARGETDURATION:{}\n#EXT-X-MEDIA-SEQUENCE:{}\n",
        plan.target_duration.max(1),
        plan.media_sequence
    ));
    if let Some(init) = &plan.init {
        text.push_str(&format!(
            "#EXT-X-MAP:URI=\"{}\"\n",
            local_name(0, &init.url)
        ));
    }
    for (index, segment) in plan.segments.iter().enumerate() {
        text.push_str(&format!(
            "#EXTINF:{:.6},\n{}\n",
            segment.duration,
            local_name(index + 1, &segment.url)
        ));
    }
    text.push_str("#EXT-X-ENDLIST\n");
    super::storage::atomic_bytes(&path, text.as_bytes()).await?;
    Ok(path)
}

pub async fn remux(playlist: &Path, output: &Path) -> Result<PathBuf, String> {
    if output.exists() {
        return Err("已存在同一集的视频文件，未覆盖".into());
    }
    let ffmpeg = std::env::var("HGHUB_FFMPEG").unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.local/ffmpeg/runtime/bin/ffmpeg")
            .to_string_lossy()
            .into_owned()
    });
    let part = output.with_extension("mp4.part");
    let result = Command::new(ffmpeg)
        .kill_on_drop(true)
        .args([
            "-v",
            "error",
            "-nostdin",
            "-y",
            "-protocol_whitelist",
            "file",
            "-allowed_extensions",
            "ALL",
            "-i",
        ])
        .arg(playlist)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
        ])
        .arg(&part)
        .output()
        .await
        .map_err(|e| format!("启动 FFmpeg 失败: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "FFmpeg 合并失败: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let file = tokio::fs::File::open(&part)
        .await
        .map_err(|e| e.to_string())?;
    file.sync_all().await.map_err(|e| e.to_string())?;
    Ok(part)
}

pub async fn verify_file(path: &Path, record: &super::SegmentRecord) -> Result<bool, String> {
    let (length, sha) = file_hash(path).await?;
    Ok(length == record.byte_len && sha == record.sha256)
}

pub async fn file_hash(path: &Path) -> Result<(u64, String), String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| e.to_string())?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut digest = Sha256::new();
    let mut length = 0;
    loop {
        let count = file.read(&mut buffer).await.map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        length += count as u64;
    }
    Ok((length, format!("{:x}", digest.finalize())))
}
