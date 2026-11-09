use super::{
    index::{self, DownloadMarker, RemoteCheck},
    storage::{self, Store},
    worker, DownloadLimits, DownloadSettings, DownloadStatus, DownloadTask,
};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, OnceLock, RwLock},
};
use tauri::AppHandle;
use tokio::sync::{mpsc, oneshot, watch};

type Reply<T> = oneshot::Sender<Result<T, String>>;
pub(super) type Resolver = Arc<
    dyn Fn(i64, u32) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> + Send + Sync,
>;

#[derive(Clone)]
pub struct DownloadManager {
    sender: Arc<OnceLock<mpsc::UnboundedSender<Message>>>,
    limits: Arc<RwLock<DownloadLimits>>,
    resolver: Resolver,
    initialization_error: Arc<OnceLock<String>>,
}

pub(super) struct Control {
    stop: watch::Sender<u8>,
}
impl Control {
    fn new() -> Self {
        Self {
            stop: watch::channel(0).0,
        }
    }
    pub fn check(&self) -> Result<(), String> {
        match *self.stop.borrow() {
            0 => Ok(()),
            1 => Err("下载已暂停".into()),
            _ => Err("下载已取消".into()),
        }
    }
    pub async fn run<T>(
        &self,
        future: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        let mut receiver = self.stop.subscribe();
        self.check()?;
        tokio::select! {
            result = future => result,
            _ = receiver.changed() => { self.check()?; Err("下载操作已停止".into()) }
        }
    }
}

enum Action {
    Pause,
    Resume,
    Cancel,
}
enum SettingsUpdate {
    Root(PathBuf),
    Limits(DownloadLimits),
}
enum Message {
    Enqueue(DownloadTask, Reply<DownloadTask>),
    Change(String, Action, Reply<()>),
    Delete(Vec<String>, Reply<usize>),
    Tasks(Reply<Vec<DownloadTask>>),
    Warnings(Reply<Vec<String>>),
    LibraryStatus(Vec<i64>, Reply<Vec<DownloadMarker>>),
    RemoteChecks(Vec<RemoteCheck>, Reply<()>),
    Progress(DownloadTask, u64, Reply<()>),
    Finished(String, u64, Result<(), String>),
    Settings(PathBuf, Option<SettingsUpdate>, Reply<DownloadSettings>),
    #[cfg(test)]
    Shutdown(Reply<()>),
}
struct Running {
    drama: i64,
    generation: u64,
    control: Arc<Control>,
    handle: tokio::task::JoinHandle<()>,
}

