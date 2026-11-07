//! Derived local-library views. This module never writes tasks or settings.

use super::{episode_path_for_read, DownloadTask};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryEpisode {
    pub episode: u32,
    pub title: String,
    pub task_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryDrama {
    pub drama_id: i64,
    pub title: String,
    pub cover_path: Option<String>,
    pub cover_url: Option<String>,
    pub episodes: Vec<LibraryEpisode>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryPage {
    pub items: Vec<LibraryDrama>,
    pub next_cursor: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryUpdateEpisode {
    pub episode: u32,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryUpdate {
    pub drama_id: i64,
    pub title: String,
    pub status: String,
    pub remote_episode_count: usize,
    pub new_episodes: Vec<LibraryUpdateEpisode>,
    pub cover_url: Option<String>,
    pub checked_at: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReceiptMeta {
    schema_version: u32,
    byte_len: u64,
    sha256: String,
}

#[derive(Default)]
struct DramaBuilder {
    title: String,
    cover_path: Option<String>,
    cover_url: Option<String>,
    updated_at: String,
    episodes: BTreeMap<u32, LibraryEpisode>,
}

async fn receipt_length_matches(path: &Path) -> bool {
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    let Ok(bytes) = tokio::fs::read(path.with_extension("json")).await else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<ReceiptMeta>(&bytes) else {
        return false;
    };
    // Indexing must stay cheap for a large library. The receipt's full SHA is
    // still used by download recovery; the library scan uses the size match
    // as its fast candidate check and the media route keeps the task allowlist.
    receipt.schema_version == 1 && !receipt.sha256.is_empty() && receipt.byte_len == metadata.len()
}

fn cover_for(output: &Path) -> Option<String> {
    let path = output.parent()?.parent()?.join("cover.img");
    std::fs::metadata(&path)
        .ok()
        .filter(|metadata| metadata.is_file() && metadata.len() > 0)
        .map(|_| path.to_string_lossy().into_owned())
}

/// Build a stable page from task snapshots and verified receipt metadata.
/// The page is derived only; it never mutates the manager or the filesystem.
pub async fn build_page(
    tasks: &[DownloadTask],
    current_root: &Path,
    cursor: usize,
    limit: usize,
) -> Result<LibraryPage, String> {
    if limit == 0 || limit > 50 {
        return Err("本地库分页大小必须是 1-50".into());
    }
    let current_root = tokio::fs::canonicalize(current_root)
        .await
        .unwrap_or_else(|_| current_root.to_path_buf());
    let mut warnings = Vec::new();
    let mut roots = HashMap::<String, bool>::new();
    let mut ordered = tasks.to_vec();
    ordered.sort_by_key(|task| {
        let root = PathBuf::from(&task.root_dir);
        let priority = if root == current_root { 0 } else { 1 };
        (priority, std::cmp::Reverse(task.updated_at.clone()))
    });
    let mut groups = BTreeMap::<i64, DramaBuilder>::new();
    for task in ordered {
        let root = Path::new(&task.root_dir);
        if !root.is_absolute() {
            continue;
        }
        let root_key = root.to_string_lossy().into_owned();
        if roots.insert(root_key.clone(), true).is_none()
            && tokio::fs::metadata(root).await.is_err()
        {
            warnings.push(format!("历史下载目录无法访问：{root_key}"));
        }
        let output = episode_path_for_read(root, task.drama_id, &task.drama_title, task.episode);
        if !receipt_length_matches(&output).await {
            continue;
        }
        let group = groups.entry(task.drama_id).or_default();
        if group.title.is_empty() && !task.drama_title.trim().is_empty() {
            group.title = task.drama_title.clone();
        }
        if task.updated_at > group.updated_at {
            group.updated_at = task.updated_at.clone();
        }
        if group.cover_url.is_none() {
            group.cover_url = task.cover_url.clone();
        }
        if group.cover_path.is_none() {
            group.cover_path = cover_for(&output);
        }
        group
            .episodes
            .entry(task.episode)
            .or_insert_with(|| LibraryEpisode {
                episode: task.episode,
                title: task.episode_title.clone(),
                task_id: task.id.clone(),
                path: output.to_string_lossy().into_owned(),
            });
    }
    let mut items: Vec<_> = groups
        .into_iter()
        .filter_map(|(drama_id, group)| {
            if group.episodes.is_empty() {
                return None;
            }
            Some(LibraryDrama {
                drama_id,
                title: if group.title.is_empty() {
                    format!("短剧 {drama_id}")
                } else {
                    group.title
                },
                cover_path: group.cover_path,
                cover_url: group.cover_url,
                episodes: group.episodes.into_values().collect(),
                updated_at: group.updated_at,
            })
        })
        .collect();
    items.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.drama_id.cmp(&right.drama_id))
    });
    let end = cursor.saturating_add(limit).min(items.len());
    let page_items = if cursor >= items.len() {
        Vec::new()
    } else {
        items[cursor..end].to_vec()
    };
    Ok(LibraryPage {
        items: page_items,
        next_cursor: (end < items.len()).then(|| end.to_string()),
        warnings,
    })
}

pub fn new_episode_numbers(remote: &[u32], local: &[u32]) -> Vec<u32> {
    let max_local = local.iter().copied().max().unwrap_or(0);
    let mut result: Vec<_> = remote
        .iter()
        .copied()
        .filter(|episode| *episode > max_local)
        .collect();
    result.sort_unstable();
    result.dedup();
    result
}

pub fn parse_remote_episodes(value: &Value) -> Vec<(u32, String)> {
    let Some(items) = value.get("episodes").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut result = items
        .iter()
        .filter_map(|item| {
            let episode = item.get("ep_num").and_then(Value::as_u64)? as u32;
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some((episode, title))
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|(episode, _)| *episode);
    result.dedup_by_key(|(episode, _)| *episode);
    result
}

#[cfg(test)]
mod tests {
    use super::parse_remote_episodes;
    use serde_json::json;

    #[test]
    fn remote_episode_parser_uses_real_numbers_and_deduplicates() {
        let parsed = parse_remote_episodes(&json!({
            "episodes": [
                {"ep_num": 2, "title": "二"},
                {"ep_num": 1, "title": "一"},
                {"ep_num": 2, "title": "重复"}
            ]
        }));
        assert_eq!(parsed, vec![(1, "一".into()), (2, "二".into())]);
    }
}
