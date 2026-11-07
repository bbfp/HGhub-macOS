<div align="center">

# HGhub macOS

**黄果短剧第三方客户端 · Rust + Tauri 2**

macOS（Apple Silicon）/ Windows / Android · Fluent 设计 · 沉浸式竖屏播放
![alt text](docs/image.png)

</div>

本项目基于 [Minireel/HGhub](https://github.com/Minireel/HGhub) 的 v1.0.1 源码（提交 `e4ab246f0ff8d3d68da97130b515998ea57d93fb`），增加 Apple Silicon macOS 适配和本地下载能力。原有 Windows/Android 代码保留；本分支主要验证 macOS，其他平台未新增运行验收。

许可基线为上述固定提交的 [MIT LICENSE](https://github.com/Minireel/HGhub/blob/e4ab246f0ff8d3d68da97130b515998ea57d93fb/LICENSE)。上游从 v1.1.0 起改用 PolyForm Noncommercial，且其 [变更说明](https://github.com/Minireel/HGhub/commit/d54ad6ff7a6e0a7d314a9615717b835fad760228) 明确 v1.0.1 及更早版本仍按 MIT 授权；本分支没有合入该后续版本。

使用说明见 [macOS 开始使用](docs/macos/START-HERE.md)，开发环境和 FFmpeg 准备见 [构建说明](docs/macos/BUILD.md)，实际测试范围见 [验证记录](docs/macos/VALIDATION.md)。完成依赖准备后运行 `pnpm macos:build` 生成 `.app` 和 `.dmg`。

## macOS 新增功能

- 逐集勾选下载和“整剧全选”，下载时可覆盖保存目录；“我的”页面可设置默认目录。
- 全局任务、同剧集数、每集分片三层并发可手动配置，默认 **5 / 3 / 4**，默认配置下最多同时下载 20 个分片。
- 下载任务持久化，支持暂停、继续、取消、失败重试和退出后恢复；完成文件保存为 MP4，可离线播放。
- 封面磁盘缓存，减少重复请求；兼容旧版 `drama-ID` 目录，新任务可使用 `drama-ID-名字`。
- 可编辑备用域名列表；登录、授权读取、下载地址刷新和站点资源共用域名切换逻辑。

> ⚠️ 本项目仅供 **学习与技术交流**，不提供、不存储、不分发任何音视频内容。
> 所有内容均来自第三方站点，请自行判断合规性，并在 24 小时内删除相关资源。


## 技术栈

| 层 | 选型 |
| --- | --- |
| 壳 | Tauri 2（Rust 1.9x） |
| 前端 | React 19 + TypeScript + Vite 7 + Tailwind CSS 4 |
| 播放 | hls.js（HLS / AES-128） |
| 网络 | reqwest + rustls |
| 图标 | 内联 SVG |

## 目录

```
.
├─ src/                      前端
│  ├─ api.ts                 接口封装 + 本地媒体协议 URL
│  ├─ store.tsx              全局状态（导航栈 / 用户 / 本地收藏 / 主题）
│  ├─ html.ts                吃瓜正文清洗与图片代理
│  ├─ components/            Feed（播放页）/ VideoSurface / PlayerControls / Panels / UI
│  └─ pages/                 Home / Search / Category / Profile / Posts
├─ src-tauri/                Rust 后端
│  └─ src/
│     ├─ net.rs              请求层（边缘信封 + 重试）
│     ├─ api.rs              业务命令
│     ├─ media.rs            封面解密 + HLS 代理 + 缓存
│     └─ session.rs          token 持久化
└─ docs/API.md               接口文档
```

## 开发 / 构建

```bash
pnpm install          # 首次安装
pnpm app:dev          # 桌面端热重载
pnpm app:build        # 打包 Windows 安装程序（NSIS）
```

Android（需 Android SDK + NDK，并配置 `ANDROID_HOME` / `NDK_HOME`）：

```bash
pnpm android:init     # 首次生成 android 工程
pnpm android:dev      # 真机 / 模拟器调试
pnpm android:build    # 产出 APK
```

### 环境注意

- 工程若放在 **exFAT** 盘（不支持符号链接），需保留 `.npmrc` 中的 `node-linker=hoisted`。
- `esbuild` 需在 `package.json` 的 `pnpm.onlyBuiltDependencies` 中放行，否则 `vite build` 缺少二进制。

## 实现要点

1. **统一边缘信封**：站点把 `/api/**` 收敛为 `POST /api/edge/request`（`method/path/body(base64)/content_type` + `Authorization: Bearer`），本项目只对接这一个入口，无需处理 Cookie。
2. **封面解密**：封面为 AES-128-CBC 密文，Rust 侧解密后经自定义协议输出，前端 `<img>` 直接使用。
3. **HLS 代理**：m3u8 与分片经自定义协议转发并改写 `#EXT-X-KEY` / 分片地址，绕开 WebView 的 CORS 与防盗链。
4. **自定义协议**：Windows 为 `http://hg.localhost/*`，Android 为 `https://hg.localhost/*`（由 Rust 侧按平台生成，避免 WebView 无法解析）。
5. **不做签名逆向**：只复用浏览器同源行为，接口字段均有文档与实测样本。

接口细节见 [docs/API.md](docs/API.md)。

## 免责声明

- 本项目与内容提供方无任何关联，作者不参与内容的制作、上传与分发。
- 使用者应自行遵守所在地法律法规；因使用本项目产生的任何后果由使用者自行承担。
- 若相关权利方认为本项目存在侵权，请通过 Issue 联系，我们会及时处理。

## License

[MIT](LICENSE)
## 常见问题

**1. `pnpm install` 报 `ERR_PNPM_EISDIR ... symlink`**

工程放在 exFAT / FAT32 盘时无法创建符号链接，保留 `.npmrc` 里的 `node-linker=hoisted` 即可。

**2. `tauri android build` 报 `Failed to create a symbolic link ... jniLibs`**

同样是 exFAT 不支持符号链接。可手动把 Rust 产物拷进 jniLibs，再直接跑 Gradle：

```bash
cp src-tauri/target/aarch64-linux-android/debug/libhuangguo_client_lib.so \
   src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a/

cd src-tauri/gen/android
./gradlew assembleDebug        # Windows: gradlew.bat assembleDebug
```

产物在 `src-tauri/gen/android/app/build/outputs/apk/`。

**3. Android 构建提示找不到 SDK / NDK**

需要设置 `ANDROID_HOME`（Android SDK 根目录）、`NDK_HOME`（`$ANDROID_HOME/ndk/<版本>`）与 `JAVA_HOME`（JDK 17）。

## LinuxDO
https://linux.do/


## 致谢

感谢 [Minireel/HGhub](https://github.com/Minireel/HGhub) 及其贡献者提供原始客户端、界面、接口实现和项目基础。本仓库是在其基础上的 macOS 适配与下载功能扩展，保留上游提交历史及原始 [MIT License](LICENSE)。

同时感谢 [Tauri](https://github.com/tauri-apps/tauri)、[React](https://github.com/facebook/react)、[hls.js](https://github.com/video-dev/hls.js) 和 [FFmpeg](https://github.com/FFmpeg/FFmpeg) 等项目。FFmpeg 采用独立许可证，构建配置、来源和许可证记录见 [third-party](docs/macos/third-party/)。
