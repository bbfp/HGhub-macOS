# 调研结论

## 结论

推荐 Route A：在现有 Tauri 项目上做 macOS Apple Silicon 适配，并在 Rust 侧自研当前源站 VOD 所需的最小下载器。成熟项目只借鉴协议解析、任务持久化、失败处理与 UI；不直接嵌入 .NET/GPL/大型完整应用。

## 已证明

- React/Vite 前端在锁定依赖下 `pnpm build` 成功。
- 本机 arm64 FFmpeg 能将自生成 AES-128 TS HLS 和 fMP4 HLS 用 `-c copy` 合成可被 `ffprobe` 完整解码、faststart 的 MP4。
- Tauri 官方配置支持 `app`/`dmg` 目标和 resources/sidecar；官方资料说明 macOS 与 Windows/Android 的本地 scheme URL 形式不同。
- GitHub 候选中存在与需求高度相似的 Tauri/HLS/短剧实现，但许可证和集成成本不同。

## 未证明

- 本机 Rust 1.82 无法满足当前 lockfile 要求的 Tauri 2.12.5 及部分依赖 Rust >=1.90；尚未安装新工具链。
- 当前网络代理到源站返回 503，未验证实时 m3u8/key/segment 结构和签名有效期。
- macOS WKWebView 下现有 `http://hg.localhost` 媒体链路尚未播放验证。
- `.app`/`.dmg` 尚未构建；FFmpeg 可分发 sidecar 尚未决定。
- 下载任务/磁盘缓存/离线库尚未实现。

## S0 最小路径

1. 获得项目所需 Rust 工具链并先修正 macOS 平台协议 URL，构建 `.app`，实测在线播放。
2. 设计带 `schema_version` 的下载任务/剧集/集元数据与目录结构；实现磁盘封面 read-through cache。
3. 以标准 AES-128 VOD 为目标实现单集下载、分片临时目录、暂停/取消/重试、退出恢复和有限公平并发；成功后用已验证的 MP4 合并层输出单集。
4. 接入逐集/整剧选择、本地库和断网播放；再打包 `.app`/`.dmg`。

每一步都需要把“静态构建、合成探针、真实在线播放、真实离线播放、跨版本读取”分开记录。
