# 本地构建与验证

仓库根目录执行所有命令。`scripts/macos.mjs` 为本进程设置仓库局部 `RUSTUP_HOME`、`CARGO_HOME` 和 Rust 1.90 路径，不修改系统默认工具链。

```sh
pnpm install --frozen-lockfile
pnpm test:platform
pnpm build
node scripts/macos.mjs cargo test --locked --offline --lib
node scripts/macos.mjs cargo clippy --locked --offline --all-targets --all-features -- -D warnings
pnpm macos:build
```

正常构建目标是 `aarch64-apple-darwin`。macOS 专用配置 `src-tauri/tauri.macos.conf.json` 自动合入通用配置，生成 App 和 DMG，不依赖 Homebrew 的动态库。源码与锁文件保留原项目的 Windows/Android 配置；本轮没有重建这两个平台。

## 局部工具链与 FFmpeg

当前工作区已经准备 Rust 1.90，位于 `.local/rustup/toolchains/1.90.0-aarch64-apple-darwin`，Cargo 缓存在 `.local/cargo`。新电脑需自行安装同版本到这两个局部目录，或按实际路径调整构建脚本；App 的使用者不需要这些开发工具。

FFmpeg 8.1 源码位于 `.local/ffmpeg/FFmpeg-n8.1`，原始压缩包位于 `.local/ffmpeg/ffmpeg-n8.1.tar.gz`。来源、SHA-256、构建参数和许可证在 [third-party](third-party/) 中。使用系统 clang 和 macOS SDK 构建，未启用 GPL/nonfree、网络协议或 Homebrew 库。

在源码目录执行 `third-party/ffmpeg-build-config.json` 中的 configure 参数（prefix 改为当前仓库下的 `.local/ffmpeg/runtime`），再执行 `make -j4` 和 `make install`。构建后核对：

```sh
.local/ffmpeg/runtime/bin/ffmpeg -version
otool -L .local/ffmpeg/runtime/bin/ffmpeg
```

App 内的 `Contents/Resources/binaries/ffmpeg` 负责把已下载、已解密的片段合并为 MP4，不转码。`Contents/Resources/third-party/` 保留许可证和来源记录。仓库内下载/构建来源记录不代表已完成对外分发的许可审查；本交付范围为个人自用。

## 存储约定

`downloads/tasks.json` 为 v1 任务数组，包含稳定任务 ID、剧集 ID、集数、目标根目录、状态、片段校验记录及时间。`downloads/settings.json` 保存默认根目录，以及 `max_tasks`、`max_episodes_per_drama`、`max_segments_per_episode`，默认分别为 5、3、4。旧设置缺少这些字段时使用默认值，schema 仍为 v1。其他版本的记录、未知字段和无法读取的单条记录保留。整个任务文件损坏时先改名保留，再显示提示；若保留失败则停止写入。

`downloads/library-index.json` 是 schema 1 的本地短剧索引。Rust 下载管理器启动时将它载入内存 `HashMap`，列表页通过一次批量 IPC 查询当前可见短剧 ID。索引只收录已完成且 MP4/回执校验通过的集数，任务完成时增量更新；缺少索引时从现有任务和回执兼容重建。索引损坏会先保留原文件并在内存中重建，未知 schema 不会覆盖原文件。

新视频目录使用 `drama-ID-名字/episodes/NNN.mp4`，标题无法安全使用时使用 `drama-ID`。临时片段在该剧 `.tmp/ep-NNN`，原子写入后记录字节数和 SHA-256，恢复前核对。旧版本的 `drama-ID` 目录会被识别并继续播放。完成回执 `NNN.json` 包含集信息、MP4 字节数及 SHA-256，在发布最终 MP4 前写入。已有 MP4 只有与回执相符时才会被当作中断后已完成，不会自动覆盖无法核对的文件。成功后只清理本次登记的临时文件。

调度器唯一负责保存任务和设置，按剧编号轮转，同时满足全局任务与同剧活动集数上限。设置命令进入同一串行队列，先保存成功再更新运行时限制，避免目录与并发设置互相覆盖。每集 worker 在补充片段时读取当前分片上限；降低上限不终止已有操作，等待活动数自然降下去。全局任务和同剧集数范围为 1–32，分片范围为 1–16。暂停与取消中断请求、释放名额；继续会等待旧操作停止后再排队。过期地址每次运行最多刷新一次；持续失败会显示错误。

