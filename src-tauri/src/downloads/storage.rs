use super::{DownloadSettings, DownloadTask, SCHEMA_VERSION};
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use tokio::io::AsyncWriteExt;

static SERIAL: AtomicU64 = AtomicU64::new(0);

pub fn now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

pub fn unique_stamp() -> String {
    format!(
        "{}-{}-{}",
        now(),
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    )
}

pub async fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    let temporary = path.with_extension(format!("{}.part", unique_stamp()));
    let result = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).await.map_err(|e| e.to_string())?;
        file.sync_all().await.map_err(|e| e.to_string())?;
        drop(file);
        tokio::fs::rename(&temporary, path)
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(temporary).await;
    }
    result
}

pub async fn atomic_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    atomic_bytes(path, &bytes).await
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

pub struct Loaded {
    pub tasks: Vec<DownloadTask>,
    pub raw: Vec<Value>,
    pub warnings: Vec<String>,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.join("downloads").join(name)
    }

    pub async fn load(&self) -> Result<Loaded, String> {
        let path = self.path("tasks.json");
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(format!("无法读取下载记录：{e}")),
        };
        let mut warnings = Vec::new();
        let raw: Vec<Value> = if bytes.is_empty() {
            Vec::new()
        } else {
            match serde_json::from_slice(&bytes) {
                Ok(raw) => raw,
                Err(error) => {
                    let backup = self.path(&format!("tasks.json.corrupt-{}", unique_stamp()));
                    tokio::fs::rename(&path, &backup)
                        .await
                        .map_err(|e| format!("下载记录损坏，保留原文件失败，已停止写入：{e}"))?;
                    warnings.push(format!(
                        "下载记录损坏，原文件已保留在 {}。视频文件未删除。原因：{error}",
                        backup.display()
                    ));
                    Vec::new()
                }
            }
        };
        let mut tasks = Vec::new();
        for entry in &raw {
            if entry.get("schema_version").and_then(Value::as_u64) != Some(SCHEMA_VERSION as u64) {
                warnings.push("有其他版本的下载记录，已保留，当前版本不修改它们。".into());
                continue;
            }
            match serde_json::from_value(entry.clone()) {
                Ok(task) => tasks.push(task),
                Err(error) => warnings.push(format!("一条下载记录无法读取，已保留原记录：{error}")),
            }
        }
        Ok(Loaded {
            tasks,
            raw,
            warnings,
        })
    }

    // The manager is the sole writer. Overlay typed fields on the raw view to
    // retain future records and extra fields when an older app saves progress.
    pub async fn save(&self, tasks: &[DownloadTask], raw: &[Value]) -> Result<(), String> {
        self.save_removed(tasks, raw, &HashSet::new()).await
    }

    /// Save after removing known v1 task records. Unknown schema records and
    /// malformed records are retained so a newer/foreign task is never deleted
    /// by this version.
    pub async fn save_removed(
        &self,
        tasks: &[DownloadTask],
        raw: &[Value],
        removed_ids: &HashSet<String>,
    ) -> Result<(), String> {
        let mut result = without_removed(raw, removed_ids);
        for task in tasks {
            let value = serde_json::to_value(task).map_err(|e| e.to_string())?;
            if let Some(old) = result.iter_mut().find(|entry| {
                entry.get("id").and_then(Value::as_str) == Some(task.id.as_str())
                    && entry.get("schema_version").and_then(Value::as_u64)
                        == Some(SCHEMA_VERSION as u64)
                    && serde_json::from_value::<DownloadTask>((*entry).clone()).is_ok()
            }) {
                if let (Some(old), Some(new)) = (old.as_object_mut(), value.as_object()) {
                    old.extend(new.clone());
                }
            } else {
                result.push(value);
            }
        }
        atomic_json(&self.path("tasks.json"), &result).await
    }

    pub async fn settings(&self, default_root: PathBuf) -> Result<DownloadSettings, String> {
        match self.existing_settings().await? {
            Some(settings) => {
                if !Path::new(&settings.root_dir).is_absolute() {
                    return Err("已保存的下载目录无效，请重新选择目录".into());
                }
                Ok(settings)
            }
            None => {
                let fresh = DownloadSettings::new(default_root);
                self.save_settings(&fresh).await?;
                Ok(fresh)
            }
        }
    }

    /// Read a previously saved settings file without creating one. Recovery
    /// uses this so a fresh test/app data directory keeps the normal default
    /// download location logic in `load_settings`.
    pub async fn existing_settings(&self) -> Result<Option<DownloadSettings>, String> {
        let path = self.path("settings.json");
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("下载并发设置无法读取：{e}")),
        };
        let settings: DownloadSettings = serde_json::from_slice(&bytes)
            .map_err(|e| format!("下载并发设置无法读取，原文件保留在 {}：{e}", path.display()))?;
        if settings.schema_version != SCHEMA_VERSION {
            return Err("下载设置来自其他版本，已保留原文件。请使用较新的版本。".into());
        }
        settings
            .validate()
            .map_err(|e| format!("下载并发设置无效，原文件保留在 {}：{e}", path.display()))?;
        Ok(Some(settings))
    }

    pub async fn save_settings(&self, settings: &DownloadSettings) -> Result<(), String> {
        settings.validate()?;
        let path = self.path("settings.json");
        let mut value = match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let value: Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("设置文件损坏，未覆盖：{e}"))?;
                if value.get("schema_version").and_then(Value::as_u64)
                    != Some(SCHEMA_VERSION as u64)
                {
                    return Err("设置来自其他版本，未覆盖".into());
                }
                value
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
            Err(e) => return Err(e.to_string()),
        };
        let object = value.as_object_mut().ok_or("设置文件格式无效，未覆盖")?;
        object.extend(
            serde_json::to_value(settings)
                .map_err(|e| e.to_string())?
                .as_object()
                .unwrap()
                .clone(),
        );
        atomic_json(&path, &value).await
    }
}

pub fn without_removed(raw: &[Value], removed_ids: &HashSet<String>) -> Vec<Value> {
    raw.iter()
        .filter(|entry| {
            let schema = entry.get("schema_version").and_then(Value::as_u64);
            let id = entry.get("id").and_then(Value::as_str);
            schema != Some(SCHEMA_VERSION as u64)
                || id.is_none_or(|id| !removed_ids.contains(id))
                || serde_json::from_value::<DownloadTask>((*entry).clone()).is_err()
        })
        .cloned()
        .collect()
}
