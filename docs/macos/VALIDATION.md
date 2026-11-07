> 当前状态（2026-11-07）：设计复核和 Router 定向只读复核已完成；Claude 最终实现复核因网关返回 402 未完成，不能把它写成通过。当前交付候选在 `out/macos-download-library-20261107/`；公开源码为 https://github.com/bbfp/HGhub-macOS。旧 `out/macos-S0/` 未被本轮候选覆盖。

# S0 候选交付验证

初始 S0 候选于 2026-11-04 构建；后续镜像、在线播放和一集下载验证见下方 2026-11-07 更新。当前独立交付候选包含下载分组、本地库和“我的”页下载地址设置。DMG 已校验，挂载验收仍未补做。

## 2026-11-07 下载分组与本地库候选

候选目录为 `out/macos-download-library-20261107/`，包含 `HGhub.app`、Apple Silicon DMG、使用说明和验证文档。它没有覆盖 `out/macos-S0/`，测试使用独立数据目录；本轮没有向生产任务或设置文件写入样本数据。

| 检查 | 结果与范围 | 证据 | 把握 |
| --- | --- | --- | --- |
| 下载选择 | 进入选集下载时默认全选，支持“清空”和“整剧全选”；加入任务仍由“下载所选”触发 | `src/components/Panels.tsx`；前端构建通过 | 高 |
| 下载页 | macOS 侧栏独立显示“下载”，任务按短剧分组，活动/等待/失败默认展开，完成/取消默认折叠 | `src/pages/DownloadsPage.tsx`、`src/components/Downloads.tsx`；隔离 App UI 树 | 高 |
| 我下载的 | 独立分页本地库，按短剧聚合已核对回执的 MP4；更新检查只提示远端新增集数，可批量或勾选加入更新任务 | `src/pages/Library.tsx`、`src-tauri/src/downloads/library.rs`；本地 Rust 回归和隔离 App UI | 高，更新检查取决于源站连通性 |
| 我的页下载地址 | 未登录“我的”页也显示默认下载地址，可选择目录并持久化；下载页同步显示同一设置 | `src/components/DownloadLocationSettings.tsx`；隔离候选 App 实际界面 | 高，未在实际账号下操作 |
| 自动回归 | Rust `45 passed, 6 ignored`；严格 Clippy、fmt、TypeScript/Vite 构建和 2 项平台测试通过 | 本轮命令输出；`out/macos-download-library-20261107/BUILDINFO.json` | 高，仅限已运行用例 |
| 包完整性 | App ad-hoc 签名验证通过，主可执行文件为 arm64，内置 FFmpeg 可执行，DMG `hdiutil verify` 通过 | `BUILDINFO.json` | 高，DMG 未做安装挂载验收 |

当前检查结束后，`ps` 未发现 HGhub 进程；旧 App 的下载目录、任务文件和 `out/macos-S0/` 均未被本轮构建覆盖。

## 2026-11-07 可配置下载并发候选

新增并发候选独立保存在 `out/macos-concurrency/`，没有替换正在运行的 `out/macos-S0/`。默认全局 5 个剧集任务、同剧 3 集、每集 4 个分片；设置入口在“我的 → 本地下载 → 下载并发”。v1 设置与任务格式继续兼容，缺少新增字段时使用默认值。

