use super::*;
use std::path::{Path, PathBuf};

async fn write_receipted_file(path: &Path, bytes: &[u8]) {
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(path, bytes).await.unwrap();
    let receipt = serde_json::json!({
        "schema_version": 1,
        "byte_len": bytes.len(),
        "sha256": hls::hash(bytes),
    });
    tokio::fs::write(
        path.with_extension("json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .await
    .unwrap();
}

fn test_dir(label: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local/tests")
        .join(format!("{label}-{}", storage::unique_stamp()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn task(root: &Path, drama_id: i64, episode: u32, source_url: &str) -> DownloadTask {
    DownloadTask {
        schema_version: 1,
        id: format!("{drama_id}-{episode}"),
        drama_id,
        drama_title: format!("Test {drama_id}"),
        episode,
        episode_title: format!("Episode {episode}"),
        source_url: source_url.into(),
        root_dir: root.to_string_lossy().into_owned(),
        status: DownloadStatus::Queued,
        progress: 0.0,
        total_segments: 0,
        completed_segments: vec![],
        error: None,
        created_at: "1".into(),
        updated_at: "1".into(),
        init_segment: None,
        cover_url: None,
    }
}

#[tokio::test]
async fn library_groups_verified_files_and_paginates_by_drama() {
    let dir = test_dir("library-page");
    let tasks = vec![
        task(&dir, 1, 1, ""),
        task(&dir, 1, 2, ""),
        task(&dir, 2, 1, ""),
    ];
    for item in &tasks {
        let output = episode_path(&dir, item.drama_id, &item.drama_title, item.episode);
        write_receipted_file(
            &output,
            format!("{}-{}", item.drama_id, item.episode).as_bytes(),
        )
        .await;
    }
    let first = library::build_page(&tasks, &dir, 0, 1).await.unwrap();
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.items[0].drama_id, 1);
    assert_eq!(first.items[0].episodes.len(), 2);
    assert_eq!(first.next_cursor.as_deref(), Some("1"));
    let second = library::build_page(&tasks, &dir, 1, 1).await.unwrap();
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].drama_id, 2);
    assert!(second.next_cursor.is_none());
}

#[tokio::test]
async fn library_ignores_missing_or_bad_receipts_and_reads_legacy_directory() {
    let dir = test_dir("library-compat");
    let mut valid = task(&dir, 3, 1, "");
    valid.drama_title = "".into();
    let missing = task(&dir, 3, 2, "");
    let bad = task(&dir, 3, 3, "");
    write_receipted_file(&legacy_episode_path(&dir, 3, 1), b"legacy-video").await;
    let bad_path = episode_path(&dir, 3, &bad.drama_title, bad.episode);
    tokio::fs::create_dir_all(bad_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&bad_path, b"bad-video").await.unwrap();
    tokio::fs::write(
        bad_path.with_extension("json"),
        br#"{"schema_version":1,"byte_len":999,"sha256":"wrong"}"#,
    )
    .await
    .unwrap();
    let page = library::build_page(&[valid, missing, bad], &dir, 0, 20)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].drama_id, 3);
    assert_eq!(page.items[0].episodes.len(), 1);
    assert_eq!(page.items[0].episodes[0].episode, 1);
}

#[test]
fn library_update_candidates_only_include_new_episode_numbers() {
    assert_eq!(library::new_episode_numbers(&[1, 2, 5], &[1, 2]), vec![5]);
    assert_eq!(
        library::new_episode_numbers(&[1, 2], &[1, 2]),
        Vec::<u32>::new()
    );
}

#[tokio::test]
async fn damaged_store_is_preserved_with_visible_warning_and_unknown_records_survive() {
    let dir = test_dir("store");
    let store = storage::Store::new(dir.clone());
    let path = dir.join("downloads/tasks.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[{broken").unwrap();
    let loaded = store.load().await.unwrap();
    assert!(!loaded.warnings.is_empty());
    assert!(std::fs::read_dir(path.parent().unwrap()).unwrap().any(|p| p
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("corrupt-")));
    let future = serde_json::json!({"schema_version": 99, "future_data": [1, 2]});
    let mut known = serde_json::to_value(task(&dir, 1, 1, "http://localhost/test")).unwrap();
    known["future_field"] = serde_json::json!("keep-me");
    std::fs::write(
        &path,
        serde_json::to_vec(&vec![future.clone(), known]).unwrap(),
    )
    .unwrap();
    let mut loaded = store.load().await.unwrap();
    loaded.tasks[0].status = DownloadStatus::Paused;
    store.save(&loaded.tasks, &loaded.raw).await.unwrap();
    let raw: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(raw.contains(&future));
    assert!(raw
        .iter()
        .any(|item| item["future_field"] == "keep-me" && item["status"] == "paused"));
}

