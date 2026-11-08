# 候选与路线证据

访问日：2026-11-04。以下结论按源码/官方页面可核对范围记录；README 功能声明不等于本项目已复现。

| 候选 | 证据 | 许可证/平台 | 结论 | 置信度 |
|---|---|---|---|---|
| `Liubsyy/M3U8Quicker` | GitHub README 写明 Tauri+Rust+React/TS、Windows/macOS/Linux、Apple Silicon、AES-128、批量、并发、暂停/续传、分片重试和 MP4；源码树有 downloader/persistence/models/remux 和本地 HLS 测试服务器 | Apache-2.0；Tauri；其 Cargo manifest 写 `rust-version=1.88` | 借鉴结构与失败处理；不直接嵌入，避免把大应用和额外功能带入 S0 | 高 |
| `nilaoda/N_m3u8DL-RE` | MIT；C#/.NET CLI；源码可见 HLS extractor、AES/key processor、EXT-X-MAP、分片下载管理和大量测试；README 有线程数、重试、保存目录、MP4 merge | MIT；跨平台含 macOS，但引入 .NET/子进程 | 借鉴解析/身份字段；暂不作为内嵌引擎 | 高 |
| `jackotom/hongguo-drama` | 同类短剧桌面项目；有 `DownloadManager`、HLS staging、AES-128/NONE 限制、取消、空间检查、FFmpeg `-c copy`/faststart、重试脚本与缓存检查 | GPL-3.0；附 GPL/LGPL FFmpeg notices | 借鉴需求、失败处理和目录/缓存思路；不复制 GPL 代码 | 高 |
| `vinitkumargoel/goel` | 原生 macOS Swift 下载管理器；统一队列、暂停/恢复、HLS VOD AES-128→MP4，并拒绝 DRM/独立音轨 | 公开 API 返回 `NOASSERTION`；Swift | 交互与限制策略参考，不复制 | 中 |
| `bezzad/Downloader.Desktop` | 跨平台下载器；选择目录/队列，任务自动保存，HLS 插件并行分片合成单文件，FFmpeg 首次获取 | 需再核许可证与插件边界 | 目录/队列交互参考；不作为依赖 | 中 |
| `utmapp/UTM` | Apple Silicon/macOS 虚拟机；Apache-2.0 | 不是原生 `.app` 迁移，资源和操作成本高 | Route B 对照/回退，不进 S0 | 高 |
| `Whisky-App/Whisky` | Apple Silicon Wine wrapper；仓库已归档，GPL-3.0 | 已归档且非原生 HGhub | 淘汰为交付路线 | 高 |
| `kksimp/Mac.apk` / `nizwar/avo` | Apple Silicon 上运行 Android；Mac.apk 为 AGPL-3.0 且 macOS 26.4+；Avo 为 Apache-2.0、VM/AOSP 路线 | 不能把现有 Tauri 项目变为原生 App；版本/兼容矩阵另有风险 | Route B 仅作对照，不进 S0 | 高 |
| `junmo-zy/hongguo-mac` 等发行仓 | 搜索结果显示 Apple Silicon/macOS 原生短剧客户端、离线/MP4/续传等；部分仓库不是完整源码或许可证未声明 | 证据边界和许可证不完整 | 只作为产品需求/交互线索，不复制 | 中 |

## 原仓库检查

GitHub API 的默认分支信息显示 `Minireel/HGhub` 当前仍为 MIT；本次列出的前 100 个 fork 描述没有出现明确 macOS/dmg/Apple 关键词，原仓 issues/PR API 未返回条目。未发现可直接合并的 macOS 分支。
