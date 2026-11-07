import { invoke } from "@tauri-apps/api/core";
import { detectPlatform, mediaBase } from "./platform";
import type {
  CommentPage, Detail, DownloadSettings, DownloadTask, NetworkSettingsView, NetworkTestResult, Paged, PlayInfo, Post, RankItem, User, VideoCard,
} from "./types";

/* ── 运行环境 ───────────────────────────────────────────── */

/** 应用版本（构建时由 vite.config.ts 从 package.json 注入） */
export const APP_VERSION = __APP_VERSION__;

export const isAndroid = /Android/i.test(navigator.userAgent);
export const platform = detectPlatform(navigator.userAgent);

export const LOCAL = mediaBase(platform);
let activeSiteOrigin = "https://huangguoai.com";
export const currentSiteOrigin = () => activeSiteOrigin;
export const siteUrl = (path = "") => `${activeSiteOrigin}${path.startsWith("/") ? path : `/${path}`}`;

/** 封面：经本地协议解密（站点封面是 AES 密文）。 */
export const cover = (url?: string, _w = 480) =>
  url ? `${LOCAL}/img?u=${encodeURIComponent(url)}` : "";

/** 播放列表：经本地协议改写分片地址，规避 CORS 与防盗链。 */
export const playlist = (url: string) => `${LOCAL}/hls?u=${encodeURIComponent(url)}`;
export const localFile = (path: string) => `${LOCAL}/file?u=${encodeURIComponent(path)}`;

/* ── 调用封装 ───────────────────────────────────────────── */

/** 写一行到应用数据目录 debug.log（用户反馈「打不开」时要的就是它） */
export const logLine = (msg: string) => invoke("debug_log", { msg }).catch(() => {});

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return (await invoke(cmd, args)) as T;
  } catch (e) {
    const msg = typeof e === "string" ? e : "请求失败，请检查网络";
    // 落盘完整错误链（dns error / connection refused / 证书错误…），toast 只显示截断版
    logLine(`[api] ${cmd} → ${msg}`);
    throw new Error(msg.length > 140 ? `${msg.slice(0, 140)}…` : msg);
  }
}

/* ── 接口 ───────────────────────────────────────────────── */