#[tokio::test]
async fn deleting_failed_and_cancelled_tasks_keeps_completed_media_and_unknown_records() {
    let dir = test_dir("delete-tasks");
    let app_dir = dir.join("app");
    let mut failed = task(&dir, 1, 1, "");
    failed.status = DownloadStatus::Failed;
    let mut cancelled = task(&dir, 2, 1, "");
    cancelled.status = DownloadStatus::Cancelled;
    let mut completed = task(&dir, 3, 1, "");
    completed.status = DownloadStatus::Completed;
    let completed_path = episode_path(&dir, 3, &completed.drama_title, 1);
    write_receipted_file(&completed_path, b"keep-me").await;
    for item in [&failed, &cancelled] {
        tokio::fs::create_dir_all(task_temp_dir_for_resume(
            Path::new(&item.root_dir),
            item.drama_id,
            &item.drama_title,
            item.episode,
        ))
        .await
        .unwrap();
    }
    let store = storage::Store::new(app_dir.clone());
    let mut raw = vec![
        serde_json::to_value(&failed).unwrap(),
        serde_json::to_value(&cancelled).unwrap(),
        serde_json::to_value(&completed).unwrap(),
        serde_json::json!({"schema_version": 99, "id": "future", "keep": true}),
    ];
    store
        .save(
            &[failed.clone(), cancelled.clone(), completed.clone()],
            &raw,
        )
        .await
        .unwrap();
    raw.clear();
    let manager = DownloadManager::new(5);
    manager.initialize(app_dir.clone()).await.unwrap();
    assert_eq!(
        manager.delete(vec![failed.id, cancelled.id]).await.unwrap(),
        2
    );
    let remaining = manager.tasks().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].status, DownloadStatus::Completed);
    assert!(completed_path.exists());
    assert!(!task_temp_dir_for_resume(Path::new(&completed.root_dir), 1, "Test 1", 1).exists());
    let saved: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(app_dir.join("downloads/tasks.json")).unwrap())
            .unwrap();
    assert!(saved.iter().any(|entry| entry["keep"] == true));
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn deleting_queued_or_paused_tasks_is_rejected() {
    let dir = test_dir("delete-reject");
    let app_dir = dir.join("app");
    let mut queued = task(&dir, 4, 1, "");
    queued.status = DownloadStatus::Paused;
    let store = storage::Store::new(app_dir.clone());
    store
        .save(std::slice::from_ref(&queued), &[])
        .await
        .unwrap();
    let manager = DownloadManager::new(5);
    manager.initialize(app_dir).await.unwrap();
    let error = manager.delete(vec![queued.id.clone()]).await.unwrap_err();
    assert!(error.contains("失败或取消"));
    assert_eq!(manager.tasks().await.unwrap().len(), 1);
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn old_settings_without_concurrency_fields_use_new_defaults() {
    let dir = test_dir("settings-defaults");
    let store = storage::Store::new(dir.clone());
    let path = dir.join("downloads/settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        serde_json::json!({
            "schema_version": 1,
            "root_dir": "/tmp/hghub"
        })
        .to_string(),
    )
    .unwrap();

    let settings = store.settings(PathBuf::from("/tmp/default")).await.unwrap();
    assert_eq!(settings.max_tasks, 5);
    assert_eq!(settings.max_episodes_per_drama, 3);
    assert_eq!(settings.max_segments_per_episode, 4);
}

#[test]
fn scheduler_rotates_drama_and_does_not_start_another_episode_of_active_drama() {
    let tasks = vec![
        task(Path::new("/tmp"), 1, 1, ""),
        task(Path::new("/tmp"), 1, 2, ""),
        task(Path::new("/tmp"), 2, 1, ""),
        task(Path::new("/tmp"), 3, 1, ""),
    ];
    assert_eq!(
        manager::next_task(&tasks, &[("1-1".into(), 1)], Some(1), 1),
        Some(2)
    );
    assert_eq!(manager::next_task(&tasks, &[], Some(2), 1), Some(3));
    assert_eq!(manager::next_task(&tasks, &[], Some(3), 1), Some(0));
}

#[test]
fn scheduler_allows_three_active_episodes_per_drama_but_rejects_the_fourth() {
    let tasks = vec![
        task(Path::new("/tmp"), 1, 1, ""),
        task(Path::new("/tmp"), 1, 2, ""),
        task(Path::new("/tmp"), 1, 3, ""),
        task(Path::new("/tmp"), 1, 4, ""),
    ];
    let active = vec![("1-1".into(), 1), ("1-2".into(), 1)];
    assert_eq!(manager::next_task(&tasks, &active, Some(1), 3), Some(2));

    let active = vec![("1-1".into(), 1), ("1-2".into(), 1), ("1-3".into(), 1)];
    assert_eq!(manager::next_task(&tasks, &active, Some(1), 3), None);
}

#[test]
fn scheduler_rotates_four_dramas_when_filling_five_slots() {
    let tasks: Vec<_> = (1..=4)
        .flat_map(|drama| (1..=4).map(move |episode| task(Path::new("/tmp"), drama, episode, "")))
        .collect();
    let mut active = Vec::new();
    let mut last = None;
    for _ in 0..5 {
        let index = manager::next_task(&tasks, &active, last, 3).unwrap();
        let chosen = &tasks[index];
        active.push((chosen.id.clone(), chosen.drama_id));
        last = Some(chosen.drama_id);
    }
    assert_eq!(
        active.iter().map(|(_, drama)| *drama).collect::<Vec<_>>(),
        [1, 2, 3, 4, 1]
    );
}

