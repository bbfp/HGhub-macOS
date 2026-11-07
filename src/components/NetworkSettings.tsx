import { useEffect, useState } from "react";
import { api } from "../api";
import type { NetworkSettingsView, NetworkTestResult } from "../types";
import { useStore } from "../store";

const splitDomains = (text: string) => text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export default function NetworkSettings() {
  const { toast, user, tab: activeTab } = useStore();
  const [settings, setSettings] = useState<NetworkSettingsView | null>(null);
  const [text, setText] = useState("");
  const [result, setResult] = useState<NetworkTestResult | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<"save" | "test" | null>(null);

  useEffect(() => {
    if (activeTab !== "profile") return;
    let alive = true;
    api.networkSettings().then((value) => {
      if (alive) { setSettings(value); setText(value.domains.join("\n")); }
    }).catch((reason) => { if (alive) setError(message(reason)); });
    return () => { alive = false; };
  }, [activeTab]);

  const run = async (action: "save" | "test") => {
    setBusy(action);
    setError("");
    try {
      const domains = splitDomains(text);
      if (action === "save") {
        const saved = await api.setNetworkDomains(domains);
        setSettings(saved);
        setText(saved.domains.join("\n"));
        setResult(null);
        toast("API 域名已保存，返回首页即可重试", "ok");
      } else {
        setResult(await api.testNetworkDomains(domains));
        setSettings(await api.networkSettings());
      }
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(null); }
  };

  const probes = result?.probes ?? settings?.probes ?? [];
  return (
    <section className="mt-5 rounded-(--r-md) bg-s2 p-3.5" aria-labelledby="network-settings-title">
      <h2 id="network-settings-title" className="text-[13px] font-semibold text-on">网络设置 · 备用域名</h2>
      <p id="network-domains-help" className="text-[11px] text-mut leading-relaxed mt-2">
        公开内容连接失败时依次换源，成功后优先使用该域名。每行一个域名，可添加、删除或调整顺序。
      </p>
      {settings && <p className="text-[10.5px] text-mut mt-2 break-all">当前读取域名：{user ? settings.domains[0] : settings.active_domain}</p>}
      <label htmlFor="network-domains" className="block text-[11.5px] text-on mt-3">API 域名列表</label>
      <textarea id="network-domains" aria-describedby="network-domains-help" value={text}
        onChange={(event) => { setText(event.target.value); setResult(null); }} disabled={busy !== null || !settings}
        spellCheck={false} autoCapitalize="none" autoCorrect="off" rows={5}
        placeholder="正在读取域名设置…"
        className="mt-1.5 w-full resize-y min-h-28 rounded-(--r-sm) bg-s3 p-2.5 text-[11.5px] font-mono text-on outline-none focus:ring-1 focus:ring-primary disabled:opacity-60" />
      <p className="text-[10.5px] text-mut leading-relaxed mt-1.5">
        支持直接填写域名，自动补齐 HTTPS。登录、下载和账号操作也会按顺序尝试这些域名。
        {user && "当前已登录；如果某个镜像不接受当前账号，程序会继续检查其他已配置域名。"}
      </p>
      {error && <p role="alert" className="text-[11px] text-danger mt-2 break-words">{error}</p>}
      {settings?.warning && <p role="status" className="text-[11px] text-mut mt-2 break-words">{settings.warning}</p>}
      <div className="flex gap-2 mt-3">
        <button type="button" onClick={() => { void run("save"); }} disabled={busy !== null || !settings}
          className="h-9 px-4 rounded-(--r-ctl) bg-primary text-on-primary text-[12px] font-semibold pressable disabled:opacity-50">
          {busy === "save" ? "保存中…" : "保存域名"}
        </button>
        <button type="button" onClick={() => { void run("test"); }} disabled={busy !== null || !settings}
          className="h-9 px-4 rounded-(--r-ctl) bg-s3 text-on text-[12px] pressable disabled:opacity-50">
          {busy === "test" ? "逐个检测中…" : "检测可用性"}
        </button>
      </div>
      <div aria-live="polite" aria-busy={busy === "test"}>
        {result && <p className="text-[11px] text-mut mt-3">{result.first_working ? `首个可用：${result.first_working}` : "所有域名暂时不可用，请检查网络或更换域名。"}</p>}
        {probes.length > 0 && <ul className="mt-2 space-y-2">
          {probes.map((probe) => (
            <li key={probe.domain} className="rounded-(--r-sm) bg-s3 p-2.5 text-[10.5px]">
              <div className="text-on break-all">{probe.domain}</div>
              <div className={`mt-1 ${probe.ok ? "text-primary" : "text-mut"}`}>
                {probe.ok ? "可用" : "不可用"}{probe.status ? ` · HTTP ${probe.status}` : ""} · {probe.elapsed_ms} ms
              </div>
              {probe.error && <p className="text-mut break-words mt-1">{probe.error}</p>}
            </li>
          ))}
        </ul>}
      </div>
    </section>
  );
}