impl DownloadManager {
    pub fn new(max_tasks: usize) -> Self {
        Self::with_resolver(
            max_tasks,
            Arc::new(|drama, episode| {
                Box::pin(async move {
                    let token = crate::network::session_token();
                    let value =
                        crate::net::fetch_playback(drama, episode, token.as_deref()).await?;
                    crate::net::playback_url(&value).ok_or_else(|| "未获取到这集的播放地址".into())
                })
            }),
        )
    }
    pub(super) fn with_resolver(max_tasks: usize, resolver: Resolver) -> Self {
        Self::with_resolver_and_limits(
            DownloadLimits {
                max_tasks: max_tasks.max(1),
                ..DownloadLimits::default()
            },
            resolver,
        )
    }
    pub(super) fn with_resolver_and_limits(limits: DownloadLimits, resolver: Resolver) -> Self {
        debug_assert!(limits.validate().is_ok());
        Self {
            sender: Arc::new(OnceLock::new()),
            limits: Arc::new(RwLock::new(limits)),
            resolver,
            initialization_error: Arc::new(OnceLock::new()),
        }
    }
    pub async fn recover(&self, app: AppHandle) -> Result<(), String> {
        let result = match crate::data_dir(&app) {
            Ok(dir) => self.initialize(dir).await,
            Err(error) => Err(error),
        };
        if let Err(error) = &result {
            let _ = self.initialization_error.set(error.clone());
        }
        result
    }
    pub(super) async fn initialize(&self, dir: PathBuf) -> Result<(), String> {
        if self.sender.get().is_some() {
            return Ok(());
        }
        let store = Store::new(dir.clone());
        let mut loaded = store.load().await?;
        let (library_index, index_warnings) = index::load_or_rebuild(&dir, &loaded.tasks).await?;
        loaded.warnings.extend(index_warnings);
        match store.existing_settings().await {
            Ok(Some(settings)) => {
                // Settings are read before workers are spawned. A later UI
                // update only changes this shared snapshot and wakes the
                // scheduler; existing workers are never stopped.
                self.apply_limits(settings.limits());
            }
            Ok(None) => {}
            Err(error) => loaded
                .warnings
                .push(format!("下载设置无法读取，已有任务暂用默认并发；请先修复设置文件，再新增任务或保存设置：{error}")),
        }
        for task in &mut loaded.tasks {
            if matches!(
                task.status,
                DownloadStatus::Queued | DownloadStatus::Downloading
            ) {
                task.status = DownloadStatus::Queued;
                task.error = None;
            }
        }
        store.save(&loaded.tasks, &loaded.raw).await?;
        let (sender, receiver) = mpsc::unbounded_channel();
        self.sender
            .set(sender)
            .map_err(|_| "下载服务已启动".to_string())?;
        tokio::spawn(event_loop(
            self.clone(),
            store,
            loaded,
            library_index,
            dir,
            receiver,
        ));
        Ok(())
    }
    fn send(&self, message: Message) -> Result<(), String> {
        self.sender
            .get()
            .ok_or_else(|| {
                self.initialization_error
                    .get()
                    .cloned()
                    .unwrap_or_else(|| "下载服务正在启动，请稍后重试".into())
            })?
            .send(message)
            .map_err(|_| "下载服务已停止".into())
    }
    async fn request<T>(&self, create: impl FnOnce(Reply<T>) -> Message) -> Result<T, String> {
        let (sender, receiver) = oneshot::channel();
        self.send(create(sender))?;
        receiver
            .await
            .map_err(|_| "下载服务未能完成操作".to_string())?
    }
    pub async fn tasks(&self) -> Result<Vec<DownloadTask>, String> {
        self.request(Message::Tasks).await
    }
    pub async fn warnings(&self) -> Result<Vec<String>, String> {
        self.request(Message::Warnings).await
    }
    pub async fn library_status(&self, drama_ids: Vec<i64>) -> Result<Vec<DownloadMarker>, String> {
        self.request(|reply| Message::LibraryStatus(drama_ids, reply))
            .await
    }
    pub async fn record_remote_checks(&self, checks: Vec<RemoteCheck>) -> Result<(), String> {
        self.request(|reply| Message::RemoteChecks(checks, reply))
            .await
    }
    pub async fn enqueue(&self, task: DownloadTask) -> Result<DownloadTask, String> {
        self.request(|reply| Message::Enqueue(task, reply)).await
    }
    pub async fn pause(&self, id: &str) -> Result<(), String> {
        self.request(|reply| Message::Change(id.into(), Action::Pause, reply))
            .await
    }
    pub async fn resume(&self, id: &str) -> Result<(), String> {
        self.request(|reply| Message::Change(id.into(), Action::Resume, reply))
            .await
    }
    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        self.request(|reply| Message::Change(id.into(), Action::Cancel, reply))
            .await
    }
    pub async fn delete(&self, ids: Vec<String>) -> Result<usize, String> {
        self.request(|reply| Message::Delete(ids, reply)).await
    }
    pub fn limits(&self) -> DownloadLimits {
        *self
            .limits
            .read()
            .expect("download limits lock should not be poisoned")
    }
    fn apply_limits(&self, limits: DownloadLimits) {
        *self
            .limits
            .write()
            .expect("download limits lock should not be poisoned") = limits;
    }
    pub async fn settings(&self, default_root: PathBuf) -> Result<DownloadSettings, String> {
        self.request(|reply| Message::Settings(default_root, None, reply))
            .await
    }
    pub async fn set_root(
        &self,
        default_root: PathBuf,
        root: PathBuf,
    ) -> Result<DownloadSettings, String> {
        self.request(|reply| {
            Message::Settings(default_root, Some(SettingsUpdate::Root(root)), reply)
        })
        .await
    }
    pub async fn set_limits(
        &self,
        default_root: PathBuf,
        limits: DownloadLimits,
    ) -> Result<DownloadSettings, String> {
        limits.validate()?;
        self.request(|reply| {
            Message::Settings(default_root, Some(SettingsUpdate::Limits(limits)), reply)
        })
        .await
    }
    pub(super) async fn progress(&self, task: DownloadTask, generation: u64) -> Result<(), String> {
        self.request(|reply| Message::Progress(task, generation, reply))
            .await
    }
    #[cfg(test)]
    pub(super) async fn shutdown(&self) -> Result<(), String> {
        self.request(Message::Shutdown).await
    }
}