| 检查 | 结果与范围 | 证据 | 把握 |
| --- | --- | --- | --- |
| 自动回归 | 41 项通过，6 项原有外网/人工用例跳过 | `.local/macos-port-20261104/concurrency-full-tests.log` | 高，仅限已运行用例 |
| 实际请求上限 | 本地受控服务观测到 5 个活动任务、同剧最多 3 集、共 20 个分片请求；自定义分片上限 1、2 也生效 | `downloads::tests::default_limits_run_five_tasks_three_per_drama_and_twenty_segments`、`workers_use_custom_one_and_two_segment_limits` | 高，合成受控请求，不代表真实源站能承受任意并发 |
| 运行中降限 | 2 个原有 worker 在降低到全局 1 后继续完成，等待任务在名额释放后按新上限启动；3 集最终 MP4 解码通过 | `lowering_limits_preserves_running_workers_and_drains_before_starting_more` | 高，合成媒体 |
| 持久化与兼容 | 旧设置默认值、未知字段保留、重启读取、同时修改目录/并发不丢字段、无效输入拒绝、未知版本文件保护均通过 | `src-tauri/src/downloads/tests.rs` | 高 |
| 真实 macOS 界面 | 独立应用标识 `com.hghub.concurrency-check` 与独立数据目录；UI 显示旧设置默认 5/3/4，拒绝 0，草稿跨刷新保留，保存 7/2/6 后重启仍显示 7/2/6，改目录保留并发值；CUA 核对三个配置项存在 | `.local/concurrency-smoke-20261107/data/debug.log` 的 `DOWNLOAD_SETTINGS_SMOKE`；第二次运行只检查重启读取 | 高，隔离测试 App |
| 静态检查与构建 | TypeScript/Vite、2 项平台测试、严格 Clippy 通过；普通 arm64 App/DMG 构建成功 | `concurrency-frontend.log`、`concurrency-platform.log`、`concurrency-clippy.log`、`concurrency-release-build.log` | 高 |
| 候选打包 | 候选 App 签名与 DMG 完整性通过，内置 FFmpeg 可执行；正常构建不包含并发 smoke 脚本 | `.local/macos-port-20261104/concurrency-package-checks.json` | 高，未启动生产数据下的候选 App，未新增 DMG 挂载验收 |

设计复核在原 Claude session `92f44804-699f-41d5-a388-52072bec1438` 成功完成；请求 `claude-opus-5-5` 与 effort `xhigh`，返回模型字段匹配，未返回 effort 独立确认。最终实现复核返回 `402：已达到每日费用限制 ($100)`，没有生成审查结论，不能算复核通过。主代理已按代码审查技能检查相关完整文件，未发现已证实的阻塞问题；Claude 最终复核仍未完成，候选未替换旧 App。记录见 `claude-concurrency-review.json`、`claude-concurrency-final-review.json`。

当前已完成独立开发、测试和打包；剩余为恢复 Claude 额度后补齐最终复核，以及当前下载结束后的实际切换。测试进程已关闭；没有使用账号凭据，也没有向真实源站额外发起并发负载测试。

| 检查 | 结果与范围 | 证据 | 把握 |
| --- | --- | --- | --- |
| 前端与平台识别 | TypeScript/Vite 构建通过；macOS 与原平台地址的 2 项测试通过 | `scripts/tests/platform.test.ts`、最终构建日志 | 高 |
| 下载与存储 | 14 项本地 Rust 测试通过；4 项需要源站/人工网络的测试明确忽略 | `src-tauri/src/downloads/tests.rs`、`.local/macos-port-20261104/final-rust-tests.log` | 高，仅限测试场景 |
| 代码检查 | 全目标、全部 feature 的 Clippy 检查通过，警告视为错误 | `.local/macos-port-20261104/final-clippy.log` | 高 |
| 强制退出续传 | 独立进程收到 SIGKILL 后，重新启动并刷新过期地址，复用已校验片段，最终 MP4 解码通过 | `killed_process_resumes_verified_segments_after_signature_refresh` | 高，合成媒体 |
| 任务操作 | 暂停释放名额、保持暂停重启、取消后重试、损坏片段重下、已完成文件缺失后重下、保存后再取消的状态均有回归 | 同一 Rust 测试文件 | 高，合成媒体 |
| 图片缓存 | 8 个同时请求只取一次；清空内存后读磁盘；签名改变不重下 | `concurrent_cover_requests_fetch_once_and_memory_reset_uses_disk` | 高，测试图片 |
| macOS 播放器 | 实际 WKWebView：本地 Range 206/100 字节；MP4 播放及跳转；加密 HLS 经 `hg` 和 hls.js 播放 | `.local/wk-smoke/debug.log` 的 `WK_SMOKE`；CUA 窗口观察 | 高，合成媒体 |
| 界面与目录 | 未登录可访问本地下载；原生目录选择保存成功；换默认目录后重新打开旧目录视频可播放 | 本轮 CUA 窗口观察 | 高，测试目录 |
| 正式 App | arm64；临时签名校验通过；直接启动持续运行，应用数据可创建；默认构建不包含测试脚本 | `.local/macos-port-20261104/release-runtime.json` | 高，启动检查不等于长期稳定性 |
| 内置 FFmpeg | 从最终 App 内实际运行，fMP4 合并成功；H.264 解码 150 帧、6 秒；AAC 解码 283 帧、约 6.021 秒；只链接系统库 | 同一 runtime JSON | 高，合成媒体 |
| DMG | 生成、`hdiutil verify` 校验通过；只读挂载返回“权限被拒绝”，未完成安装过程验证 | `.local/macos-port-20261104/final-dmg-verify.log`、本轮工具输出 | 完整性高，挂载未验证 |
| 真实源站 | 本机直连发生 TLS EOF；现有代理也返回过 CONNECT 503。外部网页抓取能读取首页，不能把本机失败归因为网站整体离线 | 本轮 curl/urllib 诊断；早期研究探针 | 本机失败高，原因未定 |

