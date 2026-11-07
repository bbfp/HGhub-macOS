use super::{hls, manager::Control, storage, DownloadManager, DownloadTask, SegmentRecord};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{sync::Mutex, task::JoinSet};

#[derive(Serialize, Deserialize)]
struct Receipt {
    schema_version: u32,
    byte_len: u64,
    sha256: String,
    task: DownloadTask,
}

fn identity_matches(record: &SegmentRecord, segment: &hls::Segment) -> bool {
    let matches = |saved: &str, url: &str| {
        saved == hls::resource_id(url) || saved == hls::hash(url.as_bytes())
    };
    matches(&record.uri_sha256, &segment.url)
        && record
            .sequence
            .is_none_or(|sequence| sequence == segment.sequence)
        && match (&record.key_uri_sha256, &segment.encryption) {
            (None, None) => true,
            (Some(key), Some(encryption)) => {
                matches(key, &encryption.url)
                    && record.iv.as_deref() == Some(hls::iv_string(&encryption.iv).as_str())
            }
            _ => false,
        }
}

async fn segment(
    index: usize,
    source: hls::Segment,
    path: PathBuf,
    previous: Option<SegmentRecord>,
    keys: hls::KeyCache,
    control: Arc<Control>,
) -> Result<SegmentRecord, String> {
    control.check()?;
    if let Some(previous) = previous {
        if identity_matches(&previous, &source)
            && hls::verify_file(&path, &previous).await.unwrap_or(false)
        {
            return Ok(previous);
        }
    }
    let bytes = control.run(hls::fetch_segment(&source, &keys)).await?;
    if bytes.is_empty() {
        return Err("视频片段为空".into());
    }
    control.check()?;
    storage::atomic_bytes(&path, &bytes).await?;
    Ok(SegmentRecord {
        index,
        uri_sha256: hls::resource_id(&source.url),
        byte_len: bytes.len() as u64,
        sha256: hls::hash(&bytes),
        key_uri_sha256: source.encryption.as_ref().map(|e| hls::resource_id(&e.url)),
        iv: source.encryption.as_ref().map(|e| hls::iv_string(&e.iv)),
        sequence: Some(source.sequence),
    })
}

async fn receipt_matches(path: &Path, receipt_path: &Path) -> bool {
    let Ok(bytes) = tokio::fs::read(receipt_path).await else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<Receipt>(&bytes) else {
        return false;
    };
    let Ok((length, sha)) = hls::file_hash(path).await else {
        return false;
    };
    receipt.schema_version == 1 && receipt.byte_len == length && receipt.sha256 == sha
}

pub(super) async fn download(
    manager: &DownloadManager,
    mut task: DownloadTask,
    generation: u64,
    control: Arc<Control>,
) -> Result<(), String> {
    let root = Path::new(&task.root_dir);
    let temporary =
        super::task_temp_dir_for_resume(root, task.drama_id, &task.drama_title, task.episode);
    let output = super::episode_path_for_read(root, task.drama_id, &task.drama_title, task.episode);
    let receipt_path = output.with_extension("json");
    tokio::fs::create_dir_all(&temporary)
        .await
        .map_err(|e| format!("无法创建下载临时目录：{e}"))?;
    tokio::fs::create_dir_all(output.parent().ok_or("下载路径无效")?)
        .await
        .map_err(|e| e.to_string())?;
    if output.exists() {
        if receipt_matches(&output, &receipt_path).await {
            return Ok(());
        }
        return Err(format!(
            "已有视频无法与下载记录核对，已保留且未覆盖：{}",
            output.display()
        ));
    }
    let mut refreshed = false;
    if task.source_url.is_empty() {
        task.source_url = control
            .run(manager.resolve(task.drama_id, task.episode))
            .await?;
        refreshed = true;
    }
    loop {
        control.check()?;
        let result = download_plan(
            manager,
            &mut task,
            generation,
            &control,
            &temporary,
            &output,
            &receipt_path,
        )
        .await;
        match result {
            Err(error) if hls::expired(&error) && !refreshed => {
                refreshed = true;
                task.source_url = control
                    .run(manager.resolve(task.drama_id, task.episode))
                    .await?;
                manager.progress(task.clone(), generation).await?;
            }
            result => return result,
        }
    }
}

