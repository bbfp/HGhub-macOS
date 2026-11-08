# 调研来源与覆盖范围

访问日期：2026-11-04。只依赖一手仓库、官方文档和本机检查。网页摘要仅用于发现候选，关键候选已读取 README/许可证/源码。

## 原仓库与平台官方文档

- [HGhub](https://github.com/Minireel/HGhub)，本地 HEAD `e4ab246` / v1.0.1。
- [Tauri macOS Application Bundle](https://v2.tauri.app/distribute/macos-application-bundle/)：原生 bundle 布局、资源、最低系统配置。
- [Tauri Embedding External Binaries](https://v2.tauri.app/develop/sidecar/)：sidecar 与平台 target 命名。
- [Tauri config / bundle targets](https://v2.tauri.app/reference/config/#bundleconfig)：app/dmg、resources、macOS 配置。
- [Tauri convertFileSrc](https://v2.tauri.app/reference/javascript/api/namespacecore/#convertfilesrc)：注册协议/文件访问的前端 API。
- [Wry Android scheme change](https://v2.tauri.app/release/wry/v0.33.0/) 及 [Tauri 官方讨论 #9174](https://github.com/orgs/tauri-apps/discussions/9174)：Windows/Android 与 macOS 本地 scheme 差异；具体运行行为仍待本机验证。
- [Tauri folder dialog](https://v2.tauri.app/plugin/dialog/)：macOS 目录选择及 Rust 调用。
- [HLS RFC 8216](https://www.rfc-editor.org/rfc/rfc8216)：AES-128/CBC、IV/序列、EXT-X-MAP。
- [FFmpeg 官方许可说明](https://ffmpeg.org/legal.html)：配置与二进制许可边界；实际分发方案尚未选定。
- [m3u8-rs 6.0.1](https://docs.rs/m3u8-rs/6.0.1/m3u8_rs/)：成熟 Rust 清单解析器候选，尚未引入。

## 深读候选

- [M3U8Quicker](https://github.com/Liubsyy/M3U8Quicker)，读取分支 master、树 `a87d00f6fb5a8379e42251fddc7d11b1d1ba4321`；[downloader](https://github.com/Liubsyy/M3U8Quicker/blob/a87d00f6fb5a8379e42251fddc7d11b1d1ba4321/src-tauri/src/downloader.rs)、[persistence](https://github.com/Liubsyy/M3U8Quicker/blob/a87d00f6fb5a8379e42251fddc7d11b1d1ba4321/src-tauri/src/persistence.rs)、[Cargo](https://github.com/Liubsyy/M3U8Quicker/blob/a87d00f6fb5a8379e42251fddc7d11b1d1ba4321/src-tauri/Cargo.toml)、LICENSE。
- [N_m3u8DL-RE](https://github.com/nilaoda/N_m3u8DL-RE)，读取 LICENSE、[HLS extractor](https://github.com/nilaoda/N_m3u8DL-RE/blob/main/src/N_m3u8DL-RE.Parser/Extractor/HLSExtractor.cs)、[key processor](https://github.com/nilaoda/N_m3u8DL-RE/blob/main/src/N_m3u8DL-RE.Parser/Processor/HLS/DefaultHLSKeyProcessor.cs)。仅借鉴标准协议设计，不引入 .NET。
- [hongguo-drama](https://github.com/jackotom/hongguo-drama)，读取 LICENSE、[DownloadManager](https://github.com/jackotom/hongguo-drama/blob/main/src/components/DownloadManager.jsx)、[HLS staging](https://github.com/jackotom/hongguo-drama/blob/main/src/native/hls-media.js)、重试说明和 package manifest。它连接红果等平台，与本项目黄果的业务接口不同。
- [ffmpeg-static b6.1.1](https://github.com/eugeneware/ffmpeg-static/releases/tag/b6.1.1)：存在 darwin-arm64 ffmpeg/ffprobe 及各自 README/LICENSE；二进制约 45 MB/个。尚未下载或采纳。

## 快速筛选候选

- [UTM](https://github.com/utmapp/UTM)、[Whisky](https://github.com/Whisky-App/Whisky)、[PlayCover](https://github.com/PlayCover/PlayCover)、[Waydroid](https://github.com/waydroid/waydroid)、[Android Emulator](https://developer.android.com/studio/run/emulator)、[Mac.apk](https://github.com/kksimp/Mac.apk)、[Avo](https://github.com/nizwar/avo)。PlayCover 是 iOS 包装器，并不能运行 Android APK；Waydroid 面向 GNU/Linux。
- [hongguo-mac](https://github.com/junmo-zy/hongguo-mac)、[hongguo-short-drama-macos](https://github.com/StriveToLearnCode/hongguo-short-drama-macos)、[hongguo-player](https://github.com/yangbin1322/hongguo-player)：是相邻产品参考，不能替代 HGhub 的在线功能；部分为发行/说明仓，许可证未声明，不复制。
- [Goel](https://github.com/vinitkumargoel/goel)、[Downloader.Desktop](https://github.com/bezzad/Downloader.Desktop)、[FerrisLoadDownloader](https://github.com/blueokanna/FerrisLoadDownloader)：后者是 Flutter+Rust，并非现有 Tauri 栈；可查源码但集成代价较高，不作为依赖。

## 检索记录与限制

检索覆盖：HGhub macOS/forks；“短剧 macOS 客户端 下载 离线”；“short drama macOS app downloader”；Tauri HLS/sidecar/macOS；m3u8 AES-128 resume Rust/Go；Windows ARM/Android Apple Silicon；GitLab m3u8 downloader AES-128。GitLab 简查未筛到足以改变主路线的候选，未据此宣称该平台无方案；Hugging Face 与本次非 ML 媒体封装的相关性低，未为凑数量强行引入。

GitHub 原仓前 100 个 fork 仅检查了描述关键词，未遍历所有提交；没有找到现成 macOS 分支，不能推断所有 fork 均无相关实现。主仓 API issues/PR 返回空列表，不扩大为“从来没有问题”。

未执行候选完整测试、依赖漏洞扫描或第三方程序安装；因此表中“功能/质量”结论只覆盖读取的源码与可观察状态，不等于第三方运行验收。
