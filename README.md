# Hotview · 相册

<p align="center">
  <b>纯 Rust 内核的照片 / 视频查看器</b><br/>
  桌面三端（Windows · macOS · Linux）+ Android（targetSdk 37 / Android 17）
</p>

Hotview 用一个 Rust 核心完成解码、色彩转换与渲染：桌面端使用 **wgpu + FFmpeg**，
Android 端使用 **wgpu(Vulkan) + NDK MediaCodec 硬件解码**，UI 分别是 egui 与
**Jetpack Compose + Material 3 Expressive**（含 Monet 动态取色与 Haze 液态玻璃）。

作者：**Hotsteel** · GitHub：<https://github.com/Hotsteel2901>
许可证：**Apache-2.0**

---

## 特性

### 通用
- 图片解码（纯 Rust `image` crate，自动应用 EXIF 方向）：
  JPEG / PNG / GIF / WebP / BMP / TIFF / ICO / QOI / DDS / EXR / HDR / PNM / TGA / Farbfeld，
  桌面端对 `image` 不认识的格式（AVIF / HEIC / JPEG XL / JP2…）自动回退到 FFmpeg 首帧解码
- 视频解码：桌面端 FFmpeg 全格式；Android 端 MediaCodec 硬件解码
  （H.264 / HEVC / VP8 / VP9 / AV1 / MPEG-4，取决于设备解码器）
- **视频声音**：两端都有声音，音频时钟作为主时钟做音画同步
  - 桌面：FFmpeg 解码 + `swresample` → cpal 输出
  - Android：MediaCodec 音频解码 → AAudio 输出（无锁环形缓冲，AudioFlinger 重采样）
  - 多声道自动下混到立体声；退到后台立即暂停（符合 Android 17 后台音频限制）
- 缩略图后台线程池、原图缩放、加载失败降级
- 仅依赖一个 Rust 核心，桌面与 Android 共享解码/帧转换代码

### 桌面端（egui + wgpu）
- **Hot-Steel 视觉主题**：支持 跟随系统 / 深色 / 浅色 三态切换，自动加载 Windows / macOS / Linux 系统 CJK 字体，无方块乱码
- **多语言**：中文 / English / 日本語 / Deutsch / Русский（自动识别系统语言，也可在设置中随时切换）
- **相册网格**：视口裁剪异步缩略图、LRU 缓存、实时搜索（`Ctrl+F`）、类型筛选（全部 / 照片 / 视频）、自然排序（名称 / 修改时间 / 大小 / 类型）、缩略图尺寸无级调节（滑块或 `Ctrl + 滚轮`）、键盘方向键导航、最近打开文件夹列表
- **查看器**：滚轮光标中心缩放、双击切换 Fit / 2× 放大、拖拽平移、适应（Fit）/ 填满（Fill）/ 1:1、向左/向右 90° 旋转（`R` / `Shift+R`）、水平/垂直翻转（`H` / `V`）、透明背景切换（深色 / 透明棋盘格 / 浅色，`B`）、底部胶片条（`T`）、右侧媒体信息面板（`I`）、复制画面到剪贴板（`Ctrl+C`）、在系统文件管理器中定位、全屏模式（`F11`）
- **视频播放**：播放/暂停、丝滑拖拽/点击进度条、`-5s` / `+5s` 快退快进、循环播放、静音（`M`）与音量滑块、倍速播放（`0.5×`–`2.0×`）、自适应系统音频采样率与声道数
- **跨平台体验**：打开单文件时自动载入同目录相邻媒体、拖拽文件/目录视觉引导遮罩、配置自动持久化、无黑框的 Windows 打包产物（内含完整 FFmpeg 运行库闭包）与签名完整的 macOS / Linux 产物

### Android 端（Compose + M3 Expressive）
- **Material 3 Expressive**：`MaterialExpressiveTheme` + `MotionScheme.expressive()`，
  expressive 圆角体系，动态 Monet 取色（Android 12+ 直接读取壁纸主题，
  低版本回退到内置配色），可切换 跟随系统 / 浅色 / 深色
- **液态玻璃悬浮底栏**：Haze 2 实时模糊 + 滑动指示器 + 拖拽切换 + 按压回弹
- **图标**：自适应图标（前景光圈 + 火焰，夜景渐变背景）+ **monochrome 单色层**，
  Android 13+ 上跟随壁纸实现 MD3E 动态取色（Material You 主题图标）
- **动效**：页面切换共享动效、缩略图错落进场、卡片按压缩放、渐变骨架屏、
  查看器分页缩放淡入、预测性返回（手势进度驱动缩放）、播放/暂停图标弹性切换、
  线性签名绘制动画
- **相册体验**：按日期分组的网格、视频时长角标、信息面板、
  系统照片选择器（无需权限）、`READ_MEDIA_VISUAL_USER_SELECTED` 部分授权处理
- **Android 17 适配**：`compileSdk/targetSdk 37`、强制 edge-to-edge、
  预测性返回默认开启、16 KB 页面对齐链接参数、后台音频严格遵守
  （退到后台立即暂停播放，不使用前台服务）、大屏忽略方向限制
- **多语言**：中文（默认）/ English / 日本語 / Deutsch / Русский，
  通过 Android 13+ 的 per-app 语言设置切换（`locales_config.xml`）

---

## 落地页

`docs/` 是一份纯静态的 GitHub Pages 落地页（零构建、零依赖）：

- **语言**：自动检测浏览器语言，也可手动切换（中文 / English / 日本語 / Deutsch / Русский），
  选择会记住并在下次访问恢复
- **主题**：跟随系统 / 浅色 / 深色 三态循环切换（含 View Transitions 过渡），
  系统主题变化时自动跟随
