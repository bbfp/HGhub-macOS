# Apple Silicon macOS 构建

本分支的目标是 Apple Silicon Mac。需要已安装的 Xcode Command Line Tools、Node.js 22、pnpm 10、Python 3，以及本地 Rust 1.90.0。正常 App 使用者不需要这些开发工具。

## 1. 准备项目依赖和 Rust

以下命令从仓库根目录执行。Rust 和 FFmpeg 放在被 Git 忽略的 `.local/`；不会设置系统默认 Rust 工具链或修改 shell 启动文件。

```sh
pnpm install --frozen-lockfile
mkdir -p .local
curl --proto '=https' --tlsv1.2 -fsS https://sh.rustup.rs -o .local/rustup-init.sh
RUSTUP_HOME="$PWD/.local/rustup" CARGO_HOME="$PWD/.local/cargo" \
  sh .local/rustup-init.sh -y --no-modify-path --profile minimal --default-toolchain 1.90.0
```

`scripts/macos.mjs` 为每个构建/检查进程显式设置 `.local/rustup`、`.local/cargo` 和 `1.90.0-aarch64-apple-darwin`。如已有同版本工具链，也可自行调整该脚本的路径。

## 2. 从官方源码准备 FFmpeg

App 通过本地 FFmpeg 合并已下载且已解密的片段，不重新编码。源码版本 n8.1、官方来源和 SHA-256 记录在 [ffmpeg-source.json](third-party/ffmpeg-source.json)，功能配置在 [ffmpeg-build-config.json](third-party/ffmpeg-build-config.json)。prefix 根据当前仓库位置计算。

```sh
mkdir -p .local/ffmpeg
curl -fL https://codeload.github.com/FFmpeg/FFmpeg/tar.gz/refs/tags/n8.1 \
  -o .local/ffmpeg/ffmpeg-n8.1.tar.gz
```

先验证源码包，再解压、配置、构建：

```sh
python3 - <<'PY_BUILD'
from pathlib import Path
import hashlib, json, subprocess
repo = Path.cwd()
source = json.loads((repo / "docs/macos/third-party/ffmpeg-source.json").read_text())
archive = repo / ".local/ffmpeg/ffmpeg-n8.1.tar.gz"
if hashlib.sha256(archive.read_bytes()).hexdigest() != source["sha256"]:
    raise SystemExit("FFmpeg 源码校验失败，未解压或构建")
subprocess.run(["tar", "-xzf", str(archive), "-C", str(repo / ".local/ffmpeg")], check=True)
source_dir = repo / ".local/ffmpeg/FFmpeg-n8.1"
args = json.loads((repo / "docs/macos/third-party/ffmpeg-build-config.json").read_text())
args.insert(1, "--prefix=" + str(repo / ".local/ffmpeg/runtime"))
subprocess.run(args, cwd=source_dir, check=True)
subprocess.run(["make", "-j4"], cwd=source_dir, check=True)
subprocess.run(["make", "install"], cwd=source_dir, check=True)
PY_BUILD
.local/ffmpeg/runtime/bin/ffmpeg -version
otool -L .local/ffmpeg/runtime/bin/ffmpeg
```

该配置关闭 GPL、nonfree、自动依赖探测和网络协议，不依赖 Homebrew 动态库。保留 `.local/ffmpeg/ffmpeg-n8.1.tar.gz`、构建配置和许可证；再分发包含 FFmpeg 的二进制时，需按其许可证提供对应源码等材料。许可证说明见 [FFmpeg-LICENSE.md](third-party/FFmpeg-LICENSE.md)。本仓库不包含预构建 FFmpeg 或下载的视频。

## 3. 测试与打包

```sh
pnpm test:platform
pnpm build
node scripts/macos.mjs cargo test --locked --lib
node scripts/macos.mjs cargo clippy --locked --all-targets --all-features -- -D warnings
pnpm macos:build
```

Cargo 依赖缓存齐全后可以加 `--offline`。构建自动合入 `src-tauri/tauri.macos.conf.json`，生成：

- `src-tauri/target/aarch64-apple-darwin/release/bundle/macos/HGhub.app`
- `src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/HGhub_1.0.1_aarch64.dmg`

App 采用本机临时签名，没有 Apple 公证。Rust 和合成媒体测试需要上述 FFmpeg/FFprobe；6 项外网或人工用例默认跳过。`src-tauri/tests/fixtures/hls/` 全部是自生成的短测试素材。

## 数据与兼容

正常数据目录为 `~/Library/Application Support/com.hghub.huangguo/`，内含 `downloads/tasks.json`、`downloads/settings.json`、`network.json` 和图片缓存。视频根目录由使用者选择。`HGHUB_DATA_DIR` 可以只为测试进程指定另一绝对路径，测试时不要与正在运行的正常 App 共用目录。

下载设置 schema 保持 v1，新增 `max_tasks`、`max_episodes_per_drama`、`max_segments_per_episode`，缺少时使用 5、3、4。全局/同剧范围 1–32，每集分片范围 1–16。目录和并发设置由管理器串行保存，保存成功后生效；降低限制时不取消已有工作，等待名额自然释放。

任务与未知字段保留，旧 `drama-ID` 文件继续可读；新任务使用 `drama-ID-名字/episodes/NNN.mp4`。分片原子写入并记录长度、SHA-256，恢复前核验；最终 MP4 与回执按校验值配对。整个任务 JSON 损坏时保留原文件并提示，尚未实现自动扫描回执重建列表。

`macos-smoke` 是显式启用的本地 WKWebView 测试 feature，普通构建不包含测试脚本。目录、网络和并发配置的测试必须使用独立数据目录；测试服务访问 loopback 时，测试启动脚本仅对自身进程补充 NO_PROXY，不修改系统代理。
