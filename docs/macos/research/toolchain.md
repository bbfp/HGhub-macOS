# 本地工具链盘点

检查日期：2026-11-04（本机 arm64 macOS）。

## 已观察

```text
rustc 1.82.0 (f6e511eec 2024-10-15)
binary: rustc
commit-hash: f6e511eec7342f59a25f7c0534f1dbea00d01b14
commit-date: 2024-10-15
host: aarch64-apple-darwin
release: 1.82.0
LLVM version: 19.1.1

Default host: aarch64-apple-darwin
rustup home:  repository-local `.local/rustup`

stable-aarch64-apple-darwin (default)
rustc 1.82.0 (f6e511eec 2024-10-15)

cargo 1.82.0 (8f40fc59f 2024-08-21)

ffmpeg version 8.1 Copyright (c) 2000-2026 the FFmpeg developers
built with Apple clang version 17.0.0 (clang-1700.6.4.2)
configuration: --prefix=/opt/homebrew/Cellar/ffmpeg/8.1 --enable-shared --enable-pthreads --enable-version3 --cc=clang --host-cflags= --host-ldflags= --enable-ffplay --enable-gpl --enable-libsvtav1 --enable-libopus --enable-libx264 --enable-libmp3lame --enable-libdav1d --enable-libvpx --enable-libx265 --enable-openssl --enable-videotoolbox --enable-audiotoolbox --enable-neon
```

- `rustup toolchain list` 只有 `stable-aarch64-apple-darwin (default)`，版本为 rustc 1.82.0。
- `pnpm install --frozen-lockfile` 成功；`pnpm build` 成功，产物在忽略的 `dist/`。
- `cargo check --locked` 未完成：当前锁文件中的 Tauri 2.12.5、tauri-build 2.7.4 和 tauri-utils 2.10.2 要求 Rust 1.90；serde_with 3.24.0/time 0.3.55 也分别要求约 1.88。网络下载 `cairo-rs` 时通过现有代理超时，故本次没有源码编译成功。
- `cargo check --locked --offline` 也未完成：本机缓存只有 `crossbeam-channel 0.5.16`，锁文件要求 0.5.17。
- 本机 Homebrew FFmpeg/ffprobe 8.1 是 arm64，配置含 `--enable-gpl`、`libx264` 等 GPL 组件，并动态依赖 Homebrew 路径下多个 dylib；不能直接当作可随 `.dmg` 自包含分发的组件。
- 没有全局升级工具链；Rust 1.90 安装和 FFmpeg 可分发构建留给执行阶段的明确动作。

## 对计划的影响

S0 构建前需要在本机获得 Rust >= 1.90（优先项目所需的稳定工具链，是否安装需单独确认/记录），并决定使用自包含、许可边界清晰的 FFmpeg sidecar 或推迟 MP4 合并。当前前端构建通过不代表 Tauri macOS App 已构建。