async fn download_plan(
    manager: &DownloadManager,
    task: &mut DownloadTask,
    generation: u64,
    control: &Arc<Control>,
    temporary: &Path,
    output: &Path,
    receipt_path: &Path,
) -> Result<(), String> {
    let plan = control.run(hls::fetch_plan(&task.source_url)).await?;
    task.total_segments = plan.segments.len();
    let keys = Arc::new(Mutex::new(HashMap::new()));
    if let Some(init) = &plan.init {
        task.init_segment = Some(
            segment(
                usize::MAX,
                init.clone(),
                temporary.join(hls::local_name(0, &init.url)),
                task.init_segment.clone(),
                keys.clone(),
                control.clone(),
            )
            .await?,
        );
        manager.progress(task.clone(), generation).await?;
    }
    let mut jobs = JoinSet::new();
    let mut next = 0usize;
    let mut committed = Vec::new();
    loop {
        control.check()?;
        // Existing requests drain naturally when the limit is lowered. Only
        // new work uses the current value; no verified progress is discarded.
        while jobs.len() < manager.limits().max_segments_per_episode && next < plan.segments.len() {
            let source = plan.segments[next].clone();
            let path = temporary.join(hls::local_name(next + 1, &source.url));
            let record = task
                .completed_segments
                .iter()
                .find(|record| record.index == next)
                .cloned();
            jobs.spawn(segment(
                next,
                source,
                path,
                record,
                keys.clone(),
                control.clone(),
            ));
            next += 1;
        }
        let Some(result) = jobs.join_next().await else {
            break;
        };
        let record = result.map_err(|e| format!("下载操作意外结束：{e}"))??;
        committed.push(record.index);
        task.completed_segments
            .retain(|old| old.index != record.index);
        task.completed_segments.push(record);
        task.progress = committed.len() as f32 / plan.segments.len() as f32;
        manager.progress(task.clone(), generation).await?;
    }
    control.check()?;
    task.completed_segments
        .retain(|record| record.index < plan.segments.len());
    let playlist = hls::write_playlist(&plan, temporary).await?;
    let part = control.run(hls::remux(&playlist, output)).await?;
    let (length, checksum) = hls::file_hash(&part).await?;
    control.check()?;
    let receipt = Receipt {
        schema_version: 1,
        byte_len: length,
        sha256: checksum,
        task: task.clone(),
    };
    // Publish the receipt first: after a crash, a published MP4 can only be
    // accepted if its complete checksum matches this receipt.
    storage::atomic_json(receipt_path, &receipt).await?;
    if output.exists() {
        return Err("目标视频已存在，未覆盖".into());
    }
    tokio::fs::rename(&part, output)
        .await
        .map_err(|e| format!("保存 MP4 失败：{e}"))?;
    if let Some(url) = task.cover_url.as_deref() {
        match control.run(crate::media::load_image(url)).await {
            Ok((_, bytes)) => {
                if let Some(dir) = output.parent().and_then(Path::parent) {
                    if let Err(error) = storage::atomic_bytes(&dir.join("cover.img"), &bytes).await
                    {
                        eprintln!("[downloads] 封面无法写入本地目录：{error}");
                    }
                }
            }
            Err(_) => eprintln!("[downloads] 视频已保存，封面获取未完成"),
        }
    }
    // Only files registered by this run are removed; unknown files survive.
    for (index, source) in plan.segments.iter().enumerate() {
        let _ =
            tokio::fs::remove_file(temporary.join(hls::local_name(index + 1, &source.url))).await;
    }
    if let Some(init) = &plan.init {
        let _ = tokio::fs::remove_file(temporary.join(hls::local_name(0, &init.url))).await;
    }
    let _ = tokio::fs::remove_file(playlist).await;
    let _ = tokio::fs::remove_dir(temporary).await;
    Ok(())
}