export const api = {
  boot: async () => {
    const result = await call<{ user: User | null; logged_in: boolean; base: string; active_base: string }>("app_boot");
    if (result.active_base) activeSiteOrigin = result.active_base;
    return result;
  },
  login: (username: string, password: string) =>
    call<{ user: User }>("api_login", { username, password }),
  register: (username: string, password: string, email = "") =>
    call<{ user: User }>("api_register", { username, password, email }),
  logout: () => call<{ ok: boolean }>("api_logout"),
  me: () => call<User>("api_me"),

  feed: (page = 1, size = 20) => call<Paged<VideoCard>>("api_feed", { page, size }),
  category: (cate: string, page = 1, size = 20) =>
    call<Paged<VideoCard>>("api_category", { cate, page, size }),
  rank: (kind: "hot" | "recommend" | "potential") =>
    call<{ type: string; items: RankItem[] }>("api_rank", { kind }),
  search: (q: string, page = 1, size = 20) =>
    call<{ items: VideoCard[]; pagination: Paged<VideoCard>["pagination"]; keyword: string }>(
      "api_search", { q, page, size },
    ),
  posts: (page = 1, size = 20) => call<Paged<Post>>("api_posts", { page, size }),
  post: (id: number) => call<Post & { content?: string; video_url?: string }>("api_post", { id }),

  video: (id: number) => call<Detail>("api_video", { id }),
  play: (id: number, ep: number) => call<PlayInfo>("api_play", { id, ep }),

  comments: (id: number, ep: number, page = 1, size = 20) =>
    call<CommentPage>("api_comments", { id, ep, page, size }),
  commentSend: (id: number, ep: number, text: string, replyId?: number) =>
    call<unknown>("api_comment_send", { id, ep, text, replyId: replyId ?? null }),

  clearCache: () => invoke<void>("cache_clear"),
  downloadSettings: () => invoke<DownloadSettings>("download_settings"),
  setDownloadRoot: (rootDir: string) => invoke<DownloadSettings>("download_settings_set", { rootDir }),
  setDownloadLimits: (limits: { maxTasks: number; maxEpisodesPerDrama: number; maxSegmentsPerEpisode: number }) =>
    invoke<DownloadSettings>("download_limits_set", limits),
  downloadLocalPath: (id: string) => invoke<string>("download_local_path", { id }),
  downloadTasks: () => invoke<DownloadTask[]>("download_tasks"),
  downloadWarnings: () => invoke<string[]>("download_warnings"),
  downloadEnqueue: (args: { dramaId: number; dramaTitle: string; episode: number; episodeTitle: string; sourceUrl: string; rootDir?: string; coverUrl?: string }) =>
    invoke<DownloadTask>("download_enqueue", {
      dramaId: args.dramaId,
      dramaTitle: args.dramaTitle,
      episode: args.episode,
      episodeTitle: args.episodeTitle,
      sourceUrl: args.sourceUrl,
      rootDir: args.rootDir ?? null,
      coverUrl: args.coverUrl ?? null,
    }),
  downloadPause: (id: string) => invoke<void>("download_pause", { id }),
  downloadResume: (id: string) => invoke<void>("download_resume", { id }),
  downloadCancel: (id: string) => invoke<void>("download_cancel", { id }),
  netDiag: () => invoke<Record<string, unknown>>("net_diag"),
  networkSettings: async () => {
    const result = await invoke<NetworkSettingsView>("network_settings");
    if (result.active_domain) activeSiteOrigin = result.active_domain;
    return result;
  },
  setNetworkDomains: async (domains: string[]) => {
    const result = await invoke<NetworkSettingsView>("network_settings_set", { domains });
    if (result.active_domain) activeSiteOrigin = result.active_domain;
    return result;
  },
  testNetworkDomains: (domains: string[]) => invoke<NetworkTestResult>("network_settings_test", { domains }),
  postLike: (id: number, on: boolean) => invoke<unknown>("api_post_like", { id, on }),
  postCollect: (id: number, on: boolean) => invoke<unknown>("api_post_collect", { id, on }),
  postComments: (id: number, page: number, size: number) => call<CommentPage>("api_post_comments", { id, page, size }),
  like: (id: number, ep: number, on: boolean) => call<unknown>("api_like", { id, ep, on }),
  collect: (id: number, on: boolean) => call<unknown>("api_collect", { id, on }),
  follow: (uid: number, on: boolean) => call<unknown>("api_follow", { uid, on }),
};

