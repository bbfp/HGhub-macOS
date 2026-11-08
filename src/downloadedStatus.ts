import { useEffect, useMemo, useState } from "react";
import { api, DOWNLOADS_CHANGED_EVENT, platform } from "./api";
import type { DownloadStatusSummary } from "./types";

export type DownloadedStatusMap = Record<number, DownloadStatusSummary>;

/**
 * Ask the local index only about IDs currently rendered by a page. The Tauri
 * command performs one batch IPC call; the Rust side answers from its in-memory
 * HashMap rather than scanning download files.
 */
export function useDownloadedStatus(ids: readonly number[]): DownloadedStatusMap {
  const signature = Array.from(new Set(ids.filter((id) => Number.isInteger(id) && id > 0)))
    .sort((left, right) => left - right)
    .join(",");
  const requested = useMemo(
    () => (signature ? signature.split(",").map((id) => Number(id)) : []),
    [signature],
  );
  const [status, setStatus] = useState<DownloadedStatusMap>({});

  useEffect(() => {
    if (platform !== "macos" || !requested.length) {
      setStatus({});
      return undefined;
    }
    let alive = true;
    const load = async () => {
      try {
        const result = await api.downloadStatus(requested);
        if (!alive) return;
        setStatus(Object.fromEntries(result.map((item) => [item.drama_id, item])));
      } catch {
        // A transient local-service error leaves the last visible state alone.
      }
    };
    void load();
    const onChanged = () => { void load(); };
    window.addEventListener(DOWNLOADS_CHANGED_EVENT, onChanged);
    const timer = window.setInterval(() => { void load(); }, 2000);
    return () => {
      alive = false;
      window.clearInterval(timer);
      window.removeEventListener(DOWNLOADS_CHANGED_EVENT, onChanged);
    };
  }, [requested, signature]);

  return status;
}
