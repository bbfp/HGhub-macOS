//! Versioned local downloads. Tauri commands stay thin; the manager owns all task mutations.
mod hls;
mod index;
pub mod library;
mod manager;
mod models;
mod storage;
pub(crate) use storage::unique_stamp;
#[cfg(test)]
mod tests;
mod worker;

pub(crate) use hls::resource_id;
pub use index::{DownloadMarker, RemoteCheck};
pub use manager::DownloadManager;
pub use models::*;
use std::path::PathBuf;
pub(crate) use storage::atomic_bytes;
use tauri::{AppHandle, Manager, State};

pub const SCHEMA_VERSION: u32 = 1;

fn default_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .video_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| e.to_string())?
        .join("HGhub"))
}

pub async fn load_settings(app: &AppHandle) -> Result<DownloadSettings, String> {
    app.state::<crate::api::Api>()
        .downloads
        .settings(default_root(app)?)
        .await
}

pub async fn load_tasks(app: &AppHandle) -> Result<Vec<DownloadTask>, String> {
    app.state::<crate::api::Api>().downloads.tasks().await
}

async fn all_library_items(
    tasks: &[DownloadTask],
    root: &std::path::Path,
) -> Result<Vec<library::LibraryDrama>, String> {
    let mut cursor = 0;
    let mut result = Vec::new();
    loop {
        let page = library::build_page(tasks, root, cursor, 50).await?;
        let count = page.items.len();
        result.extend(page.items);
        let Some(next) = page.next_cursor else { break };
        cursor = next.parse::<usize>().map_err(|_| "本地库游标无效")?;
        if count == 0 {
            break;
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn download_library_page(
    app: AppHandle,
    state: State<'_, crate::api::Api>,
    cursor: Option<String>,
    limit: Option<usize>,
) -> Result<library::LibraryPage, String> {
    if !cfg!(target_os = "macos") {
        return Err("本地库目前仅支持 macOS 版".into());
    }
    let cursor = cursor.unwrap_or_default().parse::<usize>().unwrap_or(0);
    let limit = limit.unwrap_or(20);
    let settings = load_settings(&app).await?;
    let tasks = state.downloads.tasks().await?;
    library::build_page(
        &tasks,
        std::path::Path::new(&settings.root_dir),
        cursor,
        limit,
    )
    .await
}

#[tauri::command]
pub async fn download_library_update_check(
    app: AppHandle,
    state: State<'_, crate::api::Api>,
    drama_ids: Option<Vec<i64>>,
) -> Result<Vec<library::LibraryUpdate>, String> {
    if !cfg!(target_os = "macos") {
        return Err("本地库目前仅支持 macOS 版".into());
    }
    let settings = load_settings(&app).await?;
    let tasks = state.downloads.tasks().await?;
    let items = all_library_items(&tasks, std::path::Path::new(&settings.root_dir)).await?;
    let wanted = drama_ids.map(|ids| ids.into_iter().collect::<std::collections::HashSet<_>>());
    let checked_at = storage::now();
    let mut result = Vec::new();
    let mut checks = Vec::new();
    for drama in items {
        if wanted
            .as_ref()
            .is_some_and(|ids| !ids.contains(&drama.drama_id))
        {
            continue;
        }
        let local = drama
            .episodes
            .iter()
            .map(|episode| episode.episode)
            .collect::<Vec<_>>();
        let checked =
            crate::net::edge_current("GET", &format!("/api/videos/{}", drama.drama_id), None).await;
        match checked {
            Ok(value) => {
                let remote = library::parse_remote_episodes(&value);
                let remote_episode_count = remote.len();
                checks.push(RemoteCheck {
                    drama_id: drama.drama_id,
                    title: value
                        .get("title")
                        .and_then(|title| title.as_str())
                        .unwrap_or(&drama.title)
                        .to_owned(),
                    keywords: value
                        .get("tags")
                        .and_then(|tags| tags.as_array())
                        .map(|tags| {
                            tags.iter()
                                .filter_map(|tag| tag.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                    cover_url: value
                        .get("cover")
                        .and_then(|cover| cover.as_str())
                        .map(str::to_owned)
                        .or_else(|| drama.cover_url.clone()),
                    checked_at: checked_at.clone(),
                    remote_episode_count: remote_episode_count as u32,
                });
                let new_numbers = library::new_episode_numbers(
                    &remote
                        .iter()
                        .map(|(episode, _)| *episode)
                        .collect::<Vec<_>>(),
                    &local,
                );
                let new_episodes = remote
                    .into_iter()
                    .filter(|(episode, _)| new_numbers.contains(episode))
                    .map(|(episode, title)| library::LibraryUpdateEpisode { episode, title })
                    .collect();
                result.push(library::LibraryUpdate {
                    drama_id: drama.drama_id,
                    title: drama.title,
                    status: "ok".into(),
                    remote_episode_count,
                    new_episodes,
                    cover_url: value
                        .get("cover")
                        .and_then(|cover| cover.as_str())
                        .map(str::to_owned)
                        .or(drama.cover_url),
                    checked_at: checked_at.clone(),
                    error: None,
                });
            }
            Err(error) => result.push(library::LibraryUpdate {
                drama_id: drama.drama_id,
                title: drama.title,
                status: "unknown".into(),
                remote_episode_count: 0,
                new_episodes: Vec::new(),
                cover_url: drama.cover_url,
                checked_at: checked_at.clone(),
                error: Some(error),
            }),
        }
    }
    state.downloads.record_remote_checks(checks).await?;
    Ok(result)
}

#[tauri::command]
pub async fn download_settings(app: AppHandle) -> Result<DownloadSettings, String> {
    load_settings(&app).await
}

#[tauri::command]
pub async fn download_settings_set(
    app: AppHandle,
    state: State<'_, crate::api::Api>,
    root_dir: String,
) -> Result<DownloadSettings, String> {
    let root = PathBuf::from(root_dir.trim());
    if !root.is_absolute() {
        return Err("请选择完整的下载目录".into());
    }
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|e| format!("无法创建下载目录：{e}"))?;
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(|e| e.to_string())?;
    state.downloads.set_root(default_root(&app)?, root).await
}

#[tauri::command]
pub async fn download_limits_set(
    app: AppHandle,
    state: State<'_, crate::api::Api>,
    max_tasks: usize,
    max_episodes_per_drama: usize,
    max_segments_per_episode: usize,
) -> Result<DownloadSettings, String> {
    state
        .downloads
        .set_limits(
            default_root(&app)?,
            DownloadLimits {
                max_tasks,
                max_episodes_per_drama,
                max_segments_per_episode,
            },
        )
        .await
}

#[tauri::command]
pub async fn download_local_path(
    state: State<'_, crate::api::Api>,
    id: String,
) -> Result<String, String> {
    let task = state
        .downloads
        .tasks()
        .await?
        .into_iter()
        .find(|task| task.id == id)
        .ok_or("下载任务不存在")?;
    Ok(episode_path_for_read(
        std::path::Path::new(&task.root_dir),
        task.drama_id,
        &task.drama_title,
        task.episode,
    )
    .to_string_lossy()
    .into_owned())
}

#[tauri::command]
pub async fn download_reveal(state: State<'_, crate::api::Api>, id: String) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("打开本地文件目前仅支持 macOS 版".into());
    }
    let task = state
        .downloads
        .tasks()
        .await?
        .into_iter()
        .find(|task| task.id == id)
        .ok_or("下载任务不存在")?;
    if task.status != DownloadStatus::Completed {
        return Err("只有已完成任务可以打开本地文件".into());
    }
    let output = episode_path_for_read(
        std::path::Path::new(&task.root_dir),
        task.drama_id,
        &task.drama_title,
        task.episode,
    );
    if !library::verified_file(&output).await {
        return Err("本地视频或校验回执不存在，无法安全打开".into());
    }
    std::process::Command::new("open")
        .arg("-R")
        .arg(&output)
        .spawn()
        .map_err(|error| format!("无法打开 Finder：{error}"))?;
    Ok(())
}

#[tauri::command]
pub async fn download_tasks(
    state: tauri::State<'_, crate::api::Api>,
) -> Result<Vec<DownloadTask>, String> {
    state.downloads.tasks().await
}

#[tauri::command]
pub async fn download_status(
    state: tauri::State<'_, crate::api::Api>,
    drama_ids: Vec<i64>,
) -> Result<Vec<DownloadMarker>, String> {
    if !cfg!(target_os = "macos") {
        return Ok(drama_ids
            .into_iter()
            .map(|drama_id| DownloadMarker {
                drama_id,
                downloaded: false,
                episodes: Vec::new(),
            })
            .collect());
    }
    state.downloads.library_status(drama_ids).await
}

#[tauri::command]
pub async fn download_warnings(
    state: tauri::State<'_, crate::api::Api>,
) -> Result<Vec<String>, String> {
    state.downloads.warnings().await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri injects state/app alongside the episode fields.
pub async fn download_enqueue(
    state: tauri::State<'_, crate::api::Api>,
    app: AppHandle,
    drama_id: i64,
    drama_title: String,
    episode: u32,
    episode_title: String,
    source_url: String,
    root_dir: Option<String>,
    cover_url: Option<String>,
) -> Result<DownloadTask, String> {
    if !cfg!(target_os = "macos") {
        return Err("本地下载目前仅支持 macOS 版".into());
    }
    if drama_id <= 0 || episode == 0 {
        return Err("短剧或集数无效".into());
    }
    let root = PathBuf::from(root_dir.unwrap_or(load_settings(&app).await?.root_dir));
    if !root.is_absolute() {
        return Err("下载目录必须是完整路径".into());
    }
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|e| format!("无法创建下载目录：{e}"))?;
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(|e| e.to_string())?;
    let root_dir = root.to_string_lossy().into_owned();
    let id = format!(
        "drama-{drama_id}-ep-{episode}-{}",
        &hls::hash(root_dir.as_bytes())[..12]
    );
    let stamp = storage::now();
    state
        .downloads
        .enqueue(DownloadTask {
            schema_version: SCHEMA_VERSION,
            id,
            drama_id,
            drama_title,
            episode,
            episode_title,
            source_url,
            root_dir,
            status: DownloadStatus::Queued,
            progress: 0.0,
            total_segments: 0,
            completed_segments: Vec::new(),
            error: None,
            created_at: stamp.clone(),
            updated_at: stamp,
            init_segment: None,
            cover_url,
        })
        .await
}

#[tauri::command]
pub async fn download_pause(
    state: tauri::State<'_, crate::api::Api>,
    id: String,
) -> Result<(), String> {
    state.downloads.pause(&id).await
}
#[tauri::command]
pub async fn download_resume(
    state: tauri::State<'_, crate::api::Api>,
    id: String,
) -> Result<(), String> {
    state.downloads.resume(&id).await
}
#[tauri::command]
pub async fn download_cancel(
    state: tauri::State<'_, crate::api::Api>,
    id: String,
) -> Result<(), String> {
    state.downloads.cancel(&id).await
}

#[tauri::command]
pub async fn download_delete(
    state: tauri::State<'_, crate::api::Api>,
    ids: Vec<String>,
) -> Result<usize, String> {
    if !cfg!(target_os = "macos") {
        return Err("删除本地下载记录目前仅支持 macOS 版".into());
    }
    state.downloads.delete(ids).await
}

#[cfg(test)]
mod hls_tests {
    use super::hls::*;

    #[test]
    fn sequence_iv_rotation_and_clear_segments() {
        let input = b"#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-MEDIA-SEQUENCE:12\n#EXT-X-KEY:METHOD=AES-128,URI=\"key-a\"\n#EXTINF:2,\na.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"key-b\",IV=0x000000000000000000000000000000ff\n#EXTINF:2,\nb.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:1,\nc.ts\n#EXT-X-ENDLIST\n";
        let plan = parse_media(input, "https://example.test/series/index.m3u8").unwrap();
        assert_eq!(plan.segments.len(), 3);
        assert_eq!(plan.segments[0].encryption.as_ref().unwrap().iv[15], 12);
        assert_eq!(plan.segments[1].encryption.as_ref().unwrap().iv[15], 255);
        assert!(plan.segments[2].encryption.is_none());
        assert_eq!(
            plan.segments[1].encryption.as_ref().unwrap().url,
            "https://example.test/series/key-b"
        );
    }

    #[test]
    fn rejects_drm_and_live_without_saving_partial_media() {
        let drm = b"#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"key\"\n#EXTINF:2,\na.ts\n#EXT-X-ENDLIST\n";
        assert!(parse_media(drm, "https://example.test/index.m3u8").is_err());
        assert!(parse_media(
            b"#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2,\na.ts\n",
            "https://example.test/index.m3u8"
        )
        .is_err());
    }

    #[test]
    fn clear_key_persists_until_next_aes_key_and_invalid_padding_is_rejected() {
        let input = b"#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXT-X-KEY:METHOD=AES-128,URI=\"key\"\n#EXTINF:2,\na.ts\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:2,\nb.ts\n#EXTINF:2,\nc.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"key2\"\n#EXTINF:2,\nd.ts\n#EXT-X-ENDLIST\n";
        let plan = parse_media(input, "http://example.test/index.m3u8").unwrap();
        assert!(plan.segments[1].encryption.is_none());
        assert!(plan.segments[2].encryption.is_none());
        assert!(plan.segments[3]
            .encryption
            .as_ref()
            .unwrap()
            .url
            .ends_with("key2"));
        assert!(decrypt(&[0u8; 16], &[0u8; 16], &[0u8; 16]).is_err());
        assert!(decrypt(&[0u8; 17], &[0u8; 16], &[0u8; 16]).is_err());
        assert!(decrypt(&[0u8; 16], &[0u8; 15], &[0u8; 16]).is_err());
    }

    #[test]
    fn named_download_directory_and_legacy_directory_are_distinct() {
        assert_eq!(
            super::drama_dir(std::path::Path::new("/tmp"), 21, "old")
                .file_name()
                .unwrap(),
            "drama-21-old"
        );
        assert_eq!(
            super::legacy_drama_dir(std::path::Path::new("/tmp"), 21)
                .file_name()
                .unwrap(),
            "drama-21"
        );
    }

    #[test]
    fn unsafe_or_empty_titles_fall_back_to_id_only_directory() {
        assert!(super::safe_title("  ").is_none());
        assert_eq!(
            super::drama_dir(std::path::Path::new("/tmp"), 21, "a/b")
                .file_name()
                .unwrap(),
            "drama-21-a_b"
        );
        assert_eq!(
            super::drama_dir(std::path::Path::new("/tmp"), 21, "")
                .file_name()
                .unwrap(),
            "drama-21"
        );
    }

    #[tokio::test]
    async fn old_id_only_episode_path_is_kept_for_compatibility() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/tests/compat-path");
        let old = super::legacy_episode_path(&root, 21, 1);
        tokio::fs::create_dir_all(old.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&old, b"old").await.unwrap();
        assert_eq!(super::episode_path_for_read(&root, 21, "剧名", 1), old);
        let current = super::episode_path(&root, 21, "剧名", 1);
        tokio::fs::create_dir_all(current.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&current, b"new").await.unwrap();
        assert_eq!(super::episode_path_for_read(&root, 21, "剧名", 1), current);
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
