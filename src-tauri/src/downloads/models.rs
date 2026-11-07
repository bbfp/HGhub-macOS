use super::SCHEMA_VERSION;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_MAX_TASKS: usize = 5;
pub const DEFAULT_MAX_EPISODES_PER_DRAMA: usize = 3;
pub const DEFAULT_MAX_SEGMENTS_PER_EPISODE: usize = 4;
pub const MAX_MAX_TASKS: usize = 32;
pub const MAX_MAX_EPISODES_PER_DRAMA: usize = 32;
pub const MAX_MAX_SEGMENTS_PER_EPISODE: usize = 16;

fn default_max_tasks() -> usize {
    DEFAULT_MAX_TASKS
}

fn default_max_episodes_per_drama() -> usize {
    DEFAULT_MAX_EPISODES_PER_DRAMA
}

fn default_max_segments_per_episode() -> usize {
    DEFAULT_MAX_SEGMENTS_PER_EPISODE
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Cancelled,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentRecord {
    pub index: usize,
    pub uri_sha256: String,
    pub byte_len: u64,
    pub sha256: String,
    pub key_uri_sha256: Option<String>,
    pub iv: Option<String>,
    #[serde(default)]
    pub sequence: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadTask {
    pub schema_version: u32,
    pub id: String,
    pub drama_id: i64,
    pub drama_title: String,
    pub episode: u32,
    pub episode_title: String,
    pub source_url: String,
    pub root_dir: String,
    pub status: DownloadStatus,
    pub progress: f32,
    pub total_segments: usize,
    pub completed_segments: Vec<SegmentRecord>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub init_segment: Option<SegmentRecord>,
    #[serde(default)]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadSettings {
    pub schema_version: u32,
    pub root_dir: String,
    #[serde(default = "default_max_tasks")]
    pub max_tasks: usize,
    #[serde(default = "default_max_episodes_per_drama")]
    pub max_episodes_per_drama: usize,
    #[serde(default = "default_max_segments_per_episode")]
    pub max_segments_per_episode: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadLimits {
    pub max_tasks: usize,
    pub max_episodes_per_drama: usize,
    pub max_segments_per_episode: usize,
}

impl Default for DownloadLimits {
    fn default() -> Self {
        Self {
            max_tasks: DEFAULT_MAX_TASKS,
            max_episodes_per_drama: DEFAULT_MAX_EPISODES_PER_DRAMA,
            max_segments_per_episode: DEFAULT_MAX_SEGMENTS_PER_EPISODE,
        }
    }
}

impl DownloadLimits {
    pub fn validate(self) -> Result<(), String> {
        if !(1..=MAX_MAX_TASKS).contains(&self.max_tasks) {
            return Err(format!("全局同时下载任务数必须是 1-{MAX_MAX_TASKS} 的整数"));
        }
        if !(1..=MAX_MAX_EPISODES_PER_DRAMA).contains(&self.max_episodes_per_drama) {
            return Err(format!(
                "同一部剧同时下载集数必须是 1-{MAX_MAX_EPISODES_PER_DRAMA} 的整数"
            ));
        }
        if !(1..=MAX_MAX_SEGMENTS_PER_EPISODE).contains(&self.max_segments_per_episode) {
            return Err(format!(
                "每集同时下载分片数必须是 1-{MAX_MAX_SEGMENTS_PER_EPISODE} 的整数"
            ));
        }
        Ok(())
    }
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            root_dir: String::new(),
            max_tasks: DEFAULT_MAX_TASKS,
            max_episodes_per_drama: DEFAULT_MAX_EPISODES_PER_DRAMA,
            max_segments_per_episode: DEFAULT_MAX_SEGMENTS_PER_EPISODE,
        }
    }
}

impl DownloadSettings {
    pub fn new(root_dir: PathBuf) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            root_dir: root_dir.to_string_lossy().into_owned(),
            ..Self::default()
        }
    }

    pub fn limits(&self) -> DownloadLimits {
        DownloadLimits {
            max_tasks: self.max_tasks,
            max_episodes_per_drama: self.max_episodes_per_drama,
            max_segments_per_episode: self.max_segments_per_episode,
        }
    }

    pub fn set_limits(&mut self, limits: DownloadLimits) -> Result<(), String> {
        limits.validate()?;
        self.max_tasks = limits.max_tasks;
        self.max_episodes_per_drama = limits.max_episodes_per_drama;
        self.max_segments_per_episode = limits.max_segments_per_episode;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.limits().validate()
    }
}

pub fn safe_title(title: &str) -> Option<String> {
    let mut value = title
        .trim()
        .chars()
        .map(|ch| {
            if ch == '/' || ch == '\\' || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    while value.ends_with('.') || value.ends_with(' ') {
        value.pop();
    }
    if value.is_empty() || value == "." || value == ".." {
        return None;
    }
    let mut bounded = String::new();
    for ch in value.chars() {
        if bounded.len() + ch.len_utf8() > 64 {
            break;
        }
        bounded.push(ch);
    }
    let value = bounded;
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn legacy_drama_dir(root: &Path, drama_id: i64) -> PathBuf {
    root.join(format!("drama-{drama_id}"))
}

fn existing_named_drama_dir(root: &Path, drama_id: i64) -> Option<PathBuf> {
    let prefix = format!("drama-{drama_id}-");
    std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            if name.starts_with(&prefix) && entry.file_type().ok()?.is_dir() {
                Some(entry.path())
            } else {
                None
            }
        })
}

pub fn drama_dir(root: &Path, drama_id: i64, title: &str) -> PathBuf {
    if title.trim().is_empty() {
        if let Some(existing) = existing_named_drama_dir(root, drama_id) {
            return existing;
        }
    }
    match safe_title(title) {
        Some(title) => root.join(format!("drama-{drama_id}-{title}")),
        None => legacy_drama_dir(root, drama_id),
    }
}

pub fn episode_dir(root: &Path, drama_id: i64, title: &str) -> PathBuf {
    drama_dir(root, drama_id, title).join("episodes")
}

pub fn episode_path(root: &Path, drama_id: i64, title: &str, episode: u32) -> PathBuf {
    episode_dir(root, drama_id, title).join(format!("{episode:03}.mp4"))
}

pub fn legacy_episode_path(root: &Path, drama_id: i64, episode: u32) -> PathBuf {
    legacy_drama_dir(root, drama_id)
        .join("episodes")
        .join(format!("{episode:03}.mp4"))
}

/// Use the new named directory for new tasks, while keeping v1 `drama-ID`
/// files playable and trusted when they are already present.
pub fn episode_path_for_read(root: &Path, drama_id: i64, title: &str, episode: u32) -> PathBuf {
    let current = episode_path(root, drama_id, title, episode);
    let legacy = legacy_episode_path(root, drama_id, episode);
    if current.exists() || !legacy.exists() {
        current
    } else {
        legacy
    }
}

pub fn task_temp_dir(root: &Path, drama_id: i64, title: &str, episode: u32) -> PathBuf {
    drama_dir(root, drama_id, title)
        .join(".tmp")
        .join(format!("ep-{episode:03}"))
}

pub fn task_temp_dir_for_resume(root: &Path, drama_id: i64, title: &str, episode: u32) -> PathBuf {
    let current = task_temp_dir(root, drama_id, title, episode);
    let legacy = legacy_drama_dir(root, drama_id)
        .join(".tmp")
        .join(format!("ep-{episode:03}"));
    if current.exists() || !legacy.exists() {
        current
    } else {
        legacy
    }
}