#[tokio::test]
async fn concurrency_settings_persist_keep_unknown_fields_and_survive_root_changes() {
    let dir = test_dir("limits-settings");
    let app_dir = dir.join("app");
    let store = storage::Store::new(app_dir.clone());
    let settings = DownloadSettings::new(dir.clone());
    store.save_settings(&settings).await.unwrap();
    let path = app_dir.join("downloads/settings.json");
    let mut value = serde_json::to_value(&settings).unwrap();
    value["future_field"] = serde_json::json!({"keep": true});
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let manager = DownloadManager::new(5);
    manager.initialize(app_dir.clone()).await.unwrap();
    let limits = DownloadLimits {
        max_tasks: 7,
        max_episodes_per_drama: 2,
        max_segments_per_episode: 6,
    };
    let new_root = dir.join("other");
    let (changed, moved) = tokio::join!(
        manager.set_limits(dir.clone(), limits),
        manager.set_root(dir.clone(), new_root.clone()),
    );
    changed.unwrap();
    moved.unwrap();
    let saved = manager.settings(dir.clone()).await.unwrap();
    assert_eq!(saved.root_dir, new_root.to_string_lossy());
    assert_eq!(saved.limits(), limits);
    assert_eq!(manager.limits(), limits);
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(raw["future_field"], value["future_field"]);
    manager.shutdown().await.unwrap();

    let restarted = DownloadManager::new(5);
    restarted.initialize(app_dir).await.unwrap();
    assert_eq!(restarted.limits(), limits);
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_limits_never_change_saved_settings_or_running_limits() {
    let dir = test_dir("invalid-limits");
    let manager = DownloadManager::new(5);
    manager.initialize(dir.join("app")).await.unwrap();
    let settings = manager.settings(dir.clone()).await.unwrap();
    let path = dir.join("app/downloads/settings.json");
    let before = std::fs::read(&path).unwrap();
    for limits in [
        DownloadLimits {
            max_tasks: 0,
            ..DownloadLimits::default()
        },
        DownloadLimits {
            max_tasks: 33,
            ..DownloadLimits::default()
        },
        DownloadLimits {
            max_episodes_per_drama: 0,
            ..DownloadLimits::default()
        },
        DownloadLimits {
            max_segments_per_episode: 17,
            ..DownloadLimits::default()
        },
    ] {
        assert!(manager.set_limits(dir.clone(), limits).await.is_err());
        assert_eq!(manager.limits(), settings.limits());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn unreadable_concurrency_settings_warn_without_overwriting_the_file() {
    for bad_limits in [serde_json::json!(0), serde_json::json!("bad")] {
        let dir = test_dir("bad-saved-limits");
        let app = dir.join("app");
        let path = app.join("downloads/settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "root_dir": dir, "max_tasks": bad_limits,
        }))
        .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let manager = DownloadManager::new(5);
        manager.initialize(app).await.unwrap();
        assert_eq!(manager.limits(), DownloadLimits::default());
        assert!(!manager.warnings().await.unwrap().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        manager.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn future_settings_cannot_be_overwritten_or_applied_as_current_limits() {
    let dir = test_dir("future-limits");
    let manager = DownloadManager::new(5);
    manager.initialize(dir.join("app")).await.unwrap();
    manager.settings(dir.clone()).await.unwrap();
    let path = dir.join("app/downloads/settings.json");
    let bytes = br#"{"schema_version":99,"root_dir":{"future":true},"max_tasks":100}"#;
    std::fs::write(&path, bytes).unwrap();
    assert!(manager
        .set_limits(
            dir,
            DownloadLimits {
                max_tasks: 1,
                ..DownloadLimits::default()
            }
        )
        .await
        .is_err());
    assert_eq!(manager.limits(), DownloadLimits::default());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    manager.shutdown().await.unwrap();
}

use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

struct Source {
    base: String,
    stop: Arc<AtomicBool>,
    expired: Arc<AtomicBool>,
    hold: Arc<AtomicBool>,
    counts: Arc<Mutex<HashMap<String, usize>>>,
    peak: Arc<AtomicUsize>,
    segments_active: Arc<AtomicUsize>,
    segments_peak: Arc<AtomicUsize>,
}
impl Source {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let expired = Arc::new(AtomicBool::new(false));
        let hold = Arc::new(AtomicBool::new(false));
        let counts = Arc::new(Mutex::new(HashMap::new()));
        let concurrent = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let segments_active = Arc::new(AtomicUsize::new(0));
        let segments_peak = Arc::new(AtomicUsize::new(0));
        let (segments_copy, segment_peak_copy) = (segments_active.clone(), segments_peak.clone());
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hls");
        let (stop_copy, exp_copy, counts_copy, peak_copy, hold_copy) = (
            stop.clone(),
            expired.clone(),
            counts.clone(),
            peak.clone(),
            hold.clone(),
        );
        std::thread::spawn(move || {
            while !stop_copy.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(3));
                        continue;
                    }
                };
                // Accepted sockets inherit the listener's nonblocking flag on
                // macOS; fixture request reads need blocking socket semantics.
                socket.set_nonblocking(false).unwrap();
                let (expired, counts, concurrent, peak, fixture, hold, segments, segment_peak) = (
                    exp_copy.clone(),
                    counts_copy.clone(),
                    concurrent.clone(),
                    peak_copy.clone(),
                    fixture.clone(),
                    hold_copy.clone(),
                    segments_copy.clone(),
                    segment_peak_copy.clone(),
                );
                std::thread::spawn(move || {
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut buffer = [0u8; 4096];
                    let mut request = Vec::new();
                    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        let Ok(count) = socket.read(&mut buffer) else {
                            return;
                        };
                        if count == 0 || request.len() + count > 16384 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..count]);
                    }
                    let request = String::from_utf8_lossy(&request);
                    let Some(uri) = request.split_whitespace().nth(1) else {
                        return;
                    };
                    let uri = uri.to_string();
                    eprintln!("[fixture] request {uri}");
                    *counts.lock().unwrap().entry(uri.clone()).or_insert(0) += 1;
                    let active = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(active, Ordering::SeqCst);
                    let name = uri.split('?').next().unwrap().rsplit('/').next().unwrap();
                    let is_segment = name.ends_with(".ts") || name.ends_with(".m4s");
                    if is_segment {
                        let active = segments.fetch_add(1, Ordering::SeqCst) + 1;
                        segment_peak.fetch_max(active, Ordering::SeqCst);
                    }
                    if name == "slow-cover.png" {
                        std::thread::sleep(Duration::from_millis(1000));
                    }
                    if is_segment {
                        if !name.ends_with("0.ts") && !name.ends_with("0.m4s") {
                            for _ in 0..1000 {
                                if !hold.load(Ordering::SeqCst) {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(5));
                            }
                        }
                        std::thread::sleep(if name.ends_with("0.ts") || name.ends_with("0.m4s") {
                            Duration::from_millis(30)
                        } else {
                            Duration::from_millis(600)
                        });
                    }
                    let forbidden = name == "always403.m3u8"
                        || (expired.load(Ordering::SeqCst)
                            && uri.contains("auth_key=old")
                            && !name.ends_with(".m3u8"));
                    let (status, body) = if forbidden {
                        (403, b"expired".to_vec())
                    } else if name == "cover.png" || name == "slow-cover.png" {
                        use base64::Engine;
                        (200, base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6WQAAAAASUVORK5CYII=").unwrap())
                    } else if name == "concurrency.m3u8" {
                        // A held local stream for observing real request counts.
                        // Tests cancel it rather than treating repeated media as
                        // a valid remux/decode fixture.
                        let mut playlist = String::from("#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:2\n#EXT-X-MAP:URI=\"plain-fmp4-init.mp4\"\n");
                        for index in 0..12 {
                            playlist
                                .push_str(&format!("#EXTINF:2,\nplain-fmp41.m4s?piece={index}\n"));
                        }
                        playlist.push_str("#EXT-X-ENDLIST\n");
                        (200, playlist.into_bytes())
                    } else if let Ok(mut bytes) = std::fs::read(fixture.join(name)) {
                        if name.ends_with(".m3u8") {
                            let signature = if uri.contains("auth_key=new") {
                                "new"
                            } else {
                                "old"
                            };
                            let mut text = String::from_utf8(bytes).unwrap();
                            for target in [
                                "aes-ts0.ts",
                                "aes-ts1.ts",
                                "aes-ts2.ts",
                                "key.bin",
                                "plain-fmp40.m4s",
                                "plain-fmp41.m4s",
                                "plain-fmp42.m4s",
                                "plain-fmp4-init.mp4",
                            ] {
                                text =
                                    text.replace(target, &format!("{target}?auth_key={signature}"));
                            }
                            bytes = text.into_bytes();
                        }
                        (200, bytes)
                    } else {
                        (404, b"not found".to_vec())
                    };
                    let _ = write!(
                        socket,
                        "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let sent = socket.write_all(&body);
                    eprintln!(
                        "[fixture] response {uri} status={status} length={} result={sent:?}",
                        body.len()
                    );
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                    if is_segment {
                        segments.fetch_sub(1, Ordering::SeqCst);
                    }
                });
            }
        });
        Self {
            base,
            stop,
            expired,
            counts,
            peak,
            hold,
            segments_active,
            segments_peak,
        }
    }
    fn url(&self, drama: i64, name: &str, signature: &str) -> String {
        format!("{}/{drama}/{name}?auth_key={signature}", self.base)
    }
    fn count(&self, suffix: &str) -> usize {
        self.counts
            .lock()
            .unwrap()
            .iter()
            .filter(|(uri, _)| uri.contains(suffix))
            .map(|(_, count)| count)
            .sum()
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

async fn wait_for(
    manager: &DownloadManager,
    id: &str,
    accept: impl Fn(&DownloadTask) -> bool,
) -> DownloadTask {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let tasks = manager.tasks().await.unwrap();
            // Scheduling limits have dedicated integration assertions below.
            // This helper also waits while a reduced limit drains old workers.
            if let Some(task) = tasks.into_iter().find(|task| task.id == id) {
                if accept(&task) {
                    return task;
                }
                assert_ne!(
                    task.status,
                    DownloadStatus::Failed,
                    "unexpected download failure: {:?}",
                    task.error
                );
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    })
    .await
    .expect("download checkpoint timed out")
}

