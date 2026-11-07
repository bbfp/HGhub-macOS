import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import type { DownloadSettings } from "../types";
import { useStore } from "../store";

export default function DownloadLocationSettings() {
  const { toast } = useStore();
  const [settings, setSettings] = useState<DownloadSettings | null>(null);
  const [choosing, setChoosing] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let alive = true;
    api.downloadSettings()
      .then((value) => { if (alive) setSettings(value); })
      .catch((reason) => { if (alive) setError(String(reason)); });
    return () => { alive = false; };
  }, []);

  const chooseRoot = async () => {
    setChoosing(true);
    setError("");
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected !== "string") return;
      const saved = await api.setDownloadRoot(selected);
      setSettings(saved);
      toast("后续下载将使用这个默认地址", "ok");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setChoosing(false);
    }
  };

  return (
    <section className="mt-5 rounded-(--r-md) bg-s2 p-3.5" aria-labelledby="download-location-title">
      <h2 id="download-location-title" className="text-[13px] font-semibold text-on">下载地址</h2>
      <p className="text-[11px] text-mut leading-relaxed mt-2">后续下载默认保存到这里；选集时仍可以只替换当前任务的地址。</p>
      <div className="mt-3 flex items-center gap-3 rounded-(--r-sm) bg-s3 p-2.5">
        <div className="flex-1 min-w-0">
          <div className="text-[10.5px] text-mut truncate" title={settings?.root_dir}>{settings?.root_dir || "正在读取…"}</div>
        </div>
        <button type="button" onClick={() => { void chooseRoot(); }} disabled={choosing || !settings}
          className="text-[11px] text-primary pressable disabled:opacity-50">
          {choosing ? "选择中…" : "更改"}
        </button>
      </div>
      {error && <p role="alert" className="text-[11px] text-danger mt-2 break-words">{error}</p>}
    </section>
  );
}