- **动效**：循环类 —— 漂移光斑、呼吸光圈、火星上升、格式走马灯、手机底栏指示器、
  播放进度与缩放 ping；非线性类 —— 弹簧平滑的 3D 卡片倾斜、指针视差、
  指数缓动的数字增长、带回弹的入场与语言胶囊滑动
- 图标与 OG 图由 `docs/favicon.svg` / `og-image.png` 提供，风格与应用图标一致
  （光圈 + 火焰，MD3E 动态取色）

推送 `docs/` 后由 `.github/workflows/pages.yml` 自动发布。首次使用需在
**Settings → Pages → Build and deployment → Source** 选择 **GitHub Actions**。

在线地址：<https://hotsteel2901.github.io/Hotview/>

---

## 仓库结构

```
crates/
  hotview-core/      # 解码核心：图片(image)、视频(FFmpeg / MediaCodec)、帧与色彩转换
  hotview-render/    # wgpu 渲染器：RGBA 与 YUV(I420/NV12) 着色器、缩放/平移变换
  hotview-desktop/   # egui 桌面应用（缩略图线程池、播放器线程、音频输出）
android/
  rust/           # JNI bridge + 渲染线程（cdylib: libhotview_android.so）
  app/            # Kotlin / Compose 应用
.github/workflows/
  desktop.yml     # Windows / macOS(arm64, x86_64) / Linux
  android.yml     # debug APK / release APK / release AAB
```

渲染管线：解码器输出 `MediaFrame`（RGBA 或 YUV 4:2:0），
`hotview-render` 在 GPU 上做 BT.601/709/2020 + 有限/完整范围的色彩矩阵变换，
`fit_transform()` 统一处理 contain 适配与平移钳制，桌面端与 Android 端行为一致。

---

## 桌面端构建

依赖：Rust stable、FFmpeg 开发库、pkg-config；Linux 额外需要 ALSA 开发库。

```bash
# Linux (Debian/Ubuntu)
sudo apt install pkg-config libavcodec-dev libavformat-dev libavutil-dev \
     libswscale-dev libswresample-dev libavfilter-dev libavdevice-dev libasound2-dev

# macOS
brew install ffmpeg pkg-config

# Windows
# 建议使用 MSYS2：pacman -S mingw-w64-x86_64-ffmpeg mingw-w64-x86_64-pkgconf
# 并使用 x86_64-pc-windows-gnu 目标构建：
#   rustup target add x86_64-pc-windows-gnu
#   cargo build --release --target x86_64-pc-windows-gnu

cargo build --release -p hotview-desktop
./target/release/hotview            # 或 hotview <文件/目录>
```

CI（`.github/workflows/desktop.yml`）会自动为以下平台出包：
`hotview-linux-x86_64.tar.gz`、`hotview-macos-arm64.tar.gz`、`hotview-macos-x86_64.tar.gz`、
`hotview-windows-x86_64.zip`（Windows 包内含所需 DLL）。

## Android 构建

本机需要：JDK 17+、Android SDK（platform `android-37.0`、build-tools 36）、
NDK r28c、`cargo-ndk`、Rust 目标 `aarch64-linux-android` 等。

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo install cargo-ndk

cd android
ANDROID_NDK_HOME=$ANDROID_HOME/ndk/28.2.13676358 ./gradlew :app:assembleDebug
```

不想编译 Rust 时可用 `-PskipRust=true` 只构建 Kotlin 层。
签名：项目使用两个密钥库（release + debug），密码 / 别名 / 密钥密码默认全部为
`hotsteel`。**密钥库永远不会提交到仓库**（`android/keystore/*.jks` 已 gitignore）：

- CI：把两个 jks 做 base64 后放进仓库 Secrets
  （`HOTVIEW_KEYSTORE_BASE64`、`HOTVIEW_DEBUG_KEYSTORE_BASE64`），
  工作流只在 runner 内解码使用；没有密钥时 release 会回退到 debug 签名，
  产物依然可以安装，只是不能上架。
- 本地：设置 `HOTVIEW_KEYSTORE` / `HOTVIEW_DEBUG_KEYSTORE` 环境变量，
  或使用已 gitignore 的 `android/keystore.properties`。
- 细节见 [`android/keystore/README.md`](android/keystore/README.md)。

CI（`.github/workflows/android.yml`）会产出 debug APK、release APK 与 release AAB。

## 已知限制

- Android 上 Rust 解码器不认识的图片格式（如 HEIC/AVIF）会回退到系统 `ImageDecoder`，
  再以 RGBA 上屏；桌面端这类格式由 FFmpeg 兜底。
- 桌面端音频输出依赖系统默认设备；没有可用设备时自动回退到墙钟节奏播放。
- 音频解码失败（极端编码或损坏文件）时只静音播放视频，不会中断播放。
- Android 端硬件解码使用 MediaCodec ByteBuffer + flexible YUV；
  极少数仅支持 Surface 输出的编码器可能无法播放（会给出错误提示）。
- 动态图（GIF/WebP 动画）目前显示首帧。

## 致谢

- [FFmpeg](https://ffmpeg.org/)（桌面解码）
- [wgpu](https://wgpu.rs/) · [egui](https://github.com/emilk/egui)
- [Haze](https://github.com/chrisbanes/haze)（液态玻璃模糊，Apache-2.0）
- [Jetpack Compose](https://developer.android.com/compose) · Material 3 Expressive
- 悬浮底栏结构参考 [ReSukiSU](https://github.com/ReSukiSU/ReSukiSU) 管理器
  （Apache-2.0）与 miuix 液态玻璃导航栏

## License

Apache License 2.0，详见 [LICENSE](LICENSE)。
