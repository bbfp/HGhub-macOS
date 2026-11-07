import { useEffect, useId, useState } from "react";
import type { FormEvent } from "react";
import { api } from "../api";
import type { DownloadSettings } from "../types";
import { useStore } from "../store";

const fields = [
  { key: "max_tasks", label: "全局同时下载任务数", max: 32 },
  { key: "max_episodes_per_drama", label: "同一部剧同时下载集数", max: 32 },
  { key: "max_segments_per_episode", label: "每集同时下载分片数", max: 16 },
] as const;

const draftFrom = (settings: DownloadSettings) => ({
  max_tasks: String(settings.max_tasks),
  max_episodes_per_drama: String(settings.max_episodes_per_drama),
  max_segments_per_episode: String(settings.max_segments_per_episode),
});

export default function DownloadConcurrencySettings({ settings, onSaved }: {
  settings: DownloadSettings;
  onSaved: (settings: DownloadSettings) => void;
}) {
  const { toast } = useStore();
  const id = useId();
  const [draft, setDraft] = useState(() => draftFrom(settings));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  // Task polling must not overwrite numbers while the user is editing.
  useEffect(() => {
    setDraft({
      max_tasks: String(settings.max_tasks),
      max_episodes_per_drama: String(settings.max_episodes_per_drama),
      max_segments_per_episode: String(settings.max_segments_per_episode),
    });
  }, [settings.max_tasks, settings.max_episodes_per_drama, settings.max_segments_per_episode]);

  const invalid = fields.find(({ key, max }) =>
    !/^\d+$/.test(draft[key]) || Number(draft[key]) < 1 || Number(draft[key]) > max,
  );
  const dirty = fields.some(({ key }) => Number(draft[key]) !== settings[key]);

  const save = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (invalid) {
      setError(`${invalid.label}必须是 1-${invalid.max} 的整数`);
      return;
    }
    setSaving(true);
    setError("");
    try {
      const saved = await api.setDownloadLimits({
        maxTasks: Number(draft.max_tasks),
        maxEpisodesPerDrama: Number(draft.max_episodes_per_drama),
        maxSegmentsPerEpisode: Number(draft.max_segments_per_episode),
      });
      onSaved(saved);
      setDraft(draftFrom(saved));
      toast("下载并发已保存并生效", "ok");
    } catch (reason) {
      setError(String(reason));
    } finally { setSaving(false); }
  };

  return (
    <form onSubmit={(event) => { void save(event); }} noValidate className="mt-3 rounded-(--r-sm) bg-s3 p-2.5" aria-label="下载并发设置">
      <fieldset disabled={saving} aria-describedby={`${id}-hint`}>
        <legend className="text-[11.5px] font-semibold text-on">下载并发</legend>
        <div className="mt-2 space-y-2">
          {fields.map(({ key, label, max }) => (
            <div key={key} className="flex items-center gap-3">
              <label htmlFor={`${id}-${key}`} className="flex-1 text-[11px] text-on">{label}</label>
              <input id={`${id}-${key}`} name={key} type="number" inputMode="numeric" min={1} max={max} step={1}
                value={draft[key]} onChange={(event) => { setDraft({ ...draft, [key]: event.target.value }); setError(""); }}
                className="w-20 h-8 px-2 rounded-(--r-sm) bg-s4 text-[12px] text-on text-right outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50" />
            </div>
          ))}
        </div>
        <p id={`${id}-hint`} className="mt-2 text-[10.5px] text-mut leading-relaxed">
          一个任务对应一集，同剧集数也计入全局任务数。默认 5 / 3 / 4，按默认设置最多同时下载 20 个分片。
          保存后用于后续调度和分片补充；降低上限时，已有下载先完成，再按新上限补充。
        </p>
        {error && <p role="alert" className="mt-2 text-[11px] text-danger">{error}</p>}
        <button type="submit" disabled={saving || !dirty} className="mt-2 min-h-8 px-3 rounded-(--r-ctl) bg-primary text-on-primary text-[11px] font-semibold pressable disabled:opacity-50">
          {saving ? "保存中…" : "保存并发设置"}
        </button>
      </fieldset>
    </form>
  );
}