// Pick a drama after the last one, wrapping around; then its first queued
// episode. Per-drama capacity is checked before spawning a worker; the event
// loop also enforces global capacity, including workers stopping after pause.
pub(super) fn next_task(
    tasks: &[DownloadTask],
    active: &[(String, i64)],
    last: Option<i64>,
    per_drama_limit: usize,
) -> Option<usize> {
    let mut dramas: Vec<i64> = tasks
        .iter()
        .filter(|task| {
            task.status == DownloadStatus::Queued
                && active
                    .iter()
                    .filter(|(_, drama)| *drama == task.drama_id)
                    .count()
                    < per_drama_limit
                && !active.iter().any(|(id, _)| id == &task.id)
        })
        .map(|task| task.drama_id)
        .collect();
    dramas.sort_unstable();
    dramas.dedup();
    let drama = dramas
        .iter()
        .find(|&&drama| last.is_none_or(|last| drama > last))
        .or_else(|| dramas.first())?;
    tasks.iter().position(|task| {
        task.drama_id == *drama
            && task.status == DownloadStatus::Queued
            && !active.iter().any(|(id, _)| id == &task.id)
    })
}

async fn event_loop(
    manager: DownloadManager,
    store: Store,
    mut loaded: storage::Loaded,
    mut library_index: index::LibraryIndex,
    data_dir: PathBuf,
    mut receiver: mpsc::UnboundedReceiver<Message>,
) {
    let mut active = HashMap::<String, Running>::new();
    let mut last_drama = None;
    let mut generation = 0u64;
    let mut storage_failed = false;
    loop {
        if !storage_failed {
            let limits = manager.limits();
            while active.len() < limits.max_tasks {
                let snapshot: Vec<_> = active
                    .iter()
                    .map(|(id, run)| (id.clone(), run.drama))
                    .collect();
                let Some(index) = next_task(
                    &loaded.tasks,
                    &snapshot,
                    last_drama,
                    limits.max_episodes_per_drama,
                ) else {
                    break;
                };
                loaded.tasks[index].status = DownloadStatus::Downloading;
                loaded.tasks[index].updated_at = storage::now();
                if let Err(error) = store.save(&loaded.tasks, &loaded.raw).await {
                    loaded
                        .warnings
                        .push(format!("无法保存任务，已停止启动新下载：{error}"));
                    storage_failed = true;
                    break;
                }
                generation += 1;
                let run_generation = generation;
                let task = loaded.tasks[index].clone();
                last_drama = Some(task.drama_id);
                let control = Arc::new(Control::new());
                let run_control = control.clone();
                let handle_manager = manager.clone();
                let run_task = task.clone();
                let handle = tokio::spawn(async move {
                    let result =
                        worker::download(&handle_manager, run_task, run_generation, run_control)
                            .await;
                    let _ = handle_manager.send(Message::Finished(task.id, run_generation, result));
                });
                active.insert(
                    loaded.tasks[index].id.clone(),
                    Running {
                        drama: loaded.tasks[index].drama_id,
                        generation,
                        control,
                        handle,
                    },
                );
            }
        }
        let Some(message) = receiver.recv().await else {
            break;
        };
        match message {
            Message::Tasks(reply) => {
                let _ = reply.send(Ok(loaded.tasks.clone()));
            }
            Message::Warnings(reply) => {
                let _ = reply.send(Ok(loaded.warnings.clone()));
            }
            Message::LibraryStatus(ids, reply) => {
                let _ = reply.send(Ok(library_index.statuses(&ids)));
            }
            Message::RemoteChecks(checks, reply) => {
                library_index.apply_remote_checks(&checks);
                let result = index::save(&data_dir, &library_index).await;
                if result.is_err() {
                    storage_failed = true;
                }
                let _ = reply.send(result);
            }
            Message::Enqueue(mut incoming, reply) => {
                if storage_failed {
                    let _ = reply.send(Err(
                        "下载记录无法保存，请检查磁盘空间和目录权限后重启".into()
                    ));
                    continue;
                }
                let mut index_changed = false;
                let position = loaded.tasks.iter().position(|old| {
                    old.drama_id == incoming.drama_id
                        && old.episode == incoming.episode
                        && old.root_dir == incoming.root_dir
                });
                if let Some(index) = position {
                    let old = &loaded.tasks[index];
                    let missing_output = if old.status == DownloadStatus::Completed {
                        let output = super::episode_path_for_read(
                            std::path::Path::new(&old.root_dir),
                            old.drama_id,
                            &old.drama_title,
                            old.episode,
                        );
                        match tokio::fs::metadata(output).await {
                            Ok(_) => false,
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                            Err(error) => {
                                let _ = reply
                                    .send(Err(format!("无法检查已有视频，已保留记录：{error}")));
                                continue;
                            }
                        }
                    } else {
                        false
                    };
                    if !missing_output
                        && !matches!(
                            old.status,
                            DownloadStatus::Failed | DownloadStatus::Cancelled
                        )
                    {
                        let _ = reply.send(Ok(old.clone()));
                        continue;
                    }
                    let mut next = old.clone();
                    if !incoming.source_url.is_empty() {
                        next.source_url = incoming.source_url;
                    }
                    next.status = DownloadStatus::Queued;
                    next.error = None;
                    if missing_output {
                        next.progress = 0.0;
                    }
                    next.updated_at = storage::now();
                    incoming = next;
                    loaded.tasks[index] = incoming.clone();
                    if missing_output {
                        library_index.remove_episode(incoming.drama_id, incoming.episode);
                        index_changed = true;
                    }
                } else {
                    loaded.tasks.push(incoming.clone());
                }
                let result = store
                    .save(&loaded.tasks, &loaded.raw)
                    .await
                    .map(|_| incoming);
                if result.is_err() {
                    storage_failed = true;
                }
                if index_changed {
                    if let Err(error) = index::save(&data_dir, &library_index).await {
                        loaded
                            .warnings
                            .push(format!("保存本地下载索引失败：{error}"));
                    }
                }
                let _ = reply.send(result);
            }
            Message::Change(id, action, reply) => {
                let Some(task) = loaded.tasks.iter_mut().find(|task| task.id == id) else {
                    let _ = reply.send(Err("下载任务不存在".into()));
                    continue;
                };
                if task.status == DownloadStatus::Completed {
                    let _ = reply.send(Ok(()));
                    continue;
                }
                match action {
                    Action::Pause => {
                        if matches!(
                            task.status,
                            DownloadStatus::Queued | DownloadStatus::Downloading
                        ) {
                            task.status = DownloadStatus::Paused;
                            if let Some(run) = active.get(&id) {
                                run.control.stop.send_replace(1);
                            }
                        }
                    }
                    Action::Cancel => {
                        task.status = DownloadStatus::Cancelled;
                        if let Some(run) = active.get(&id) {
                            run.control.stop.send_replace(2);
                        }
                    }
                    Action::Resume => {
                        // Keep the old stop request until its worker has returned.
                        // Queued + active id is excluded by next_task.
                        if !matches!(
                            task.status,
                            DownloadStatus::Queued | DownloadStatus::Downloading
                        ) {
                            task.status = DownloadStatus::Queued;
                            task.error = None;
                        }
                    }
                }
                task.updated_at = storage::now();
                let result = store.save(&loaded.tasks, &loaded.raw).await;
                if result.is_err() {
                    storage_failed = true;
                }
                let _ = reply.send(result);
            }
            Message::Delete(ids, reply) => {
                if storage_failed {
                    let _ = reply.send(Err(
                        "下载记录无法保存，请检查磁盘空间和目录权限后重启".into()
                    ));
                    continue;
                }
                let unique_ids: Vec<_> = ids
                    .into_iter()
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
                let mut targets = Vec::with_capacity(unique_ids.len());
                let mut removed_ids = HashSet::new();
                let validation = (|| {
                    for id in &unique_ids {
                        let Some(task) = loaded.tasks.iter().find(|task| task.id == *id) else {
                            return Err(format!("下载任务不存在：{id}"));
                        };
                        if active.contains_key(id) {
                            return Err(format!("任务仍在停止中，请稍后再删除：{id}"));
                        }
                        if !matches!(
                            task.status,
                            DownloadStatus::Failed | DownloadStatus::Cancelled
                        ) {
                            return Err(format!("只能删除失败或取消任务：{id}"));
                        }
                        let output = super::episode_path_for_read(
                            std::path::Path::new(&task.root_dir),
                            task.drama_id,
                            &task.drama_title,
                            task.episode,
                        );
                        if output.exists() || output.with_extension("json").exists() {
                            return Err(format!(
                                "这条任务已有本地文件，为避免误删请保留记录：{}",
                                output.display()
                            ));
                        }
                        let temporary = super::task_temp_dir_for_resume(
                            std::path::Path::new(&task.root_dir),
                            task.drama_id,
                            &task.drama_title,
                            task.episode,
                        );
                        let root = std::path::Path::new(&task.root_dir);
                        if !temporary.starts_with(root)
                            || temporary.parent().and_then(std::path::Path::file_name)
                                != Some(std::ffi::OsStr::new(".tmp"))
                        {
                            return Err(format!("下载临时目录不安全，未删除任务：{id}"));
                        }
                        targets.push((task.clone(), temporary));
                        removed_ids.insert(id.clone());
                    }
                    Ok(())
                })();
                if let Err(error) = validation {
                    let _ = reply.send(Err(error));
                    continue;
                }
                let retained: Vec<_> = loaded
                    .tasks
                    .iter()
                    .filter(|task| !removed_ids.contains(&task.id))
                    .cloned()
                    .collect();
                let result = store
                    .save_removed(&retained, &loaded.raw, &removed_ids)
                    .await;
                match result {
                    Ok(()) => {
                        loaded.tasks = retained;
                        loaded.raw = storage::without_removed(&loaded.raw, &removed_ids);
                        for (task, temporary) in targets {
                            if let Err(error) = tokio::fs::remove_dir_all(&temporary).await {
                                if error.kind() != std::io::ErrorKind::NotFound {
                                    loaded.warnings.push(format!(
                                        "任务已删除，但临时文件清理失败（{}）：{}",
                                        task.id, error
                                    ));
                                }
                            }
                        }
                        let _ = reply.send(Ok(removed_ids.len()));
                    }
                    Err(error) => {
                        storage_failed = true;
                        let _ = reply.send(Err(error));
                    }
                }
            }
            Message::Progress(progress, run_generation, reply) => {
                if active
                    .get(&progress.id)
                    .is_none_or(|run| run.generation != run_generation)
                {
                    let _ = reply.send(Err("旧的下载操作已结束".into()));
                    continue;
                }
                if let Some(task) = loaded.tasks.iter_mut().find(|task| task.id == progress.id) {
                    task.source_url = progress.source_url;
                    task.total_segments = progress.total_segments;
                    task.completed_segments = progress.completed_segments;
                    task.init_segment = progress.init_segment;
                    task.progress = progress.progress;
                    task.updated_at = storage::now();
                }
                let result = store.save(&loaded.tasks, &loaded.raw).await;
                if result.is_err() {
                    storage_failed = true;
                }
                let _ = reply.send(result);
            }
            Message::Finished(id, run_generation, result) => {
                if active
                    .get(&id)
                    .is_none_or(|run| run.generation != run_generation)
                {
                    continue;
                }
                active.remove(&id);
                let mut completed_task = None;
                if let Some(task) = loaded.tasks.iter_mut().find(|task| task.id == id) {
                    match result {
                        // A published and verified MP4 is complete. A late
                        // pause/cancel during optional cover fetch must not
                        // hide a finished episode from the offline library.
                        Ok(()) => {
                            task.status = DownloadStatus::Completed;
                            task.progress = 1.0;
                            task.error = None;
                            completed_task = Some(task.clone());
                        }
                        Err(error) => {
                            if task.status == DownloadStatus::Downloading {
                                task.status = DownloadStatus::Failed;
                                task.error = Some(error);
                            }
                        }
                    }
                    task.updated_at = storage::now();
                }
                if let Err(error) = store.save(&loaded.tasks, &loaded.raw).await {
                    loaded.warnings.push(format!("保存下载结果失败：{error}"));
                    storage_failed = true;
                }
                if let Some(task) = completed_task {
                    let output = super::episode_path_for_read(
                        std::path::Path::new(&task.root_dir),
                        task.drama_id,
                        &task.drama_title,
                        task.episode,
                    );
                    library_index.mark_completed(&task, super::library::cover_for(&output));
                    if let Err(error) = index::save(&data_dir, &library_index).await {
                        loaded
                            .warnings
                            .push(format!("保存本地下载索引失败：{error}"));
                    }
                }
            }
            Message::Settings(default_root, update, reply) => {
                // Keep directory and concurrency updates in the same writer
                // queue, so concurrent saves cannot overwrite each other.
                let result = async {
                    let mut settings = store.settings(default_root).await?;
                    if let Some(update) = update {
                        match update {
                            SettingsUpdate::Root(root) => {
                                settings.root_dir = root.to_string_lossy().into_owned();
                            }
                            SettingsUpdate::Limits(limits) => settings.set_limits(limits)?,
                        }
                        store.save_settings(&settings).await?;
                    }
                    manager.apply_limits(settings.limits());
                    Ok(settings)
                }
                .await;
                let _ = reply.send(result);
            }
            #[cfg(test)]
            Message::Shutdown(reply) => {
                for (_, run) in active.drain() {
                    run.handle.abort();
                    let _ = run.handle.await;
                }
                let _ = reply.send(Ok(()));
                break;
            }
        }
    }
    for (_, run) in active {
        run.handle.abort();
    }
}

impl DownloadManager {
    pub(super) async fn resolve(&self, drama: i64, episode: u32) -> Result<String, String> {
        (self.resolver)(drama, episode).await
    }
}
