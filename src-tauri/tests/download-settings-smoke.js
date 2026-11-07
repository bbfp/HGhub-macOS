// Runs only with the opt-in macos-smoke feature and a repository-local data dir.
(async () => {
  const config = window.__HG_SMOKE_CONFIG;
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const report = { defaults: null, invalidRejected: false, draftSurvivesPolling: false, savedValueSurvivesStalePoll: false, saved: null, rootPreservesLimits: false, error: null };
  const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const wait = async (test) => {
    const start = performance.now();
    while (!(await test())) {
      if (performance.now() - start > 12000) throw new Error("download settings checkpoint timed out");
      await delay(50);
    }
  };
  const values = (settings) => [settings.max_tasks, settings.max_episodes_per_drama, settings.max_segments_per_episode];
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  try {
    await wait(() => document.querySelector('form[aria-label="下载并发设置"]'));
    const profile = Array.from(document.querySelectorAll("nav button")).find((button) => button.textContent.trim() === "我的");
    if (!profile) throw new Error("profile navigation missing");
    profile.click();
    const form = document.querySelector('form[aria-label="下载并发设置"]');
    form.scrollIntoView({ block: "center" });
    const before = await invoke("download_settings");
    const expected = config.expected || [5, 3, 4];
    report.defaults = values(before);
    if (!same(report.defaults, expected)) throw new Error(`unexpected settings ${report.defaults}`);
    const inputs = Array.from(form.querySelectorAll("input"));
    await wait(() => same(inputs.map((input) => Number(input.value)), expected));
    if (!config.verify_only) {
      const input = (index, value) => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(inputs[index], String(value));
        inputs[index].dispatchEvent(new Event("input", { bubbles: true }));
      };
      const save = form.querySelector('button[type="submit"]');
      input(0, 0);
      await delay(50);
      save.click();
      await wait(() => form.querySelector('[role="alert"]'));
      if (!same(values(await invoke("download_settings")), expected)) throw new Error("invalid limit changed persisted settings");
      report.invalidRejected = true;
      input(0, 7); input(1, 2); input(2, 6);
      await delay(2000);
      report.draftSurvivesPolling = same(inputs.map((input) => Number(input.value)), [7, 2, 6]);
      if (!report.draftSurvivesPolling) throw new Error("task polling overwrote an unsaved draft");
      // Hold a poll's old settings until after the UI has saved new settings.
      // This reproduces a slow earlier read arriving after a successful save.
      // Tauri's invoke property is read-only; intercept the exposed callback
      // map so the real IPC response is delayed rather than replacing invoke.
      const callbacks = window.__TAURI_INTERNALS__.callbacks;
      const originalSet = callbacks.set;
      let held = false;
      let release;
      callbacks.set = function (callbackId, callback) {
        return originalSet.call(this, callbackId, (data) => {
          if (!held && data && data.schema_version === 1 && typeof data.root_dir === "string" && same(values(data), expected)) {
            held = true;
            release = () => callback(data);
            return;
          }
          callback(data);
        });
      };
      try {
        await wait(() => typeof release === "function");
        save.click();
        await wait(async () => same(values(await invoke("download_settings")), [7, 2, 6]));
        await wait(() => save.disabled && !form.querySelector("fieldset").disabled);
        release();
        await delay(200);
        report.savedValueSurvivesStalePoll = same(inputs.map((input) => Number(input.value)), [7, 2, 6]);
        if (!report.savedValueSurvivesStalePoll) throw new Error("stale poll overwrote newly saved settings");
      } finally {
        if (release) release();
        callbacks.set = originalSet;
      }
      report.saved = values(await invoke("download_settings"));
      const moved = await invoke("download_settings_set", { rootDir: config.root_dir });
      report.rootPreservesLimits = same(values(moved), [7, 2, 6]);
      if (!report.rootPreservesLimits) throw new Error("root save reset concurrency settings");
      await wait(() => save.disabled);
    } else { report.saved = values(before); }
  } catch (error) { report.error = String(error); }
  window.__HG_DOWNLOAD_SETTINGS_REPORT = report;
  await invoke("debug_log", { msg: `[DOWNLOAD_SETTINGS_SMOKE] ${JSON.stringify(report)}` });
})();
