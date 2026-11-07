import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, localFile } from "../api";
import type { DownloadSettings, DownloadTask } from "../types";
import { useStore } from "../store";
import Player from "./Player";
import DownloadConcurrencySettings from "./DownloadConcurrencySettings";
import { IChevronDown, IChevronUp } from "../icons";

const labels: Record<DownloadTask["status"], string> = {
  queued: "等待下载",
  downloading: "下载中",
  paused: "已暂停",
  cancelled: "已取消",
  failed: "下载失败",
  completed: "已完成，可离线播放",
};

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
  const settingsRevision = useRef(0);

  const savedSettings = (saved: DownloadSettings) => {
    settingsRevision.current += 1;
    setRoot(saved.root_dir);
    setSettings(saved);
  };

  const refresh = async () => {
    const revision = settingsRevision.current;
    try {
      const [settings, items, warnings] = await Promise.all([api.downloadSettings(), api.downloadTasks(), api.downloadWarnings()]);
      if (revision === settingsRevision.current) {
        setRoot(settings.root_dir);
        setSettings(settings);
      }
      setTasks(items);
      const resolved = await Promise.all(items.map(async (task) => [task.id, await api.downloadLocalPath(task.id)] as const));
      setPaths(Object.fromEntries(resolved));
      setWarnings(warnings);
      setError("");
    } catch (reason) {
      setError(String(reason));
    }
  };

  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const update = async () => {
      const revision = settingsRevision.current;
      try {
        const [settings, items, warnings] = await Promise.all([api.downloadSettings(), api.downloadTasks(), api.downloadWarnings()]);
        const resolved = await Promise.all(items.map(async (task) => [task.id, await api.downloadLocalPath(task.id)] as const));
        if (!alive) return;
        // A slow poll started before a save must not reset the saved values.
        if (revision === settingsRevision.current) {
          setRoot(settings.root_dir); setSettings(settings);
        }
        setTasks(items); setPaths(Object.fromEntries(resolved)); setWarnings(warnings); setError("");
      } catch (reason) {
        if (alive) setError(String(reason));
      } finally {
        if (alive) timer = window.setTimeout(() => { void update(); }, 1500);
      }
    };
    void update();
    return () => { alive = false; window.clearTimeout(timer); };
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
      await refresh();
    } catch (reason) { toast(String(reason), "info"); }
    finally { setBusy(null); }
  };

  const groups = useMemo(() => {
    const byDrama = new Map<number, DownloadTask[]>();
    for (const task of tasks) byDrama.set(task.drama_id, [...(byDrama.get(task.drama_id) || []), task]);
    return [...byDrama.entries()]
      .map(([dramaId, dramaTasks]) => {
        const active = dramaTasks.some((task) => ["queued", "downloading", "paused", "failed"].includes(task.status));
        const latest = dramaTasks.reduce((value, task) => task.updated_at > value ? task.updated_at : value, "");
        return { dramaId, tasks: dramaTasks, active, latest, title: dramaTasks.find((task) => task.drama_title)?.drama_title || `短剧 ${dramaId}` };
      })
      .sort((a, b) => Number(b.active) - Number(a.active) || b.latest.localeCompare(a.latest) || a.dramaId - b.dramaId);
  }, [tasks]);

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
      {tasks.length === 0 ? <p className="text-[11px] text-mut py-4">在短剧的“选集”中选择需要下载的集数。无需登录即可下载和离线播放。</p> : (
        <div className="mt-3 space-y-2.5">
          {groups.map((group) => {
            const key = String(group.dramaId);
            const open = expanded[key] ?? group.active;
            const completed = group.tasks.filter((task) => task.status === "completed").length;
            return (
              <section key={key} className="rounded-(--r-sm) bg-s3 overflow-hidden">
                <button type="button" onClick={() => setExpanded((current) => ({ ...current, [key]: !open }))}
                  className="w-full flex items-center gap-2 p-2.5 text-left pressable">
                  <span className="text-mut">{open ? <IChevronUp size={14} /> : <IChevronDown size={14} />}</span>
                  <span className="flex-1 min-w-0">
                    <span className="block text-[12px] text-on truncate">{group.title}</span>
                    <span className="block text-[10.5px] text-mut mt-0.5">{completed}/{group.tasks.length} 集完成 · {group.active ? "有任务进行中" : "已结束"}</span>
                  </span>
                </button>
                {open && <div className="px-2.5 pb-2.5 space-y-2">
                  {group.tasks.slice().sort((a, b) => a.episode - b.episode).map((task) => (
                    <article key={task.id} className="rounded-(--r-sm) bg-s4 p-2.5">
                      <div className="text-[12px] text-on truncate">第 {task.episode} 集</div>
                      <div className="text-[10.5px] text-mut mt-1">{labels[task.status]}{task.status === "downloading" || task.status === "queued" ? ` · ${Math.round(task.progress * 100)}%` : ""}</div>
                      {task.error && task.status === "failed" && <div className="text-[10.5px] text-danger mt-1 break-all">{task.error}</div>}
                      {task.status !== "completed" && <div className="h-1 bg-s3 rounded-full mt-2 overflow-hidden"><div className="h-full bg-primary transition-[width]" style={{ width: `${Math.max(0, Math.min(100, task.progress * 100))}%` }} /></div>}
                      <div className="flex gap-4 mt-2 text-[11px] text-primary">
                        {(task.status === "downloading" || task.status === "queued") && <button disabled={busy === task.id} onClick={() => { void act(task, "pause"); }} className="pressable disabled:opacity-50">暂停</button>}
                        {task.status === "paused" && <button disabled={busy === task.id} onClick={() => { void act(task, "resume"); }} className="pressable disabled:opacity-50">继续</button>}
                        {(task.status === "failed" || task.status === "cancelled") && <button disabled={busy === task.id} onClick={() => { void act(task, "resume"); }} className="pressable disabled:opacity-50">重试</button>}
                        {task.status !== "completed" && task.status !== "cancelled" && <button disabled={busy === task.id} onClick={() => { void act(task, "cancel"); }} className="pressable disabled:opacity-50">取消</button>}
                        {task.status === "completed" && <button onClick={() => setPlaying(playing === task.id ? null : task.id)} className="pressable">{playing === task.id ? "收起播放器" : "离线播放"}</button>}
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
