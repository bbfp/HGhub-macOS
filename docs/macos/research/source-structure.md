# 播放链路结构

## 已确认的本地事实

- `src/api.ts` 把图片和播放列表写成 `http://hg.localhost/img|hls|seg?...`。
- `src-tauri/src/lib.rs` 注册 `hg` 异步协议，但将子资源 scheme 固定为 `http`；`src-tauri/src/media.rs` 负责图片 AES-CBC 解密、m3u8 重写和 key/分片转发。
- `docs/API.md` 描述公开播放地址为 HLS、约 1 小时签名参数、AES-128 key 行，要求播放前重新获取。此内容是仓库文档/历史实测说明，不是本轮源站复测。
- `media.rs` 只有进程内 `HashMap` 缓存，重启后不会保留封面和播放列表。

## 本轮公开探针

2026-11-04 通过本机系统 HTTP 代理访问 `huangguoai.com`，代理连接返回 503，未拿到播放清单、key 或分片；结果记录在 `probes/source-structure-probe.json`。不能据此判定源站不可用，也不能声称本轮验证了 AES-128 或签名过期窗口。

## 实施门禁

macOS 播放需要先解决 Tauri/Wry 在 macOS 下自定义 scheme 的 URL 形式；官方资料说明 macOS 与 Windows/Android 的本地 URL 形式不同。必须在 macOS dev/build 版实际播放一次后再宣称通过。