## 2026-11-07 统一域名路由与下载地址验证

这次修正覆盖用户补充的登录与全量域名切换，以及下载地址和目录命名。站点 origin 的 API、登录、授权请求、下载播放地址刷新统一经过候选域名；媒体 CDN host 保持原样。默认下载地址持久化，选集面板仍支持本次覆盖。

| 检查 | 结果与范围 | 证据 | 把握 |
| --- | --- | --- | --- |
| 统一路由回归 | 29 项 Rust 测试通过；6 项外网/人工用例跳过；登录连接/重定向换源、授权 GET、写请求连接故障、结果未知和令牌媒体隔离均有本地 fixture | `node scripts/macos.mjs cargo test --locked --offline --lib`、`src-tauri/src/net_tests.rs` | 高，仅限 fixture |
| 资源路由 | 精确替换主站/镜像 origin，保留签名路径查询；CDN host 不替换，媒体请求不附加 bearer，使用当前站点 Referer | `src-tauri/src/network_tests.rs`、`src-tauri/src/net.rs`、`src-tauri/src/media.rs` | 高 |
| 下载地址 | 默认地址可持久化；选集本次覆盖不修改默认值；新任务使用 `drama-ID-名字`，标题清理按 UTF-8 字节限制 | `src/components/Downloads.tsx`、`src/components/Panels.tsx`、`src-tauri/src/downloads/models.rs` | 高，自动测试覆盖路径规则 |
| 旧文件兼容 | 已有 `drama-ID` 文件按任务 ID 解析，Range 白名单和离线播放继续使用旧路径；新目录与旧目录同时支持 | `old_id_only_episode_path_is_kept_for_compatibility`、本地媒体路径代码 | 高，测试场景 |
| 实际镜像 | 三个镜像 API 列表 HTTP 200；真实 HLS 播放地址、清单和分片通过；未使用账号凭据 | `live_default_domains_return_video_lists`、`media::tests::live_play_chain` | 高，网络状态可能变化 |
| 静态与构建 | TypeScript/Vite、平台 2 项、严格 Clippy 通过 | 构建输出 | 高 |

登录、注册和真实账号写操作没有在本轮使用用户凭据执行；本地 fixture 验证了相同请求体在登录镜像切换时会转发到下一个已配置 origin。镜像能决定 API 返回的媒体、封面、key 和分片地址，用户应只填写自己认可的镜像。DMG 挂载、其他 Mac、Windows/Android 和未来版本升级仍未新增验收。

## 2026-11-07 备用域名与真实源验证（北京时间）

默认列表为原主域名，加 `olfs.gitiecgad.cc`、`y6wb.gitiecgad.cc`、`omqefh.gitiecgad.cc`。网络设置在未登录和已登录的“我的”页面均有入口；此功能限 macOS。配置单独保存在 v1 `network.json`，下载与会话格式未改变。

