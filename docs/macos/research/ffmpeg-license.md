# FFmpeg 现状与分发边界

本机 `/opt/homebrew/bin/ffmpeg` 为 arm64 FFmpeg 8.1；`configuration` 含 `--enable-gpl`、`libx264` 等，且动态链接 Homebrew 路径下多个库。FFmpeg 官方说明：基础代码主要 LGPL，但启用 GPL 部件时 GPL 条件会适用；官方合规清单建议不启用 GPL/nonfree、动态链接并提供对应源码与构建配置。

因此当前 Homebrew 二进制只可作本机探针工具，不可直接放进最终 `.app` 当作已解决的 sidecar。执行阶段需选择：

1. 构建一个许可证和依赖边界清晰的 arm64 sidecar，并把 notices/构建配置放进 App；或
2. 先不打包 FFmpeg，使用系统已有工具只做开发探针，S0 MP4 合并延后；或
3. 采用纯 Rust 的受限封装方案，接受格式覆盖面较小。

本文件不是法律意见；若 `.dmg` 给他人，分发义务需要重新评估。