/* ── 自检：验证 webview → 本地协议 → 上游 的取流链路 ───────── */
export async function selfTest(): Promise<string> {
  const steps: string[] = [];
  try {
    const info = await api.play(117, 3);
    steps.push(`play:${info?.video_url ? "ok" : "empty"}`);
    const hlsUrl = `${LOCAL}/hls?u=${encodeURIComponent(info.video_url)}`;
    const r1 = await fetch(hlsUrl);
    const txt = await r1.text();
    steps.push(`hls:${r1.status}/${txt.length}/${txt.startsWith("#EXTM3U") ? "m3u8" : "bad"}`);
    const keyLine = txt.split("\n").find((l) => l.startsWith("#EXT-X-KEY"));
    if (keyLine) {
      const m = keyLine.match(/URI="([^"]+)"/);
      if (m) {
        const rustUrl = m[1];
        steps.push(`rustUrl:${rustUrl.slice(0, 60)}`);
        // A/B：用 JS 重新编码同一条上游地址
        const inner = decodeURIComponent(rustUrl.split("?u=")[1] || "");
        steps.push(`inner:${inner.slice(0, 50)}`);
        try {
          const rk = await fetch(rustUrl);
          const bk = await rk.arrayBuffer();
          steps.push(`keyRust:${rk.status}/${bk.byteLength}`);
        } catch (e) { steps.push(`keyRust:throw:${(e as Error).message.slice(0, 30)}`); }
        try {
          const rk2 = await fetch(`${LOCAL}/seg?u=${encodeURIComponent(inner)}`);
          const bk2 = await rk2.arrayBuffer();
          steps.push(`keyJs:${rk2.status}/${bk2.byteLength}`);
        } catch (e) { steps.push(`keyJs:throw:${(e as Error).message.slice(0, 30)}`); }
      }
    }
    const segLine = txt.split("\n").find((l) => l && !l.startsWith("#") && l.includes("/seg?u="));
    if (segLine) {
      try {
        const r2 = await fetch(segLine.trim());
        const buf = await r2.arrayBuffer();
        steps.push(`seg:${r2.status}/${buf.byteLength}`);
      } catch (e) { steps.push(`seg:throw:${(e as Error).message.slice(0, 40)}`); }
    } else steps.push("seg:no-line");
  } catch (e) {
    steps.push(`err:${(e as Error).message.slice(0, 60)}`);
  }
  return steps.join(" | ");
}

/* ── 格式化 ─────────────────────────────────────────────── */

export const fmt = (n: number | undefined) => {
  const v = Number(n) || 0;
  if (v >= 100_000_000) return `${(v / 100_000_000).toFixed(1).replace(/\.0$/, "")}亿`;
  if (v >= 10_000) return `${(v / 10_000).toFixed(1).replace(/\.0$/, "")}万`;
  return String(v);
};

export const fmtTime = (s: number) => {
  if (!isFinite(s) || s < 0) s = 0;
  const m = Math.floor(s / 60);
  const ss = Math.floor(s % 60);
  return `${m}:${ss.toString().padStart(2, "0")}`;
};

export const dur = (s?: number | string) => {
  const n = Number(s) || 0;
  if (n <= 0) return "";
  const m = Math.floor(n / 60);
  const ss = n % 60;
  return `${m}:${String(ss).padStart(2, "0")}`;
};

/** 真实解码探针：用 hls.js 挂一个隐藏 video，回报 readyState / 尺寸 / 进度。 */
export async function mediaPlaybackProbe(): Promise<string> {
  const Hls = (await import("hls.js")).default;
  const info = await api.play(117, 3);
  const src = playlist(info.video_url);
  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.style.cssText = "position:fixed;left:-10px;top:-10px;width:2px;height:2px;opacity:0;pointer-events:none";
  document.body.appendChild(video);
  return await new Promise<string>((resolve) => {
    let done = false;
    const finish = (note: string) => {
      if (done) return;
      done = true;
      const out = `${note} rs=${video.readyState} w=${video.videoWidth} h=${video.videoHeight} t=${video.currentTime.toFixed(2)} err=${video.error?.code ?? 0}`;
      try { hls.destroy(); } catch { /* ignore */ }
      video.remove();
      resolve(out);
    };
    const hls = new Hls({ enableWorker: false });
    hls.on(Hls.Events.ERROR, (_e, d) => { if (d.fatal) finish(`fatal:${d.details}`); });
    hls.loadSource(src);
    hls.attachMedia(video);
    video.play().catch(() => {});
    window.setTimeout(() => finish("timeout"), 9000);
  });
}
/** 非 m3u8 媒体（mp4 等）走 /seg 原样透传 */
export function directMedia(url: string): string {
  return `${LOCAL}/seg?u=${encodeURIComponent(url)}`;
}

/** 按扩展名选择代理方式：m3u8 → 播放列表改写；其它 → 原样透传 */
export function mediaUrl(url: string): string {
  return /\.m3u8(\?|$)/i.test(url) ? playlist(url) : directMedia(url);
}