失败和取消任务的删除也进入同一串行队列，只允许删除已经停止的失败/取消记录；活动任务、暂停任务和已完成任务会被拒绝。删除会保留 MP4、回执和封面，只清理该任务的临时片段。已完成文件的 Finder 定位由 Rust 侧根据任务 ID 解析路径并调用 macOS `open -R`，前端不直接传入任意路径。

`hg://localhost/file` 只允许任务记录对应的视频，比较时使用规范化后的完整文件路径。Range 请求按需定位读取，每次最多返回 8 MiB；不是流式传输。无 Range 的请求仍按协议返回整个文件。

`HGHUB_DATA_DIR` 可将应用数据放到指定绝对路径，用于本地验证或便携启动。正常启动时不设置它。开发网络诊断可用现有的 `HGHUB_PROXY` 只覆盖该次进程的代理，不改系统设置。

## API 域名配置

App 数据目录的 `network.json` 独立保存 `{ "schema_version": 1, "domains": [...] }`，不改变下载或会话文件。启动时同步加载；写入先落临时文件再 rename。损坏 JSON 先备份并提示；未知版本配置保留原文件，使用默认值，并拒绝由当前版本覆盖。

所有原站 API 请求都通过统一候选域名路由：App 启动时并行检测全部配置域名，按有效响应耗时排序；登录、注册、登录后读取、下载播放地址和账号操作从最快域名开始，连接/网关失败后按排序继续换源。其他写操作只在请求尚未送达的连接/TLS 故障时换源，超时、响应中断和 5xx 会提示结果未知并停止重发。当前会话令牌只附加到 API 信封，不附加到封面、HLS、密钥或 CDN 分片请求。

短剧播放和下载地址使用当前源站的 `POST /api/videos/:id/episodes/:ep/playback` 接口，读取 `streams.main`；仍保留旧 `GET /api/videos/:id/play?ep=...` 的兼容回退。播放接口返回 HTTP 200 但没有有效地址时会继续尝试下一个镜像。

站点 URL 只替换精确匹配的主站或已配置镜像 origin，保留路径和签名查询；独立 CDN host 保持原样，并使用当前成功站点作为 Referer。下载器、封面、HLS 清单、密钥和分片共用媒体取数入口。

“我的 → 下载地址”保存默认下载根目录；“下载”页同步显示并可修改该地址；选集面板的“本次换地址”只覆盖当前任务。旧版 `drama-ID` 文件按任务记录继续读取，新任务使用 `drama-ID-名字`。

下载页和本地库通过应用内任务变更事件联动。更新下载成功加入任务后，下载页立即刷新；暂停、继续、取消和删除也会立即更新总览，不等待下一次轮询。

macOS 侧栏的“下载”与“我下载的”是两个独立入口。“下载”按短剧分组并折叠任务；“我下载的”按本地库分页读取已完成集数，并只把远端编号大于本地最大集数的内容标为更新。更新检查失败显示未检测，不会自动加入任务。

可显式运行真实只读验证（默认测试会跳过外网）：

```sh
node scripts/macos.mjs cargo test --locked --offline --lib live_default_domains_return_video_lists -- --ignored --nocapture
node scripts/macos.mjs cargo test --locked --offline --lib media::tests::live_play_chain -- --ignored --nocapture
node scripts/macos.mjs cargo test --locked --offline --lib live_source_episode_download_and_full_decode -- --ignored --nocapture
```

## macOS WebView 测试

`macos-smoke` 是显式启用的测试 feature，默认构建不包含测试脚本。它把测试结果写入指定应用数据目录的 `debug.log`。需要本地已登记的合成 MP4 与 loopback HLS 服务；不会把样本任务放入正常 App。

```sh
node scripts/macos.mjs build --debug --bundles app --features macos-smoke
# HGHUB_SMOKE_CONFIG 指向包含 mp4_path 和 hls_url 的本地 JSON。
# 同时设置 HGHUB_DATA_DIR，直接运行测试 App 内的 MacOS/huangguo-client。
```

六项需外网/人工运行的测试默认忽略。真实单集测试默认写入仓库 `.local/tests/`；`HGHUB_LIVE_DOWNLOAD_DIR` 可指向其中已有样本目录，复用完成文件继续解码核验，避免重复下载。当前本地测试不依赖源站。测试源和许可证等说明见 [tests/fixtures/hls](../../src-tauri/tests/fixtures/hls/README.md)。

测试启动脚本只为 `cargo test` 子进程补充本机地址的 `NO_PROXY`，避免系统代理影响本地 HTTP 测试服务。App 和系统网络设置不受影响。
