(async () => {
  const config = window.__HG_SMOKE_CONFIG;
  const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args);
  const report = { localRange: null, localPlayback: null, hlsPlayback: null, error: null };
  const wait = (test, ms = 12000) => new Promise((resolve, reject) => {
    const start = performance.now();
    const timer = setInterval(() => {
      if (test()) { clearInterval(timer); resolve(); }
      else if (performance.now() - start > ms) { clearInterval(timer); reject(new Error("media checkpoint timed out")); }
    }, 50);
  });
  const makeVideo = () => {
    const video = document.createElement("video");
    video.muted = true; video.playsInline = true;
    Object.assign(video.style, { position: "fixed", right: "12px", bottom: "12px", width: "112px", height: "196px", zIndex: "99999" });
    document.body.append(video);
    return video;
  };
  try {
    await new Promise((resolve) => setTimeout(resolve, 2000));
    const file = `hg://localhost/file?u=${encodeURIComponent(config.mp4_path)}`;
    const response = await fetch(file, { headers: { Range: "bytes=100-199" } });
    const bytes = await response.arrayBuffer();
    if (response.status !== 206 || bytes.byteLength !== 100) throw new Error(`range: ${response.status}/${bytes.byteLength}`);
    report.localRange = { status: response.status, bytes: bytes.byteLength };
    const local = makeVideo();
    local.src = file;
    await local.play();
    await wait(() => local.currentTime > 0.3);
    local.currentTime = 4;
    await wait(() => !local.seeking && local.currentTime > 4.2);
    report.localPlayback = { duration: local.duration, timeAfterSeek: local.currentTime, width: local.videoWidth, height: local.videoHeight };
    local.pause(); local.removeAttribute("src"); local.load(); local.remove();
    const online = makeVideo();
    const source = `hg://localhost/hls?u=${encodeURIComponent(config.hls_url)}`;
    const mode = window.Hls.isSupported() ? "hls.js" : "native";
    let hls;
    if (mode === "hls.js") {
      hls = new window.Hls({ enableWorker: true, manifestLoadingMaxRetry: 1, fragLoadingMaxRetry: 1 });
      hls.loadSource(source); hls.attachMedia(online);
      await wait(() => online.readyState >= 2);
    } else { online.src = source; }
    await online.play();
    await wait(() => online.currentTime > 0.3);
    report.hlsPlayback = { mode, duration: online.duration, time: online.currentTime, width: online.videoWidth, height: online.videoHeight };
    online.pause(); if (hls) hls.destroy(); online.remove();
  } catch (error) { report.error = String(error); }
  await invoke("debug_log", { msg: `[WK_SMOKE] ${JSON.stringify(report)}` });
})();