| 检查 | 结果与范围 | 证据 | 把握 |
| --- | --- | --- | --- |
| 本地回归 | 26 项通过；6 项外网/人工用例默认跳过。前端构建、2 项平台测试和严格 Clippy 通过 | `.local/macos-port-20261104/mirror-final-tests.log`、`mirror-final-clippy.log`、构建输出 | 高 |
| 切换规则 | 匿名读取按顺序处理传输错误/5xx；4xx、3xx、业务错误和无效 JSON 不自动切换；写操作不重发；令牌不发往第二域名 | `src-tauri/src/net_tests.rs` | 高，仅限覆盖场景 |
| 同域重试与提交状态 | 登录态 GET 的连接故障共尝试三次，不切域名；响应体失败不触发同域重试；写超时与响应中断提示结果未知且只提交一次 | 同一测试文件的 TLS 断连、延迟与截断服务 | 高 |
| 配置兼容与保护 | 规范化 HTTPS、去重、拒绝本地地址；损坏 JSON 保留；未来版本即使字段形状改变也保持原字节，且拒绝旧版保存覆盖 | `src-tauri/src/network_tests.rs` | 高 |
| 三个镜像 | Rust 正式请求层和 App 设置页检测均收到 HTTP 200 与有效视频列表；原主域名多次出现连接超时或 tunnel error，也有一次 App 请求成功 | 显式 live 测试、本轮 CUA 设置页 | 高，网络可随时变化 |
| 正式 App 与强制换源 | 实际打开 `out/macos-S0/HGhub.app`；测试配置首行为不可用 `.invalid` 域名，启动后自动选择 `olfs.gitiecgad.cc`，首页加载，真实播放进度推进至 90 秒。首次打开“我的”即显示正确当前域名和失败/成功记录 | 本轮 CUA；隔离数据在 `.local/mirror-app-runtime-20261107` | 高 |
| 设置保存 | UI 输入裸域名后保存为 HTTPS；恢复主域名加三镜像后退出重启，列表保持，主域名超时后自动切到 olfs，视频播放推进至 25 秒 | 同一隔离目录的 `network.json`、本轮 CUA | 高，已在最终交付 App 验证 |
| 真实剧集下载 | 一集 186 个片段全部下载、解密并合并 MP4；大小 362,608,785 字节，时长约 929.84 秒；全量解码 H.264 27,893 帧、AAC 40,045 帧，无解码错误 | `mirror-real-episode-verified.log`、`mirror-real-episode-probe.json` | 高，仅代表此样本 |
| 当前交付 | arm64 正式 App 签名完整；DMG 完整性校验通过；没有重新做 DMG 挂载验收 | `codesign --verify --deep --strict`、`file`、`hdiutil verify` | 签名与完整性高 |

首次真实下载检查中，下载与合并成功；后续验证命令试图将浮点 AAC 转为 PCM s16，但精简 FFmpeg 不含重采样过滤器，因此该命令失败。没有改变成品或增加 FFmpeg 功能来掩盖它：检查改用 FFprobe 全量解码计帧，并复用已下载文件；修正后的显式测试通过。首次失败日志保留在 `mirror-real-episode.log`。

Claude Code 在原 session `92f44804-699f-41d5-a388-52072bec1438` 完成镜像实现审查和两轮定向核对。请求模型为 `claude-opus-5-5`、effort `xhigh`，返回模型元数据匹配；接口没有返回 effort 确认，不声称后端独立证明。最后复核未发现有证据支持的阻塞缺陷，没有独立运行测试。之后设置页在切入“我的”时刷新域名状态的小修正，由主代理在最终 App 中实际验收。

