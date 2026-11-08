# 路线比较

## A：直接适配现有 Tauri（推荐）

复用现有在线页面、Rust API、HLS 代理和会话；修正 macOS 平台识别/协议 URL，增加下载任务、磁盘封面缓存、本地库和 `.app`/`.dmg` 构建。当前前端 build 已通过，Rust 版本门槛和 macOS 播放尚未通过。

## B：Windows/Android 在 Mac 上运行

UTM、Avo、Mac.apk、PlayCover 等证明 Apple Silicon 上存在虚拟机/兼容层或 Android 运行路径，但这不会产生本项目要求的原生 `.app`；Whisky 已归档。保留作“原生路线被阻塞时先自用”的对照，不能替代 A。

## C：借鉴同类短剧客户端

`hongguo-drama` 的下载管理/HLS staging/空间检查/失败恢复与其他 macOS 短剧项目的本地库交互最有参考价值；GPL 或未声明许可证使直接复用不合适。

## 横向组件

优先自研最小 VOD AES-128 下载内核：现有 reqwest、tokio、RustCrypto AES + 新增 m3u8 解析/任务状态；参考成熟实现处理 key/IV、EXT-X-MAP、重试、暂停/退出恢复、MP4 合并和空间检查。
