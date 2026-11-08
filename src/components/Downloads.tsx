import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, DOWNLOADS_CHANGED_EVENT, localFile, notifyDownloadsChanged } from "../api";
import type { DownloadSettings, DownloadTask } from "../types";
import { isTaskDeletable, matchesDownloadFilter, summarizeDownloads, type DownloadFilter } from "../downloadsSummary";
import { useStore } from "../store";
import Player from "./Player";
import DownloadConcurrencySettings from "./DownloadConcurrencySettings";
import { Img } from "./ui";
import { IChevronDown, IChevronUp, IFileText, ITrash } from "../icons";

const labels: Record<DownloadTask["status"], string> = {
  queued: "等待下载",
  downloading: "下载中",
  paused: "已暂停",
  cancelled: "已取消，可重试",
  failed: "下载失败，可重试",
  completed: "已完成，可离线播放",
};

const filterLabels: { key: DownloadFilter; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "downloading", label: "下载中" },
  { key: "queued", label: "等待" },
  { key: "paused", label: "暂停" },
  { key: "failed", label: "失败/重试" },
];

function statusCounts(tasks: DownloadTask[]) {
  return summarizeDownloads(tasks);
}

export default function Downloads() {
  const { toast } = useStore();
  const [root, setRoot] = useState("");
  const [settings, setSettings] = useState<DownloadSettings | null>(null);
  const [tasks, setTasks] = useState<DownloadTask[]>([]);
  const [error, setError] = useState("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [choosing, setChoosing] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);
  const [paths, setPaths] = useState<Record<string, string>>({});
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [filter, setFilter] = useState<DownloadFilter>("all");
  const settingsRevision = useRef(0);
  const tasksRevision = useRef(0);

  const savedSettings = (saved: DownloadSettings) => {
    settingsRevision.current += 1;
    setRoot(saved.root_dir);
    setSettings(saved);
  };

  const refresh = async () => {
    const taskRevision = ++tasksRevision.current;
    const revision = settingsRevision.current;
    try {
      const [nextSettings, items, nextWarnings] = await Promise.all([
        api.downloadSettings(), api.downloadTasks(), api.downloadWarnings(),
      ]);
      const resolved = await Promise.all(items.map(async (task) => [task.id, await api.downloadLocalPath(task.id)] as const));
      if (taskRevision !== tasksRevision.current) return;
      if (revision === settingsRevision.current) {
        setRoot(nextSettings.root_dir);
        setSettings(nextSettings);
      }
      setTasks(items);
      setPaths(Object.fromEntries(resolved));
      setWarnings(nextWarnings);
      setError("");
    } catch (reason) {
      if (taskRevision === tasksRevision.current) setError(String(reason));
    }
  };

  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const update = async () => {
      await refresh();
      if (alive) timer = window.setTimeout(() => { void update(); }, 1500);
    };
    const onChanged = () => { void refresh(); };
    window.addEventListener(DOWNLOADS_CHANGED_EVENT, onChanged);
    void update();
    return () => {
      alive = false;
      window.clearTimeout(timer);
      window.removeEventListener(DOWNLOADS_CHANGED_EVENT, onChanged);
    };
  }, []);

  const chooseRoot = async () => {
    setChoosing(true);
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected !== "string") return;
      const saved = await api.setDownloadRoot(selected);
      savedSettings(saved);
      toast("后续下载将使用这个默认地址", "ok");
    } catch (reason) {
      toast(String(reason), "info");
    } finally { setChoosing(false); }
  };

  const act = async (task: DownloadTask, action: "pause" | "resume" | "cancel") => {
    setBusy(task.id);
    try {
      if (action === "pause") await api.downloadPause(task.id);
      if (action === "resume") await api.downloadResume(task.id);
      if (action === "cancel") await api.downloadCancel(task.id);
      notifyDownloadsChanged();
      await refresh();
    } catch (reason) { toast(String(reason), "info"); }
    finally { setBusy(null); }
  };

  const deleteTasks = async (ids: string[], bulk = false) => {
    if (!ids.length) return;
    if (bulk && !window.confirm(`确定删除 ${ids.length} 条失败/取消记录吗？已完成视频不会删除。`)) return;
    setBusy(bulk ? "bulk-delete" : ids[0]);
    try {
      const count = await api.downloadDelete(ids);
      notifyDownloadsChanged();
      await refresh();
      toast(`已删除 ${count} 条下载记录`, "ok");
    } catch (reason) { toast(String(reason), "info"); }
    finally { setBusy(null); }
  };

  const reveal = async (task: DownloadTask) => {
    setBusy(task.id);
    try {
      await api.downloadReveal(task.id);
      toast("已在 Finder 中定位视频文件", "ok");
    } catch (reason) { toast(String(reason), "info"); }
    finally { setBusy(null); }
  };

  const filteredTasks = useMemo(() => tasks.filter((task) => matchesDownloadFilter(task.status, filter)), [tasks, filter]);

  const groups = useMemo(() => {
    const byDrama = new Map<number, DownloadTask[]>();
    for (const task of filteredTasks) byDrama.set(task.drama_id, [...(byDrama.get(task.drama_id) || []), task]);
    return [...byDrama.entries()]
      .map(([dramaId, dramaTasks]) => ({
        dramaId,
        tasks: dramaTasks,
        counts: statusCounts(dramaTasks),
        active: dramaTasks.some((task) => ["queued", "downloading", "paused", "failed", "cancelled"].includes(task.status)),
        title: dramaTasks.find((task) => task.drama_title)?.drama_title || `短剧 ${dramaId}`,
        coverUrl: dramaTasks.find((task) => task.cover_url)?.cover_url || null,
      }))
      .sort((a, b) => a.dramaId - b.dramaId);
  }, [filteredTasks]);

  useEffect(() => {
    setExpanded((current) => {
      const next = { ...current };
      for (const group of groups) {
        const key = String(group.dramaId);
        if (next[key] === undefined) next[key] = group.active;
      }
      return next;
    });
  }, [groups]);

  const overview = useMemo(() => summarizeDownloads(tasks), [tasks]);
  const deletableIds = useMemo(() => tasks.filter((task) => isTaskDeletable(task.status)).map((task) => task.id), [tasks]);

  return (
    <section className="mt-5 rounded-(--r-md) bg-s2 p-3.5" aria-label="本地下载">
      <div className="flex items-center gap-2">
        <h2 className="text-[13px] font-semibold text-on flex-1">本地下载</h2>
        <button type="button" onClick={() => { void refresh(); }} className="text-[11px] text-primary pressable">刷新</button>
      </div>
      <div className="mt-3 flex items-center gap-3 rounded-(--r-sm) bg-s3 p-2.5">
        <div className="flex-1 min-w-0">
          <div className="text-[11.5px] text-on">下载地址</div>
          <div className="text-[10.5px] text-mut truncate mt-0.5" title={root}>{root || "正在读取…"}</div>
        </div>
        <button type="button" onClick={chooseRoot} disabled={choosing} className="text-[11px] text-primary pressable disabled:opacity-50">{choosing ? "选择中…" : "更改"}</button>
      </div>
      {settings && <DownloadConcurrencySettings settings={settings} onSaved={savedSettings} />}
      {error && <div role="alert" className="text-[11px] text-danger mt-2">{error}</div>}
      {warnings.map((warning, index) => <div key={index} role="status" className="text-[11px] text-mut mt-2 break-all">{warning}</div>)}
      <div className="mt-3 rounded-(--r-sm) bg-s3 p-2.5" aria-label="下载总览">
        <div className="flex items-center gap-2">
          <span className="text-[11.5px] font-semibold text-on flex-1">下载总览</span>
          <span className="text-[10.5px] text-mut">{overview.unfinished_dramas} 部未完成短剧 · {overview.unfinished_tasks} 集未完成 · {overview.dramas} 部有记录</span>
        </div>
        <div className="grid grid-cols-2 sm:grid-cols-5 gap-1.5 mt-2 text-[10px]">
          {filterLabels.map(({ key, label }) => {
            const count = key === "all" ? overview.unfinished_tasks : key === "downloading" ? overview.downloading : key === "queued" ? overview.queued : key === "paused" ? overview.paused : overview.failed;
            const selected = filter === key;
            return <button key={key} type="button" aria-pressed={selected}
              onClick={() => setFilter(key === "all" || selected ? "all" : key)}
              className={`rounded-(--r-sm) px-2 py-1.5 text-left pressable transition-colors ${selected ? "bg-primary text-on-primary" : "bg-s4 text-on/70 hover:text-on"}`}>
              {label} <b>{count}</b>
            </button>;
          })}
        </div>
        {deletableIds.length > 0 && <button type="button" disabled={busy === "bulk-delete"} onClick={() => { void deleteTasks(deletableIds, true); }} className="mt-2 flex items-center gap-1 text-[10.5px] text-danger pressable disabled:opacity-50"><ITrash size={12} />一键删除失败/取消记录 ({deletableIds.length})</button>}
      </div>
      {tasks.length === 0 ? <p className="text-[11px] text-mut py-4">在短剧的“选集”中选择需要下载的集数。无需登录即可下载和离线播放。</p> : groups.length === 0 ? <p className="text-[11px] text-mut py-4">当前筛选没有匹配的任务。</p> : (
        <div className="mt-3 space-y-2.5">
          {groups.map((group) => {
            const key = String(group.dramaId);
            const isOpen = expanded[key] === true;
            return (
              <section key={key} className="rounded-(--r-sm) bg-s3 overflow-hidden">
                <button type="button" aria-expanded={isOpen} onClick={() => setExpanded((current) => ({ ...current, [key]: !isOpen }))}
                  className="w-full flex items-center gap-2 p-2.5 text-left pressable">
                  <div className="relative w-11 h-14 shrink-0 rounded-(--r-sm) overflow-hidden bg-s4">
                    {group.coverUrl ? <Img src={group.coverUrl} eager alt={group.title} /> : <div className="w-full h-full flex items-center justify-center text-dim text-[9px]">无封面</div>}
                  </div>
                  <span className="flex-1 min-w-0">
                    <span className="block text-[12px] text-on truncate">{group.title}</span>
                    <span className="block text-[10.5px] text-mut mt-0.5">共 {group.tasks.length} 个任务 · 已完成 {group.counts.completed}</span>
                    <span className="block text-[10px] text-mut mt-1">下载中 {group.counts.downloading} · 等待 {group.counts.queued} · 暂停 {group.counts.paused} · 失败/重试 {group.counts.failed}</span>
                  </span>
                  <span className="text-mut pointer-events-none">{isOpen ? <IChevronUp size={15} /> : <IChevronDown size={15} />}</span>
                </button>
                {isOpen && <div className="px-2.5 pb-2.5 space-y-2">
                  {group.tasks.slice().sort((a, b) => a.episode - b.episode).map((task) => (
                    <article key={task.id} className="rounded-(--r-sm) bg-s4 p-2.5">
                      <div className="text-[12px] text-on truncate">第 {task.episode} 集</div>
                      <div className="text-[10.5px] text-mut mt-1">{labels[task.status]}{task.status === "downloading" || task.status === "queued" ? ` · ${Math.round(task.progress * 100)}%` : ""}</div>
                      {task.error && task.status === "failed" && <div className="text-[10.5px] text-danger mt-1 break-all">{task.error}</div>}
                      {task.status !== "completed" && <div className="h-1 bg-s3 rounded-full mt-2 overflow-hidden"><div className="h-full bg-primary transition-[width]" style={{ width: `${Math.max(0, Math.min(100, task.progress * 100))}%` }} /></div>}
                      <div className="flex flex-wrap gap-4 mt-2 text-[11px] text-primary">
                        {(task.status === "downloading" || task.status === "queued") && <button disabled={busy === task.id} onClick={() => { void act(task, "pause"); }} className="pressable disabled:opacity-50">暂停</button>}
                        {task.status === "paused" && <button disabled={busy === task.id} onClick={() => { void act(task, "resume"); }} className="pressable disabled:opacity-50">继续</button>}
                        {(task.status === "failed" || task.status === "cancelled") && <button disabled={busy === task.id} onClick={() => { void act(task, "resume"); }} className="pressable disabled:opacity-50">重试</button>}
                        {task.status !== "completed" && task.status !== "cancelled" && <button disabled={busy === task.id} onClick={() => { void act(task, "cancel"); }} className="pressable disabled:opacity-50">取消</button>}
                        {isTaskDeletable(task.status) && <button disabled={busy === task.id} onClick={() => { void deleteTasks([task.id]); }} className="flex items-center gap-1 text-danger pressable disabled:opacity-50"><ITrash size={12} />删除</button>}
                        {task.status === "completed" && <button onClick={() => setPlaying(playing === task.id ? null : task.id)} className="pressable">{playing === task.id ? "收起播放器" : "离线播放"}</button>}
                        {task.status === "completed" && <button disabled={busy === task.id} onClick={() => { void reveal(task); }} className="flex items-center gap-1 pressable disabled:opacity-50"><IFileText size={12} />打开所在文件夹</button>}
                      </div>
                      {task.status === "completed" && playing === task.id && paths[task.id] && <Player src={localFile(paths[task.id])} controls className="mt-2 h-64 rounded-(--r-sm)" />}
                    </article>
                  ))}
                </div>}
              </section>
            );
          })}
        </div>
      )}
    </section>
  );
}