历史镜像版本边界曾是登录态读取和账号操作仅用首行域名；本次统一路由已覆盖这些请求。镜像可决定媒体、封面、key 与分片地址，填入的域名须由使用者认可。媒体客户端及 CDN URL 没有改写为镜像。启动后若文件被外部破坏，用户显式保存可覆盖该损坏文件；启动时识别到的损坏 JSON 仍先备份。未登录账号、注册或提交评论/点赞；其他 Mac、Windows/Android 运行与未来版本升级没有新增验收。

## 2026-11-06 网络故障复现

重新打开正式 App 后，首页在等待一个请求超时后显示了可读错误和“重试”按钮。App 日志记录：`api_feed/api_category/api_posts` 请求在连接阶段失败，错误为 `tunnel error: unsuccessful`；没有收到源站 HTTP 状态或业务 JSON。

本机当前系统 HTTP/HTTPS 代理是 `127.0.0.1:1082`，由 Shadowrocket 的 PacketTunnel 提供。经该代理访问 Apple 和 Google 成功，访问 `huangguoai.com` 的 HTTPS CONNECT 返回 503。TUN 的本地 DNS 将该域名返回到 `198.18.0.92`；通过公共 DNS 查得的当前地址为 `99.84.41.69/44/9/30`。将其中一个真实地址用于当前连接验证时，代理和直连都能拿到 200，说明网络路径问题集中在该域名的 DNS/代理隧道选择，尚未证明源站接口本身故障。地址会变化，不能把它作为 App 内固定配置。

这次没有修改 Shadowrocket、系统代理或 DNS。修复的是 App 的首屏状态：请求失败时显示原始错误、检查方向和“重试”按钮，避免继续转圈。App 仍使用原来的代理和 TLS 连接方式；用户需要先调整能访问该域名的节点或规则。外部网络请求的结果也可能随节点、TUN 模式和时间变化。

暂停测试重复时发现系统代理会干扰 loopback 请求。诊断后，测试启动脚本仅为测试进程补充本机地址的 `NO_PROXY`；三次相关测试重复通过，完整测试集也通过。App 和系统代理配置未改。测试 HTTP 服务同时改为读取完整请求头。

测试使用 `src-tauri/tests/fixtures/hls/` 的自生成素材，不包含源站视频。真实账号登录、注册、评论、点赞等操作没有执行。Windows/Android 运行、其他 Mac、未来版本升级均未验证。

## 审查结论和剩余限制

Claude Code 同一 session `92f44804-699f-41d5-a388-52072bec1438` 参与了调研方案、总计划、实现审查和修订核对。最后一轮请求为 Opus 5.5、medium effort，返回元数据的模型名为 `claude-opus-5-5`。使用现有网关；这些元数据不构成实际后端模型的独立证明。最后核对基于代码和主代理报告的验证，没有由 Claude 独立运行测试。

最后核对接受了平台入口限制、完成文件缺失后重下、完成后晚到的取消状态修复，以及下面的损坏记录处理范围；没有提出仍需阻止自用候选交付的已证实缺陷。记录在 `.local/macos-port-20261104/claude-11-final-review.json` 与 `claude-12-final-reconcile.json`。

本版遇到整个任务 JSON 损坏时，保留损坏文件、显示提示、保留视频和每集回执；不会自动扫描回执重建列表，需要恢复任务记录。未知版本与额外字段的保留已测试，这不代表未来版本升级已经验收。

HLS 支持点播、AES-128 与不加密、TS/fMP4、密钥更换和初始化片段。直播、DRM、分段字节范围、播放中切换初始化资源、独立音轨会明确报错。视频以原编码保存，不转码。未承诺任意编码均能由 macOS 播放。

App 使用临时签名，无 Apple 公证。优先使用已验证的 `.app`；`.dmg` 是已构建、已校验、未挂载验证的另一交付形式。

并发改动收尾的只读状态核对：旧 App PID 49708 仍在原路径运行，设置文件和二进制哈希与基线相同。检查期间完成记录从 302 增至 331，下载中/等待均归零；原有失败 3 条和取消 11 条未改变。没有据此声称失败任务已成功。证据为 `.local/macos-port-20261104/concurrency-live-baseline.json` 与 `concurrency-live-final.json`。
