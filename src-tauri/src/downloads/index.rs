//! Fast local download index used by list pages and episode pickers.
//!
//! The index is derived from verified completed files and is kept separate
//! from the task journal so the existing v1 task format remains compatible.

use super::{episode_path_for_read, library, storage, DownloadStatus, DownloadTask};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};

pub const INDEX_SCHEMA_VERSION: u32 = 1;
const INDEX_FILE: &str = "library-index.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DramaRecord {
    pub drama_id: i64,
    pub title: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub cover_path: Option<String>,
    pub cover_url: Option<String>,
    #[serde(default)]
    pub downloaded_episodes: BTreeSet<u32>,
    #[serde(default)]
    pub has_downloaded: bool,
    pub last_downloaded_at: Option<String>,
    pub remote_checked_at: Option<String>,
    pub remote_episode_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryIndex {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub dramas: HashMap<i64, DramaRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadMarker {
    pub drama_id: i64,
    pub downloaded: bool,
    pub episodes: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteCheck {
    pub drama_id: i64,
    pub title: String,
    pub keywords: Vec<String>,
    pub cover_url: Option<String>,
    pub checked_at: String,
    pub remote_episode_count: u32,
}

fn default_schema_version() -> u32 {
    INDEX_SCHEMA_VERSION
}

impl Default for LibraryIndex {
    fn default() -> Self {
        Self {
            schema_version: INDEX_SCHEMA_VERSION,
            dramas: HashMap::new(),
        }
    }
}

impl LibraryIndex {
    pub async fn rebuild(tasks: &[DownloadTask]) -> Self {
        let mut index = Self::default();
        for task in tasks {
            if task.status != DownloadStatus::Completed {
                continue;
            }
            let output = episode_path_for_read(
                Path::new(&task.root_dir),
                task.drama_id,
                &task.drama_title,
                task.episode,
            );
            if !library::verified_file(&output).await {
                continue;
            }
            index.mark_completed(task, library::cover_for(&output));
        }
        index
    }

    pub async fn reconcile_missing(&mut self, tasks: &[DownloadTask]) -> bool {
        let mut changed = false;
        for task in tasks {
            if task.status != DownloadStatus::Completed
                || self
                    .dramas
                    .get(&task.drama_id)
                    .is_some_and(|record| record.downloaded_episodes.contains(&task.episode))
            {
                continue;
            }
            let output = episode_path_for_read(
                Path::new(&task.root_dir),
                task.drama_id,
                &task.drama_title,
                task.episode,
            );
            if library::verified_file(&output).await {
                self.mark_completed(task, library::cover_for(&output));
                changed = true;
            }
        }
        changed
    }

    pub fn mark_completed(&mut self, task: &DownloadTask, cover_path: Option<String>) {
        let record = self
            .dramas
            .entry(task.drama_id)
            .or_insert_with(|| DramaRecord {
                drama_id: task.drama_id,
                title: task.drama_title.clone(),
                keywords: Vec::new(),
                cover_path: None,
                cover_url: task.cover_url.clone(),
                downloaded_episodes: BTreeSet::new(),
                has_downloaded: false,
                last_downloaded_at: None,
                remote_checked_at: None,
                remote_episode_count: None,
            });
        if record.title.trim().is_empty() && !task.drama_title.trim().is_empty() {
            record.title = task.drama_title.clone();
        }
        if record.cover_url.is_none() {
            record.cover_url = task.cover_url.clone();
        }
        if let Some(path) = cover_path {
            record.cover_path.get_or_insert(path);
        }
        record.downloaded_episodes.insert(task.episode);
        record.has_downloaded = !record.downloaded_episodes.is_empty();
        if record
            .last_downloaded_at
            .as_ref()
            .is_none_or(|old| old < &task.updated_at)
        {
            record.last_downloaded_at = Some(task.updated_at.clone());
        }
    }

    pub fn remove_episode(&mut self, drama_id: i64, episode: u32) {
        let should_remove = if let Some(record) = self.dramas.get_mut(&drama_id) {
            record.downloaded_episodes.remove(&episode);
            record.has_downloaded = !record.downloaded_episodes.is_empty();
            !record.has_downloaded && record.remote_checked_at.is_none()
        } else {
            false
        };
        if should_remove {
            self.dramas.remove(&drama_id);
        }
    }

    pub fn apply_remote_checks(&mut self, checks: &[RemoteCheck]) {
        for check in checks {
            let record = self
                .dramas
                .entry(check.drama_id)
                .or_insert_with(|| DramaRecord {
                    drama_id: check.drama_id,
                    title: check.title.clone(),
                    keywords: Vec::new(),
                    cover_path: None,
                    cover_url: check.cover_url.clone(),
                    downloaded_episodes: BTreeSet::new(),
                    has_downloaded: false,
                    last_downloaded_at: None,
                    remote_checked_at: None,
                    remote_episode_count: None,
                });
            if !check.title.trim().is_empty() {
                record.title = check.title.clone();
            }
            if !check.keywords.is_empty() {
                record.keywords = check.keywords.clone();
            }
            if check.cover_url.is_some() {
                record.cover_url = check.cover_url.clone();
            }
            record.remote_checked_at = Some(check.checked_at.clone());
            record.remote_episode_count = Some(check.remote_episode_count);
        }
    }

    pub fn statuses(&self, ids: &[i64]) -> Vec<DownloadMarker> {
        ids.iter()
            .map(|drama_id| {
                let Some(record) = self.dramas.get(drama_id) else {
                    return DownloadMarker {
                        drama_id: *drama_id,
                        downloaded: false,
                        episodes: Vec::new(),
                    };
                };
                DownloadMarker {
                    drama_id: *drama_id,
                    downloaded: !record.downloaded_episodes.is_empty(),
                    episodes: record.downloaded_episodes.iter().copied().collect(),
                }
            })
            .collect()
    }
}

pub fn index_path(data_dir: &Path) -> PathBuf {
    data_dir.join("downloads").join(INDEX_FILE)
}

pub async fn save(data_dir: &Path, index: &LibraryIndex) -> Result<(), String> {
    storage::atomic_json(&index_path(data_dir), index).await
}

pub async fn load_or_rebuild(
    data_dir: &Path,
    tasks: &[DownloadTask],
) -> Result<(LibraryIndex, Vec<String>), String> {
    let path = index_path(data_dir);
    let mut warnings = Vec::new();
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let index = LibraryIndex::rebuild(tasks).await;
            if let Err(error) = save(data_dir, &index).await {
                warnings.push(format!(
                    "本地下载索引无法保存，将在本次运行中使用内存索引：{error}"
                ));
            }
            return Ok((index, warnings));
        }
        Err(error) => return Err(format!("无法读取本地下载索引：{error}")),
    };

    let parsed = serde_json::from_slice::<LibraryIndex>(&bytes);
    let mut can_save = true;
    let mut needs_save = false;
    let mut index = match parsed {
        Ok(index) if index.schema_version == INDEX_SCHEMA_VERSION => index,
        Ok(_) => {
            warnings.push("本地下载索引来自其他版本，当前版本不覆盖原文件。".into());
            can_save = false;
            LibraryIndex::rebuild(tasks).await
        }
        Err(error) => {
            needs_save = true;
            let backup = path.with_file_name(format!(
                "library-index.json.corrupt-{}",
                storage::unique_stamp()
            ));
            tokio::fs::rename(&path, &backup)
                .await
                .map_err(|rename_error| format!("下载索引损坏且无法保留原文件：{rename_error}"))?;
            warnings.push(format!(
                "本地下载索引损坏，原文件已保留在 {}。原因：{error}",
                backup.display()
            ));
            LibraryIndex::rebuild(tasks).await
        }
    };
    index.schema_version = INDEX_SCHEMA_VERSION;
    if can_save && (needs_save || index.reconcile_missing(tasks).await) {
        if let Err(error) = save(data_dir, &index).await {
            warnings.push(format!(
                "本地下载索引补齐失败，将在本次运行中使用内存索引：{error}"
            ));
        }
    }
    Ok((index, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed_task(id: i64, episode: u32, updated_at: &str) -> DownloadTask {
        DownloadTask {
            schema_version: 1,
            id: format!("{id}-{episode}"),
            drama_id: id,
            drama_title: format!("剧 {id}"),
            episode,
            episode_title: format!("第 {episode} 集"),
            source_url: String::new(),
            root_dir: "/tmp/hghub-index".into(),
            status: DownloadStatus::Completed,
            progress: 1.0,
            total_segments: 1,
            completed_segments: Vec::new(),
            error: None,
            created_at: updated_at.into(),
            updated_at: updated_at.into(),
            init_segment: None,
            cover_url: Some("https://example.test/cover".into()),
        }
    }

    #[test]
    fn status_lookup_is_batch_ordered_and_empty_for_unknown_ids() {
        let mut index = LibraryIndex::default();
        index.mark_completed(&completed_task(7, 3, "20"), None);
        index.mark_completed(&completed_task(7, 1, "10"), None);
        assert_eq!(
            index.statuses(&[9, 7]),
            vec![
                DownloadMarker {
                    drama_id: 9,
                    downloaded: false,
                    episodes: vec![]
                },
                DownloadMarker {
                    drama_id: 7,
                    downloaded: true,
                    episodes: vec![1, 3]
                }
            ]
        );
    }

    #[test]
    fn remote_checks_update_metadata_without_erasing_downloads() {
        let mut index = LibraryIndex::default();
        index.mark_completed(&completed_task(7, 3, "20"), None);
        index.apply_remote_checks(&[RemoteCheck {
            drama_id: 7,
            title: "新剧名".into(),
            keywords: vec!["古风".into()],
            cover_url: None,
            checked_at: "30".into(),
            remote_episode_count: 9,
        }]);
        let record = index.dramas.get(&7).unwrap();
        assert_eq!(record.title, "新剧名");
        assert_eq!(
            record
                .downloaded_episodes
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(record.remote_episode_count, Some(9));
        assert_eq!(record.keywords, vec!["古风"]);
    }

    #[test]
    fn index_round_trip_keeps_schema_and_defaults() {
        let mut index = LibraryIndex::default();
        index.mark_completed(&completed_task(1, 1, "1"), None);
        let value = serde_json::to_value(&index).unwrap();
        let decoded: LibraryIndex = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.schema_version, INDEX_SCHEMA_VERSION);
        assert_eq!(decoded.statuses(&[1])[0].episodes, vec![1]);
    }
}
