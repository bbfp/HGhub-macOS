import { useCallback, useEffect, useRef, useState } from "react";
import { api, localFile, localImage, notifyDownloadsChanged } from "../api";
import type { LibraryDrama, LibraryUpdate, LibraryUpdateEpisode } from "../types";
import { useStore } from "../store";
import { Empty, Img } from "../components/ui";
import Player from "../components/Player";
import { IDownload, IRefresh, IChevronDown, IChevronUp, ICheck } from "../icons";

function coverSrc(item: LibraryDrama): { src: string; raw: boolean } | null {
  if (item.cover_path) return { src: localImage(item.cover_path), raw: true };
  if (item.cover_url) return { src: item.cover_url, raw: false };
  return null;
}

export default function Library() {
  const { toast, setTab } = useStore();
  const [items, setItems] = useState<LibraryDrama[]>([]);
  const [cursor, setCursor] = useState<string | null>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [updates, setUpdates] = useState<Record<number, LibraryUpdate>>({});
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [playing, setPlaying] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const checked = useRef(new Set<number>());
  const sentinel = useRef<HTMLDivElement>(null);

  const loadMore = useCallback(async () => {
    if (loading || cursor === null) return;
    setLoading(true);
    try {
      const page = await api.downloadLibraryPage(cursor || undefined, 20);
      setItems((current) => [...current, ...page.items.filter((item) => !current.some((old) => old.drama_id === item.drama_id))]);
      setCursor(page.next_cursor ?? null);
      setWarnings(page.warnings);
      setError("");
    } catch (reason) {
      setError(String(reason));
    } finally { setLoading(false); }
  }, [cursor, loading]);

  useEffect(() => { void loadMore(); }, []); // loadMore owns its cursor state

  useEffect(() => {
    const ids = items.map((item) => item.drama_id).filter((id) => !checked.current.has(id));
    if (!ids.length) return;
    ids.forEach((id) => checked.current.add(id));
    api.downloadLibraryUpdateCheck(ids).then((result) => {
      setUpdates((current) => ({ ...current, ...Object.fromEntries(result.map((item) => [item.drama_id, item])) }));
    }).catch(() => {
      ids.forEach((id) => checked.current.delete(id));
    });
  }, [items]);

  useEffect(() => {
    const element = sentinel.current;
    if (!element) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void loadMore();
    }, { rootMargin: "500px" });
    observer.observe(element);
    return () => observer.disconnect();
  }, [loadMore]);

  const checkAll = async () => {
    setBusy(true);
    try {
      const result = await api.downloadLibraryUpdateCheck();
      setUpdates(Object.fromEntries(result.map((item) => [item.drama_id, item])));
      result.forEach((item) => checked.current.add(item.drama_id));
      toast("更新检查完成", "ok");
    } catch (reason) { toast(String(reason), "info"); }
    finally { setBusy(false); }
  };

  const enqueue = async (result: LibraryUpdate, item: LibraryDrama) => {
    const episodes: LibraryUpdateEpisode[] = result.new_episodes;
    const settled = await Promise.allSettled(episodes.map(async (episode) => {
      const task = await api.downloadEnqueue({
        dramaId: item.drama_id,
        dramaTitle: item.title,
        episode: episode.episode,
        episodeTitle: episode.title || `第 ${episode.episode} 集`,
        sourceUrl: "",
        coverUrl: result.cover_url || item.cover_url || undefined,
      });
      // 每一集入队后立即通知下载页，避免等整批任务完成才刷新。
      notifyDownloadsChanged();
      return task;
    }));
    const failed = settled.filter((value) => value.status === "rejected").length;
    const addedEpisodes = episodes.filter((_, index) => settled[index]?.status === "fulfilled").map((episode) => episode.episode);
    return { count: addedEpisodes.length, failed, addedEpisodes };
  };

  const downloadUpdates = async (all: boolean) => {
    setBusy(true);
    try {
      // 使用页面上已有的检查结果，点击后不再重复请求远端更新接口。
      const available = Object.values(updates).filter((item) => item.status === "ok" && item.new_episodes.length > 0);
      const latest = all ? available : available.filter((item) => selected.has(item.drama_id));
      const byId = new Map(items.map((item) => [item.drama_id, item]));
      const doneEpisodes = new Map<number, Set<number>>();
      if (latest.length > 0) setTab("downloads");
      const settled = await Promise.allSettled(latest.map(async (update) => {
        const item = byId.get(update.drama_id) || {
          drama_id: update.drama_id,
          title: update.title,
          cover_url: update.cover_url,
          cover_path: null,
          episodes: [],
          updated_at: update.checked_at,
        };
        return { update, result: await enqueue(update, item) };
      }));
      let added = 0; let failed = 0;
      for (const result of settled) {
        if (result.status === "rejected") { failed += 1; continue; }
        added += result.value.result.count;
        failed += result.value.result.failed;
        if (result.value.result.addedEpisodes.length) doneEpisodes.set(result.value.update.drama_id, new Set(result.value.result.addedEpisodes));
      }
      if (doneEpisodes.size) {
        setUpdates((current) => Object.fromEntries(Object.entries(current).map(([id, update]) => {
          const done = doneEpisodes.get(Number(id));
          return done ? [id, { ...update, new_episodes: update.new_episodes.filter((episode) => !done.has(episode.episode)) }] : [id, update];
        })));
      }
      if (added > 0) notifyDownloadsChanged();
      toast(failed ? `已加入 ${added} 集，${failed} 集失败` : `已加入 ${added} 集更新`, failed ? "info" : "ok");
      setSelected(new Set());
    } catch (reason) { toast(String(reason), "info"); }
    finally { setBusy(false); }
  };

  const availableUpdates = Object.values(updates).filter((item) => item.status === "ok" && item.new_episodes.length > 0);
  const updateDramaCount = availableUpdates.length;
  const updateEpisodeCount = availableUpdates.reduce((sum, item) => sum + item.new_episodes.length, 0);
  const selectedUpdates = availableUpdates.filter((item) => selected.has(item.drama_id));
  const selectedDramaCount = selectedUpdates.length;
  const selectedEpisodeCount = selectedUpdates.reduce((sum, item) => sum + item.new_episodes.length, 0);

  return (
    <div className="absolute inset-0 bg-bg flex flex-col">
      <div className="shrink-0 px-4 pb-2" style={{ paddingTop: "calc(var(--sat, 0px) + 14px)" }}>
        <div className="text-[24px] font-bold text-on leading-none">我下载的</div>
        <div className="text-[11px] text-mut mt-1.5">本地视频库 · 按短剧归类</div>
      </div>
      <div className="shrink-0 flex items-center gap-2 px-4 pb-3">
        <button type="button" onClick={() => { void checkAll(); }} disabled={busy} className="flex items-center gap-1 h-8 px-3 rounded-(--r-ctl) bg-s3 text-[11px] text-on pressable disabled:opacity-50"><IRefresh size={13} />检查更新</button>
        <button type="button" onClick={() => { void downloadUpdates(true); }} disabled={busy || updateDramaCount === 0} className="flex items-center gap-1 h-8 px-3 rounded-(--r-ctl) bg-primary text-[11px] text-on-primary font-semibold pressable disabled:opacity-50"><IDownload size={13} />下载全部更新{updateDramaCount ? ` · ${updateDramaCount} 部（${updateEpisodeCount} 集）` : ""}</button>
        {selectedDramaCount > 0 && <button type="button" onClick={() => { void downloadUpdates(false); }} disabled={busy} className="h-8 px-3 rounded-(--r-ctl) bg-s3 text-[11px] text-primary pressable disabled:opacity-50">下载所选 · {selectedDramaCount} 部（{selectedEpisodeCount} 集）</button>}
      </div>
      <div className="flex-1 scroll-y px-4 pb-24">
        {error && <div role="alert" className="mb-3 text-[11px] text-danger">{error}</div>}
        {warnings.map((warning) => <div key={warning} className="mb-2 text-[10.5px] text-mut break-all">{warning}</div>)}
        {!items.length && !loading ? <Empty icon={<IDownload size={24} />} title="还没有本地短剧" sub="下载完成后，短剧会出现在这里" /> : (
          <div className="space-y-3">
            {items.map((item) => {
              const update = updates[item.drama_id];
              const open = expanded.has(item.drama_id);
              const image = coverSrc(item);
              return (
                <article key={item.drama_id} className="rounded-(--r-md) bg-s2 overflow-hidden">
                  <div className="flex gap-3 p-3">
                    <div className="relative w-14 h-20 shrink-0 rounded-(--r-sm) overflow-hidden bg-s3">
                      {image ? <Img src={image.src} raw={image.raw} alt={item.title} /> : <div className="w-full h-full flex items-center justify-center text-dim text-[10px]">无封面</div>}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-start gap-2">
                        <button type="button" onClick={() => setExpanded((current) => { const next = new Set(current); if (next.has(item.drama_id)) next.delete(item.drama_id); else next.add(item.drama_id); return next; })} className="flex-1 text-left pressable">
                          <div className="text-[13px] font-semibold text-on truncate">{item.title}</div>
                          <div className="text-[10.5px] text-mut mt-1">已下载 {item.episodes.length} 集</div>
                        </button>
                        {open ? <IChevronUp size={15} className="text-mut" /> : <IChevronDown size={15} className="text-mut" />}
                      </div>
                      <div className="flex items-center gap-2 mt-2 text-[10.5px]">
                        {!update && <span className="text-mut">检测中…</span>}
                        {update?.status === "unknown" && <span className="text-mut">未检测</span>}
                        {update?.status === "ok" && !update.new_episodes.length && <span className="text-mint">已是最新</span>}
                        {update?.status === "ok" && update.new_episodes.length > 0 && <>
                          <label className="flex items-center gap-1 text-primary"><input type="checkbox" checked={selected.has(item.drama_id)} onChange={(event) => setSelected((current) => { const next = new Set(current); if (event.target.checked) next.add(item.drama_id); else next.delete(item.drama_id); return next; })} />更新 {update.new_episodes.length} 集</label>
                        </>}
                      </div>
                    </div>
                  </div>
                  {open && <div className="border-t border-white/5 px-3 py-2 space-y-1.5">
                    {item.episodes.map((episode) => (
                      <div key={episode.episode}>
                        <div className="flex items-center gap-2 rounded-(--r-sm) bg-s3 px-2.5 py-2">
                        <span className="flex-1 text-[11px] text-on">第 {episode.episode} 集</span>
                        <button type="button" onClick={() => setPlaying(playing === episode.task_id ? null : episode.task_id)} className="text-[10.5px] text-primary pressable">{playing === episode.task_id ? "收起" : "播放"}</button>
                        <ICheck size={13} className="text-mint" />
                        </div>
                        {playing === episode.task_id && <Player src={localFile(episode.path)} controls className="mt-2 h-48 rounded-(--r-sm)" />}
                      </div>
                    ))}
                  </div>}
                </article>
              );
            })}
            <div ref={sentinel} className="h-10 flex items-center justify-center text-[10.5px] text-dim">{loading ? "加载中…" : cursor === null ? "— 到底啦 —" : "继续下滑加载"}</div>
          </div>
        )}
      </div>
    </div>
  );
}
