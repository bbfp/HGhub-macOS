import { useCallback, useEffect, useRef, useState } from "react";
import Feed from "../components/Feed";
import { api, platform } from "../api";
import type { VideoCard } from "../types";
import { useStore } from "../store";
import { ISearch, IVol, IVolMute } from "../icons";

export default function Home({ active, bottomInset }: { active: boolean; bottomInset: number }) {
  const { muted, setMuted, setTab, toast, fs, user } = useStore();
  const [items, setItems] = useState<VideoCard[]>([]);
  const [page, setPage] = useState(1);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const loadingRef = useRef(false);

  const load = useCallback(async (p: number, force = false) => {
    if (loadingRef.current || (done && !force)) return;
    loadingRef.current = true;
    setLoading(true);
    setError(null);
    try {
      const res = await api.feed(p, 10);
      if (!Array.isArray(res?.items)) throw new Error("接口未返回视频列表，请检测或更换域名");
      const list = res.items;
      setItems((prev) => {
        const seen = new Set(prev.map((x) => x.id));
        return [...prev, ...list.filter((x) => !seen.has(x.id))];
      });
      setPage(p);
      if (!list.length || (res?.pagination && p >= res.pagination.pages)) setDone(true);
    } catch (e) {
      const message = (e as Error).message || "加载失败";
      setError(message);
      toast(message, "info");
    } finally {
      loadingRef.current = false;
      setLoading(false);
    }
  }, [done, toast]);

  useEffect(() => { if (active && !items.length) void load(1, true); /* eslint-disable-next-line */ }, [active]);

  const ids = items.map((v) => v.id);

  return (
    <div className="absolute inset-0 bg-black">
      {items.length ? (
        <Feed videoIds={ids} mode="home" active={active} autoPlay={platform !== "macos"} bottomInset={bottomInset}
          onNearEnd={() => { if (!loading && !done) load(page + 1); }} />
      ) : loading ? (
        <div className="absolute inset-0 flex items-center justify-center" role="status" aria-live="polite"><div className="w-9 h-9 spinner" /></div>
      ) : error ? (
        <div className="absolute inset-0 flex items-center justify-center px-8 text-center">
          <div className="max-w-[430px] rounded-(--r-md) bg-s2 p-5">
            <div className="text-[14px] font-semibold text-on">内容加载失败</div>
            <div className="text-[11.5px] text-mut leading-relaxed mt-2 break-words">{error}</div>
            <div className="text-[11px] text-dim leading-relaxed mt-2">可在“我的 → 网络设置”中检测或更换备用域名。</div>
            {user && <div className="text-[11px] text-mut leading-relaxed mt-2">已登录时，请把检测可用的域名移到首行后保存。</div>}
            <button type="button" onClick={() => { setDone(false); void load(1, true); }} className="mt-4 h-9 px-5 rounded-(--r-ctl) bg-primary text-on-primary text-[12px] font-semibold pressable">重试</button>
            <button type="button" onClick={() => setTab("profile")} className="mt-4 ml-3 h-9 px-4 rounded-(--r-ctl) bg-s3 text-on text-[12px] pressable">网络设置</button>
          </div>
        </div>
      ) : (
        <div className="absolute inset-0 flex items-center justify-center text-[12px] text-mut">暂时没有内容</div>
      )}

      {/* 移动端：静音 + 搜索 */}
      {platform === "android" && !fs && (
        <div className="absolute inset-x-0 z-20 flex items-center px-3 pointer-events-none" style={{ top: "calc(var(--sat, 0px) + 6px)" }}>
          <div className="ml-auto flex items-center gap-2">
            <button onClick={() => setMuted((m) => !m)}
              className="pointer-events-auto w-9 h-9 rounded-full bg-black/40 flex items-center justify-center text-white/90 pressable elev-1">
              {muted ? <IVolMute size={17} /> : <IVol size={17} />}
            </button>
            <button onClick={() => setTab("search")}
              className="pointer-events-auto w-9 h-9 rounded-full bg-black/40 flex items-center justify-center text-white/90 pressable elev-1">
              <ISearch size={18} />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