async fn decode(path: &Path) {
    let ffmpeg = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/ffmpeg/runtime/bin/ffmpeg");
    let output = tokio::process::Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-an", "-f", "null", "-"])
        .output()
        .await
        .expect("build repository-local FFmpeg before integration tests");
    assert!(
        output.status.success(),
        "decode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let probe = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/ffmpeg/runtime/bin/ffprobe");
    let result = tokio::process::Command::new(probe)
        .args([
            "-v",
            "error",
            "-count_frames",
            "-show_entries",
            "stream=codec_name,nb_read_frames,duration",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "ffprobe audio/video decode failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let streams = value["streams"].as_array().unwrap();
    for codec in ["h264", "aac"] {
        let stream = streams
            .iter()
            .find(|stream| stream["codec_name"] == codec)
            .expect("expected audio/video stream missing");
        assert!(
            stream["nb_read_frames"]
                .as_str()
                .unwrap()
                .parse::<usize>()
                .unwrap()
                > 100
        );
        let duration = stream["duration"].as_str().unwrap().parse::<f64>().unwrap();
        assert!(
            (5.9..6.2).contains(&duration),
            "unexpected clip duration {duration}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_encrypted_ts_and_fmp4_download_remux_and_decode() {
    let source = Source::start();
    let dir = test_dir("download");
    let manager = DownloadManager::new(2);
    manager.initialize(dir.join("app")).await.unwrap();
    manager
        .enqueue(task(&dir, 1, 1, &source.url(1, "aes-ts.m3u8", "old")))
        .await
        .unwrap();
    manager
        .enqueue(task(&dir, 2, 1, &source.url(2, "plain-fmp4.m3u8", "old")))
        .await
        .unwrap();
    wait_for(&manager, "1-1", |task| {
        task.status == DownloadStatus::Completed
    })
    .await;
    wait_for(&manager, "2-1", |task| {
        task.status == DownloadStatus::Completed
    })
    .await;
    assert!(
        source.peak.load(Ordering::SeqCst) >= 3,
        "segments did not run concurrently"
    );
    decode(&episode_path(&dir, 1, "", 1)).await;
    decode(&episode_path(&dir, 2, "", 1)).await;
    assert!(episode_path(&dir, 1, "", 1).with_extension("json").exists());
    manager.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn default_limits_run_five_tasks_three_per_drama_and_twenty_segments() {
    let source = Source::start();
    source.hold.store(true, Ordering::SeqCst);
    let dir = test_dir("default-concurrency");
    let tasks: Vec<_> = (1..=2)
        .flat_map(|drama| {
            let source = &source;
            let dir = &dir;
            (1..=4).map(move |episode| {
                task(
                    dir,
                    drama,
                    episode,
                    &source.url(drama, "concurrency.m3u8", "old"),
                )
            })
        })
        .collect();
    let store = storage::Store::new(dir.join("app"));
    store.save(&tasks, &[]).await.unwrap();
    let manager = DownloadManager::new(5);
    manager.initialize(dir.join("app")).await.unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let tasks = manager.tasks().await.unwrap();
            let active: Vec<_> = tasks
                .iter()
                .filter(|task| task.status == DownloadStatus::Downloading)
                .collect();
            assert!(active.len() <= 5);
            for drama in 1..=2 {
                assert!(active.iter().filter(|task| task.drama_id == drama).count() <= 3);
            }
            assert!(source.segments_peak.load(Ordering::SeqCst) <= 20);
            if active.len() == 5 && source.segments_active.load(Ordering::SeqCst) == 20 {
                assert_eq!(
                    tasks
                        .iter()
                        .filter(|task| task.status == DownloadStatus::Queued)
                        .count(),
                    3
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("default concurrency did not fill five tasks and twenty segment slots");
    manager.shutdown().await.unwrap();
    source.hold.store(false, Ordering::SeqCst);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn workers_use_custom_one_and_two_segment_limits() {
    for segment_limit in [1, 2] {
        let source = Source::start();
        source.hold.store(true, Ordering::SeqCst);
        let dir = test_dir("custom-segments");
        let manager = DownloadManager::new(5);
        manager.initialize(dir.join("app")).await.unwrap();
        manager
            .set_limits(
                dir.clone(),
                DownloadLimits {
                    max_segments_per_episode: segment_limit,
                    ..DownloadLimits::default()
                },
            )
            .await
            .unwrap();
        manager
            .enqueue(task(&dir, 1, 1, &source.url(1, "concurrency.m3u8", "old")))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                assert!(source.segments_peak.load(Ordering::SeqCst) <= segment_limit);
                if source.segments_active.load(Ordering::SeqCst) == segment_limit {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // More segments are ready in the playlist, but no extra request may
        // start while the configured slots are occupied.
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(source.segments_peak.load(Ordering::SeqCst), segment_limit);
        manager.shutdown().await.unwrap();
        source.hold.store(false, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lowering_limits_preserves_running_workers_and_drains_before_starting_more() {
    let source = Source::start();
    source.hold.store(true, Ordering::SeqCst);
    let dir = test_dir("limits-drain");
    let store = storage::Store::new(dir.join("app"));
    let tasks = vec![
        task(&dir, 1, 1, &source.url(1, "aes-ts.m3u8", "old")),
        task(&dir, 1, 2, &source.url(1, "aes-ts.m3u8", "old")),
        task(&dir, 2, 1, &source.url(2, "aes-ts.m3u8", "old")),
    ];
    store.save(&tasks, &[]).await.unwrap();
    let manager = DownloadManager::new(2);
    manager.initialize(dir.join("app")).await.unwrap();
    wait_for(&manager, "1-1", |task| !task.completed_segments.is_empty()).await;
    wait_for(&manager, "2-1", |task| !task.completed_segments.is_empty()).await;
    let before: Vec<_> = manager
        .tasks()
        .await
        .unwrap()
        .into_iter()
        .filter(|task| task.status == DownloadStatus::Downloading)
        .map(|task| task.id)
        .collect();
    assert_eq!(before.len(), 2);
    manager
        .set_limits(
            dir.clone(),
            DownloadLimits {
                max_tasks: 1,
                max_episodes_per_drama: 1,
                max_segments_per_episode: 1,
            },
        )
        .await
        .unwrap();
    let after: Vec<_> = manager
        .tasks()
        .await
        .unwrap()
        .into_iter()
        .filter(|task| task.status == DownloadStatus::Downloading)
        .map(|task| task.id)
        .collect();
    assert_eq!(
        after, before,
        "changing limits interrupted or requeued existing workers"
    );
    source.hold.store(false, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let tasks = manager.tasks().await.unwrap();
            assert!(tasks
                .iter()
                .all(|task| task.status != DownloadStatus::Failed));
            let completed = tasks
                .iter()
                .filter(|task| task.status == DownloadStatus::Completed)
                .count();
            if completed >= 1 {
                assert!(
                    tasks
                        .iter()
                        .filter(|task| task.status == DownloadStatus::Downloading)
                        .count()
                        <= 1
                );
            }
            if completed == 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    })
    .await
    .unwrap();
    decode(&episode_path(&dir, 1, "Test 1", 1)).await;
    decode(&episode_path(&dir, 1, "Test 1", 2)).await;
    decode(&episode_path(&dir, 2, "Test 2", 1)).await;
    manager.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "live source episode download; writes only repository-local test data"]
async fn live_source_episode_download_and_full_decode() {
    let dir = std::env::var_os("HGHUB_LIVE_DOWNLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| test_dir("live-source"));
    let approved = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local/tests")
        .canonicalize()
        .unwrap();
    assert!(
        dir.canonicalize().unwrap().starts_with(approved),
        "live fixture must stay inside repository-local tests"
    );
    let manager = DownloadManager::new(2);
    manager.initialize(dir.join("app")).await.unwrap();
    manager.enqueue(task(&dir, 117, 3, "")).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(240), async {
        loop {
            let item = manager
                .tasks()
                .await
                .unwrap()
                .into_iter()
                .find(|item| item.id == "117-3")
                .unwrap();
            println!(
                "live download: {:?} {}/{} segments",
                item.status,
                item.completed_segments.len(),
                item.total_segments
            );
            if matches!(
                item.status,
                DownloadStatus::Completed | DownloadStatus::Failed
            ) {
                break item;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    })
    .await;
    manager.shutdown().await.unwrap();
    let finished = result.expect("live episode download exceeded bounded 4 minute check");
    assert_eq!(
        finished.status,
        DownloadStatus::Completed,
        "{:?}",
        finished.error
    );
    let output = episode_path(&dir, 117, "", 3);
    // ffprobe counts decoded audio/video frames without re-encoding float AAC
    // into PCM s16 (the minimal bundled remux tool has no resampling filter).
    let probe = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/ffmpeg/runtime/bin/ffprobe");
    let info = tokio::process::Command::new(probe)
        .args([
            "-v",
            "error",
            "-count_frames",
            "-show_entries",
            "format=duration,size:stream=codec_name,nb_read_frames,duration",
            "-of",
            "json",
        ])
        .arg(&output)
        .output()
        .await
        .unwrap();
    assert!(info.status.success());
    assert!(
        info.stderr.is_empty(),
        "decode errors: {}",
        String::from_utf8_lossy(&info.stderr)
    );
    let decoded: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
    let streams = decoded["streams"].as_array().unwrap();
    assert!(streams.len() >= 2, "expected audio and video streams");
    for stream in streams {
        assert!(
            stream["nb_read_frames"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
    }
    println!("live output: {}", output.display());
    println!("{}", String::from_utf8_lossy(&info.stdout));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_releases_capacity_persists_and_cancel_can_retry() {
    let source = Source::start();
    source.hold.store(true, Ordering::SeqCst);
    let dir = test_dir("pause");
    let manager = DownloadManager::new(2);
    manager.initialize(dir.join("app")).await.unwrap();
    for drama in 1..=3 {
        manager
            .enqueue(task(
                &dir,
                drama,
                1,
                &source.url(drama, "aes-ts.m3u8", "old"),
            ))
            .await
            .unwrap();
    }
    wait_for(&manager, "1-1", |task| !task.completed_segments.is_empty()).await;
    manager.pause("1-1").await.unwrap();
    assert_eq!(
        manager.tasks().await.unwrap()[0].status,
        DownloadStatus::Paused
    );
    wait_for(&manager, "3-1", |task| {
        task.status == DownloadStatus::Downloading || task.status == DownloadStatus::Completed
    })
    .await;
    manager.shutdown().await.unwrap();
    let second = DownloadManager::new(2);
    second.initialize(dir.join("app")).await.unwrap();
    assert_eq!(
        second.tasks().await.unwrap()[0].status,
        DownloadStatus::Paused
    );
    let first_count = source.count("/1/aes-ts0.ts");
    let segment = task_temp_dir(&dir, 1, "", 1).join("000001.ts");
    assert!(segment.exists());
    std::fs::write(segment, b"truncated").unwrap();
    second.cancel("1-1").await.unwrap();
    assert_eq!(
        second.tasks().await.unwrap()[0].status,
        DownloadStatus::Cancelled
    );
    source.hold.store(false, Ordering::SeqCst);
    second.resume("1-1").await.unwrap();
    wait_for(&second, "1-1", |task| {
        task.status == DownloadStatus::Completed
    })
    .await;
    assert_eq!(
        source.count("/1/aes-ts0.ts"),
        first_count + 1,
        "damaged segment was incorrectly reused"
    );
    decode(&episode_path(&dir, 1, "", 1)).await;
    second.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segment_expiry_refreshes_once_and_persistent_expiry_fails() {
    let source = Source::start();
    source.expired.store(true, Ordering::SeqCst);
    let dir = test_dir("expiry");
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let base = source.base.clone();
    let manager = DownloadManager::with_resolver(
        2,
        Arc::new(move |drama, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            let url = format!("{base}/{drama}/aes-ts.m3u8?auth_key=new");
            Box::pin(async move { Ok(url) })
        }),
    );
    manager.initialize(dir.join("app")).await.unwrap();
    manager
        .enqueue(task(&dir, 1, 1, &source.url(1, "aes-ts.m3u8", "old")))
        .await
        .unwrap();
    wait_for(&manager, "1-1", |task| {
        task.status == DownloadStatus::Completed
    })
    .await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
    manager.shutdown().await.unwrap();
    let base = source.base.clone();
    let failing_count = Arc::new(AtomicUsize::new(0));
    let copy = failing_count.clone();
    let failing = DownloadManager::with_resolver(
        2,
        Arc::new(move |_, _| {
            copy.fetch_add(1, Ordering::SeqCst);
            let url = format!("{base}/1/always403.m3u8");
            Box::pin(async move { Ok(url) })
        }),
    );
    failing.initialize(dir.join("fail-app")).await.unwrap();
    failing
        .enqueue(task(&dir, 9, 1, &source.url(9, "always403.m3u8", "old")))
        .await
        .unwrap();
    let failed = wait_for(&failing, "9-1", |task| {
        task.status == DownloadStatus::Failed
    })
    .await;
    assert!(failed.error.unwrap().contains("HTTP 403"));
    assert_eq!(failing_count.load(Ordering::SeqCst), 1);
    failing.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn killed_process_resumes_verified_segments_after_signature_refresh() {
    if let Ok(child_dir) = std::env::var("HGHUB_TEST_CHILD_DIR") {
        let dir = PathBuf::from(child_dir);
        let url = std::env::var("HGHUB_TEST_CHILD_URL").unwrap();
        let manager = DownloadManager::new(2);
        manager.initialize(dir.join("app")).await.unwrap();
        manager.enqueue(task(&dir, 1, 1, &url)).await.unwrap();
        tokio::time::sleep(Duration::from_secs(45)).await;
        panic!("parent did not terminate child process at download checkpoint");
    }
    let source = Source::start();
    source.hold.store(true, Ordering::SeqCst);
    let dir = test_dir("sigkill");
    let log = std::fs::File::create(dir.join("child.log")).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "downloads::tests::killed_process_resumes_verified_segments_after_signature_refresh",
            "--nocapture",
        ])
        .env("HGHUB_TEST_CHILD_DIR", &dir)
        .env("HGHUB_TEST_CHILD_URL", source.url(1, "aes-ts.m3u8", "old"))
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .unwrap();
    let store = storage::Store::new(dir.join("app"));
    let checkpoint = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(loaded) = store.load().await {
                if loaded
                    .tasks
                    .first()
                    .is_some_and(|task| !task.completed_segments.is_empty())
                {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    child.kill().unwrap(); // std::process::Child::kill sends SIGKILL on macOS.
    let status = child.wait().unwrap();
    checkpoint.expect("child did not publish a verified segment before SIGKILL");
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(9));
    let before = source.count("aes-ts0.ts");
    source.expired.store(true, Ordering::SeqCst);
    source.hold.store(false, Ordering::SeqCst);
    let refreshed = Arc::new(AtomicUsize::new(0));
    let counter = refreshed.clone();
    let url = source.url(1, "aes-ts.m3u8", "new");
    let manager = DownloadManager::with_resolver(
        2,
        Arc::new(move |_, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            let url = url.clone();
            Box::pin(async move { Ok(url) })
        }),
    );
    manager.initialize(dir.join("app")).await.unwrap();
    wait_for(&manager, "1-1", |task| {
        task.status == DownloadStatus::Completed
    })
    .await;
    assert_eq!(
        source.count("aes-ts0.ts"),
        before,
        "verified first segment downloaded again after restart"
    );
    assert_eq!(refreshed.load(Ordering::SeqCst), 1);
    decode(&episode_path(&dir, 1, "", 1)).await;
    manager.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_cover_requests_fetch_once_and_memory_reset_uses_disk() {
    let source = Source::start();
    let dir = test_dir("covers");
    crate::media::bind_cache(dir);
    let url = source.url(1, "cover.png", "old");
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let url = url.clone();
        requests.spawn(async move { crate::media::load_image(&url).await.unwrap() });
    }
    while let Some(result) = requests.join_next().await {
        assert_eq!(result.unwrap().0, "image/png");
    }
    assert_eq!(source.count("cover.png"), 1);
    crate::media::clear_memory_caches();
    let (_, bytes) = crate::media::load_image(&url).await.unwrap();
    assert!(bytes.len() > 60);
    assert_eq!(
        source.count("cover.png"),
        1,
        "disk cache was not reused after memory reset"
    );
    crate::media::clear_memory_caches();
    crate::media::load_image(&source.url(1, "cover.png", "new"))
        .await
        .unwrap();
    assert_eq!(
        source.count("cover.png"),
        1,
        "expiring image signature bypassed disk cache"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_completed_mp4_can_be_downloaded_again() {
    let source = Source::start();
    let dir = test_dir("missing-output");
    let manager = DownloadManager::new(2);
    manager.initialize(dir.join("app")).await.unwrap();
    let original = task(&dir, 1, 1, &source.url(1, "aes-ts.m3u8", "old"));
    manager.enqueue(original.clone()).await.unwrap();
    wait_for(&manager, "1-1", |task| {
        task.status == DownloadStatus::Completed
    })
    .await;
    let path = episode_path(&dir, 1, "Test 1", 1);
    std::fs::remove_file(&path).unwrap();
    let enqueued = manager.enqueue(original).await.unwrap();
    assert_ne!(
        enqueued.status,
        DownloadStatus::Completed,
        "missing MP4 was silently treated as available"
    );
    wait_for(&manager, "1-1", |task| {
        task.status == DownloadStatus::Completed
    })
    .await;
    assert!(path.exists());
    decode(&path).await;
    manager.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_legacy_directory_is_not_redownloaded() {
    let source = Source::start();
    let dir = test_dir("legacy-completed");
    let manager = DownloadManager::new(2);
    manager.initialize(dir.join("app")).await.unwrap();
    let original = task(&dir, 1, 1, &source.url(1, "aes-ts.m3u8", "old"));
    manager.enqueue(original.clone()).await.unwrap();
    wait_for(&manager, "1-1", |task| {
        task.status == DownloadStatus::Completed
    })
    .await;
    let named = episode_path(&dir, 1, "Test 1", 1)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let legacy = legacy_drama_dir(&dir, 1);
    tokio::fs::rename(&named, &legacy).await.unwrap();
    let before = source.count("/1/aes-ts.m3u8");
    let returned = manager.enqueue(original).await.unwrap();
    assert_eq!(returned.status, DownloadStatus::Completed);
    assert_eq!(source.count("/1/aes-ts.m3u8"), before);
    manager.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_after_mp4_publication_keeps_episode_completed_and_playable() {
    let source = Source::start();
    let dir = test_dir("cancel-published");
    let manager = DownloadManager::new(2);
    manager.initialize(dir.join("app")).await.unwrap();
    let mut episode = task(&dir, 1, 1, &source.url(1, "aes-ts.m3u8", "old"));
    episode.cover_url = Some(source.url(1, "slow-cover.png", "old"));
    manager.enqueue(episode).await.unwrap();
    let path = episode_path(&dir, 1, "Test 1", 1);
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    manager.cancel("1-1").await.unwrap();
    wait_for(&manager, "1-1", |task| {
        task.status == DownloadStatus::Completed
    })
    .await;
    decode(&path).await;
    manager.shutdown().await.unwrap();
}
