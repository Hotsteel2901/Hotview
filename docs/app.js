/* ==========================================================================
   Hotview landing page
   - language: auto-detect from the browser, manual switch, remembered
   - theme:    auto (system) / light / dark, remembered, View-Transition aware
   - motion:   scroll reveals, spring-smoothed tilt, parallax, counters
   ========================================================================== */
(() => {
  "use strict";

  const SUPPORTED = ["zh", "en", "ja", "de", "ru"];
  const LS_LANG = "hotview.lang";
  const LS_THEME = "hotview.theme";
  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ----------------------------- translations ---------------------------- */
  const I18N = {
    zh: {
      "page.title": "Hotview · 纯 Rust 照片与视频查看器",
      "page.desc": "Hotview —— 一颗 Rust 核心驱动的照片与视频查看器，桌面三端与 Android 17 同源。",
      "nav.features": "功能", "nav.formats": "格式", "nav.pipeline": "架构",
      "nav.showcase": "界面", "nav.download": "下载", "nav.source": "源码",
      "hero.badge": "纯 Rust 内核 · 零妥协",
      "hero.title": "Hotview",
      "hero.tagline": "照片与视频，交给一颗 Rust 心脏。",
      "hero.desc": "解码、色彩转换与渲染全部在 Rust 核心完成：桌面端 wgpu + FFmpeg，Android 端 wgpu(Vulkan) + MediaCodec 硬件解码与 AAudio 音频输出。UI 是 egui 与 Jetpack Compose + Material 3 Expressive。",
      "hero.download": "立即下载", "hero.source": "在 GitHub 上查看",
      "stats.formats": "图片 / 视频格式", "stats.platforms": "平台（含 3 种 ABI）",
      "stats.rust": "Rust 解码与渲染", "stats.license": "开源许可证",
      "features.title": "为什么是 Hotview",
      "features.subtitle": "一颗 Rust 核心，两端原生体验。",
      "f1.title": "Rust 解码核心",
      "f1.desc": "image crate 覆盖常见图片格式，FFmpeg 兜底 AVIF / HEIC / JPEG XL；桌面与 Android 共用同一套帧与色彩管线。",
      "f2.title": "wgpu 渲染器",
      "f2.desc": "RGBA 与 I420 / NV12 着色器在 GPU 上完成 BT.601/709/2020 色彩矩阵，contain 适配与平移钳制两端行为一致。",
      "f3.title": "硬件解码 + 真声音",
      "f3.desc": "Android 走 MediaCodec 视频/音频与 AAudio 无锁输出；桌面走 FFmpeg + swresample + cpal。音频时钟做音画同步。",
      "f4.title": "Material 3 Expressive",
      "f4.desc": "Monet 莫奈取色、液态玻璃悬浮底栏、预测性返回、错落进场与弹性动效，界面会呼吸。",
      "f5.title": "处处皆原生",
      "f5.desc": "Windows / macOS(arm64 · x86_64) / Linux / Android 五份产物由 CI 自动出包，Android 覆盖 arm64、armv7、x86_64。",
      "f6.title": "本地 · 隐私 · 开源",
      "f6.desc": "不联网、不上传。Android 支持系统照片选择器与部分授权，仓库以 Apache-2.0 完全开放。",
      "formats.title": "格式广泛支持",
      "formats.subtitle": "从手机相册到专业素材，打开就能看。",
      "pipeline.title": "解码 → 变换 → 上屏",
      "pipeline.subtitle": "一帧的旅程，全程在 Rust 里。",
      "p1.title": "解码", "p1.desc": "image / FFmpeg / MediaCodec 输出 RGBA 或 YUV 4:2:0 帧，硬件解码优先。",
      "p2.title": "色彩变换", "p2.desc": "GPU 着色器完成矩阵与量程转换，CPU 端仅在必要时做多声道下混与缩放。",
      "p3.title": "上屏", "p3.desc": "wgpu 把纹理合成到 ANativeWindow 或 egui 视图，缩放、平移与分页动效都在 GPU 完成。",
      "showcase.title": "两端同源，各自原生",
      "showcase.subtitle": "Android 端的相册、播放器与桌面端查看器。",
      "showcase.gallery": "相册", "showcase.gallerySub": "12 个相册 · 486 个项目",
      "tabs.albums": "相册", "tabs.photos": "照片", "tabs.picked": "已选择",
      "showcase.open": "打开文件夹…", "showcase.prev": "◀ 上一张", "showcase.next": "下一张 ▶",
      "showcase.fit": "适应", "showcase.video": "video_2026.mp4",
      "download.title": "下载 Hotview",
      "download.subtitle": "CI 为每个平台自动构建，全部开源。",
      "download.note": "全部产物见 GitHub Releases；Android release 由 CI 使用项目密钥签名。",
      "footer.note": "Hotview 的作者 · 独立开发者", "footer.repo": "项目仓库",
      "footer.made": "Made with Rust + wgpu",
      "theme.toggle": "主题：{mode}（点击切换）",
      "theme.auto": "跟随系统", "theme.light": "浅色", "theme.dark": "深色",
      "lang.switch": "切换到 {name}",
    },
    en: {
      "page.title": "Hotview · Photo & video viewer with a pure Rust core",
      "page.desc": "Hotview — a photo & video viewer powered by a pure Rust core, on desktop and Android 17.",
      "nav.features": "Features", "nav.formats": "Formats", "nav.pipeline": "Pipeline",
      "nav.showcase": "Screens", "nav.download": "Download", "nav.source": "Source",
      "hero.badge": "Pure Rust core · no compromise",
      "hero.title": "Hotview",
      "hero.tagline": "Photos and videos, handled by a Rust heart.",
      "hero.desc": "Decoding, colour conversion and rendering all happen in the Rust core: wgpu + FFmpeg on desktop, wgpu (Vulkan) + MediaCodec hardware decoding and AAudio output on Android. The UI is egui and Jetpack Compose + Material 3 Expressive.",
      "hero.download": "Download now", "hero.source": "View on GitHub",
      "stats.formats": "image / video formats", "stats.platforms": "platforms (3 ABIs)",
      "stats.rust": "Rust decoding & rendering", "stats.license": "open-source licence",
      "features.title": "Why Hotview",
      "features.subtitle": "One Rust core, two native experiences.",
      "f1.title": "Rust decoding core",
      "f1.desc": "The image crate covers common formats; FFmpeg backs AVIF / HEIC / JPEG XL. Desktop and Android share the same frame and colour pipeline.",
      "f2.title": "wgpu renderer",
      "f2.desc": "RGBA and I420 / NV12 shaders apply BT.601/709/2020 matrices on the GPU, with identical contain fitting and pan clamping everywhere.",
      "f3.title": "Hardware decoding + real audio",
      "f3.desc": "Android uses MediaCodec video/audio plus lock-free AAudio output; desktop uses FFmpeg + swresample + cpal. The audio clock drives A/V sync.",
      "f4.title": "Material 3 Expressive",
      "f4.desc": "Monet dynamic colour, a liquid-glass floating bottom bar, predictive back, staggered reveals and springy motion — the UI breathes.",
      "f5.title": "Native everywhere",
      "f5.desc": "Windows / macOS (arm64 · x86_64) / Linux / Android builds are produced by CI, with arm64, armv7 and x86_64 Android ABIs.",
      "f6.title": "Local · private · open",
      "f6.desc": "No network, no uploads. Android supports the system photo picker and partial access; the whole project is Apache-2.0.",
      "formats.title": "Broad format support",
      "formats.subtitle": "From phone snapshots to professional assets — just open it.",
      "pipeline.title": "Decode → convert → present",
      "pipeline.subtitle": "One frame's journey, entirely in Rust.",
      "p1.title": "Decode", "p1.desc": "image / FFmpeg / MediaCodec produce RGBA or YUV 4:2:0 frames, preferring hardware decoders.",
      "p2.title": "Colour conversion", "p2.desc": "GPU shaders handle matrices and ranges; the CPU only downmixes audio or rescales when needed.",
      "p3.title": "Present", "p3.desc": "wgpu composites to an ANativeWindow or the egui view — zoom, pan and pager motion all on the GPU.",
      "showcase.title": "One core, two native faces",
      "showcase.subtitle": "The Android gallery and player, and the desktop viewer.",
      "showcase.gallery": "Albums", "showcase.gallerySub": "12 albums · 486 items",
      "tabs.albums": "Albums", "tabs.photos": "Photos", "tabs.picked": "Selected",
      "showcase.open": "Open folder…", "showcase.prev": "◀ Previous", "showcase.next": "Next ▶",
      "showcase.fit": "Fit", "showcase.video": "video_2026.mp4",
      "download.title": "Download Hotview",
      "download.subtitle": "CI builds every platform automatically. All open source.",
      "download.note": "All artifacts live in GitHub Releases; Android release builds are signed with the project keystore in CI.",
      "footer.note": "Author of Hotview · indie developer", "footer.repo": "Repository",
      "footer.made": "Made with Rust + wgpu",
      "theme.toggle": "Theme: {mode} (click to cycle)",
      "theme.auto": "System", "theme.light": "Light", "theme.dark": "Dark",
      "lang.switch": "Switch to {name}",
    },
    ja: {
      "page.title": "Hotview · 純 Rust の写真・動画ビューア",
      "page.desc": "Hotview — Rust コアで動く写真・動画ビューア。デスクトップと Android 17 に対応。",
      "nav.features": "機能", "nav.formats": "形式", "nav.pipeline": "構成",
      "nav.showcase": "画面", "nav.download": "ダウンロード", "nav.source": "ソース",
      "hero.badge": "純 Rust コア・妥協なし",
      "hero.title": "Hotview",
      "hero.tagline": "写真も動画も、Rust の心臓で。",
      "hero.desc": "デコード、色変換、レンダリングはすべて Rust コアで実行。デスクトップは wgpu + FFmpeg、Android は wgpu(Vulkan) + MediaCodec のハードウェアデコードと AAudio 出力。UI は egui と Jetpack Compose + Material 3 Expressive。",
      "hero.download": "ダウンロード", "hero.source": "GitHub で見る",
      "stats.formats": "画像 / 動画フォーマット", "stats.platforms": "対応プラットフォーム（3 ABI）",
      "stats.rust": "Rust でデコードと描画", "stats.license": "オープンソースライセンス",
      "features.title": "Hotview の理由",
      "features.subtitle": "ひとつの Rust コア、ふたつのネイティブ体験。",
      "f1.title": "Rust デコードコア",
      "f1.desc": "image crate が一般的な形式を担当し、AVIF / HEIC / JPEG XL は FFmpeg が補完。デスクトップと Android で同じフレーム・色パイプラインを共有します。",
      "f2.title": "wgpu レンダラー",
      "f2.desc": "RGBA と I420 / NV12 のシェーダーが GPU 上で BT.601/709/2020 の行列を適用。contain 配置とパン制限も両者で同一です。",
      "f3.title": "ハードウェアデコードと本物の音",
      "f3.desc": "Android は MediaCodec と AAudio のロックフリー出力、デスクトップは FFmpeg + swresample + cpal。音声クロックで A/V 同期します。",
      "f4.title": "Material 3 Expressive",
      "f4.desc": "Monet の動的カラー、液体ガラスのフローティングバー、予測型戻る、ずらし表示とバネの動き。UI が呼吸します。",
      "f5.title": "どこでもネイティブ",
      "f5.desc": "Windows / macOS(arm64 · x86_64) / Linux / Android の成果物を CI が自動ビルド。Android は arm64・armv7・x86_64 に対応。",
      "f6.title": "ローカル・プライベート・オープン",
      "f6.desc": "通信もアップロードもしません。Android はフォトピッカーと部分アクセスに対応、プロジェクトは Apache-2.0 です。",
      "formats.title": "幅広いフォーマット対応",
      "formats.subtitle": "スマホの写真からプロ素材まで、開くだけ。",
      "pipeline.title": "デコード → 変換 → 表示",
      "pipeline.subtitle": "1 フレームの旅は、すべて Rust の中で。",
      "p1.title": "デコード", "p1.desc": "image / FFmpeg / MediaCodec が RGBA または YUV 4:2:0 を出力。ハードウェアデコーダーを優先します。",
      "p2.title": "色変換", "p2.desc": "行列とレンジの変換は GPU シェーダーで。CPU は必要なときだけ音声ダウンミックスや縮小を行います。",
      "p3.title": "表示", "p3.desc": "wgpu が ANativeWindow や egui ビューへ合成。ズーム、パン、ページャーの動きも GPU 上で完結します。",
      "showcase.title": "同じコア、それぞれのネイティブ",
      "showcase.subtitle": "Android のギャラリーとプレイヤー、デスクトップのビューア。",
      "showcase.gallery": "アルバム", "showcase.gallerySub": "12 個のアルバム · 486 件",
      "tabs.albums": "アルバム", "tabs.photos": "写真", "tabs.picked": "選択済み",
      "showcase.open": "フォルダーを開く…", "showcase.prev": "◀ 前へ", "showcase.next": "次へ ▶",
      "showcase.fit": "フィット", "showcase.video": "video_2026.mp4",
      "download.title": "Hotview をダウンロード",
      "download.subtitle": "CI が各プラットフォームを自動ビルド。すべてオープンソース。",
      "download.note": "成果物は GitHub Releases にあります。Android の release は CI でプロジェクト鍵により署名されます。",
      "footer.note": "Hotview の作者 · 個人開発者", "footer.repo": "リポジトリ",
      "footer.made": "Made with Rust + wgpu",
      "theme.toggle": "テーマ: {mode}（クリックで切替）",
      "theme.auto": "システム", "theme.light": "ライト", "theme.dark": "ダーク",
      "lang.switch": "{name} に切り替え",
    },
    de: {
      "page.title": "Hotview · Foto- und Videobetrachter mit reinem Rust-Kern",
      "page.desc": "Hotview — ein Foto- und Videobetrachter mit reinem Rust-Kern, für Desktop und Android 17.",
      "nav.features": "Funktionen", "nav.formats": "Formate", "nav.pipeline": "Pipeline",
      "nav.showcase": "Ansichten", "nav.download": "Download", "nav.source": "Quellcode",
      "hero.badge": "Reiner Rust-Kern · kompromisslos",
      "hero.title": "Hotview",
      "hero.tagline": "Fotos und Videos, von einem Rust-Herzen getragen.",
      "hero.desc": "Dekodierung, Farbkonvertierung und Rendering laufen im Rust-Kern: wgpu + FFmpeg auf dem Desktop, wgpu (Vulkan) + MediaCodec-Hardwaredekodierung und AAudio auf Android. Die UI ist egui bzw. Jetpack Compose + Material 3 Expressive.",
      "hero.download": "Jetzt herunterladen", "hero.source": "Auf GitHub ansehen",
      "stats.formats": "Bild- / Videoformate", "stats.platforms": "Plattformen (3 ABIs)",
      "stats.rust": "Dekodierung & Rendering in Rust", "stats.license": "Open-Source-Lizenz",
      "features.title": "Warum Hotview",
      "features.subtitle": "Ein Rust-Kern, zwei native Erlebnisse.",
      "f1.title": "Rust-Dekodierkern",
      "f1.desc": "Die image-Crate deckt gängige Formate ab, FFmpeg übernimmt AVIF / HEIC / JPEG XL. Desktop und Android teilen dieselbe Frame- und Farbpipeline.",
      "f2.title": "wgpu-Renderer",
      "f2.desc": "RGBA- und I420-/NV12-Shader berechnen BT.601/709/2020-Matrizen auf der GPU, mit identischem Contain-Fitting und Pan-Begrenzung.",
      "f3.title": "Hardware-Dekodierung + echter Ton",
      "f3.desc": "Android nutzt MediaCodec für Video/Audio und lockere AAudio-Ausgabe; Desktop nutzt FFmpeg + swresample + cpal. Die Audiouhr steuert die A/V-Synchronisation.",
      "f4.title": "Material 3 Expressive",
      "f4.desc": "Monet-Farben, schwebende Liquid-Glass-Leiste, Predictive Back, gestaffelte Auftritte und federnde Bewegung — die UI atmet.",
      "f5.title": "Überall nativ",
      "f5.desc": "Windows / macOS (arm64 · x86_64) / Linux / Android werden von CI gebaut, inklusive arm64, armv7 und x86_64 für Android.",
      "f6.title": "Lokal · privat · offen",
      "f6.desc": "Kein Netzwerk, keine Uploads. Android unterstützt den System-Fotopicker und Teilzugriff; das Projekt ist Apache-2.0.",
      "formats.title": "Breite Formatunterstützung",
      "formats.subtitle": "Von Handyfotos bis Profi-Material — einfach öffnen.",
      "pipeline.title": "Dekodieren → konvertieren → zeigen",
      "pipeline.subtitle": "Die Reise eines Frames, komplett in Rust.",
      "p1.title": "Dekodieren", "p1.desc": "image / FFmpeg / MediaCodec liefern RGBA- oder YUV-4:2:0-Frames, Hardware-Decoder bevorzugt.",
      "p2.title": "Farbkonvertierung", "p2.desc": "GPU-Shader übernehmen Matrizen und Wertebereiche; die CPU mischt nur bei Bedarf Audio herunter oder skaliert.",
      "p3.title": "Darstellen", "p3.desc": "wgpu komponiert in ein ANativeWindow oder die egui-Ansicht — Zoom, Pan und Pager-Animationen laufen auf der GPU.",
      "showcase.title": "Ein Kern, zwei native Gesichter",
      "showcase.subtitle": "Die Android-Galerie und der Player sowie der Desktop-Betrachter.",
      "showcase.gallery": "Alben", "showcase.gallerySub": "12 Alben · 486 Elemente",
      "tabs.albums": "Alben", "tabs.photos": "Fotos", "tabs.picked": "Ausgewählt",
      "showcase.open": "Ordner öffnen…", "showcase.prev": "◀ Zurück", "showcase.next": "Weiter ▶",
      "showcase.fit": "Anpassen", "showcase.video": "video_2026.mp4",
      "download.title": "Hotview herunterladen",
      "download.subtitle": "CI baut jede Plattform automatisch. Alles Open Source.",
      "download.note": "Alle Artefakte liegen in den GitHub Releases; Android-Release-Builds werden in CI mit dem Projekt-Keystore signiert.",
      "footer.note": "Autor von Hotview · Indie-Entwickler", "footer.repo": "Repository",
      "footer.made": "Made with Rust + wgpu",
      "theme.toggle": "Design: {mode} (zum Wechseln klicken)",
      "theme.auto": "System", "theme.light": "Hell", "theme.dark": "Dunkel",
      "lang.switch": "Zu {name} wechseln",
    },
    ru: {
      "page.title": "Hotview · просмотрщик фото и видео на чистом Rust",
      "page.desc": "Hotview — просмотрщик фото и видео с ядром на чистом Rust, для компьютеров и Android 17.",
      "nav.features": "Возможности", "nav.formats": "Форматы", "nav.pipeline": "Архитектура",
      "nav.showcase": "Экраны", "nav.download": "Скачать", "nav.source": "Исходники",
      "hero.badge": "Ядро на чистом Rust · без компромиссов",
      "hero.title": "Hotview",
      "hero.tagline": "Фото и видео — под управлением сердца на Rust.",
      "hero.desc": "Декодирование, преобразование цвета и рендеринг выполняются в ядре на Rust: wgpu + FFmpeg на компьютере, wgpu (Vulkan) + аппаратный MediaCodec и вывод AAudio на Android. Интерфейс — egui и Jetpack Compose + Material 3 Expressive.",
      "hero.download": "Скачать", "hero.source": "Открыть на GitHub",
      "stats.formats": "форматов изображений / видео", "stats.platforms": "платформ (3 ABI)",
      "stats.rust": "декодирование и рендеринг на Rust", "stats.license": "лицензия",
      "features.title": "Почему Hotview",
      "features.subtitle": "Одно ядро на Rust — два нативных опыта.",
      "f1.title": "Ядро декодирования на Rust",
      "f1.desc": "Крейт image покрывает популярные форматы, FFmpeg берёт на себя AVIF / HEIC / JPEG XL. Компьютер и Android используют один конвейер кадров и цвета.",
      "f2.title": "Рендерер на wgpu",
      "f2.desc": "Шейдеры RGBA и I420 / NV12 считают матрицы BT.601/709/2020 на GPU, а вписывание и ограничение панорамирования везде одинаковы.",
      "f3.title": "Аппаратное декодирование и настоящий звук",
      "f3.desc": "На Android — MediaCodec для видео/аудио и вывод AAudio без блокировок; на компьютере — FFmpeg + swresample + cpal. Аудиочасы задают синхронизацию.",
      "f4.title": "Material 3 Expressive",
      "f4.desc": "Динамические цвета Monet, стеклянная плавающая панель, предсказуемый возврат, каскадные появления и пружинная анимация — интерфейс дышит.",
      "f5.title": "Нативно везде",
      "f5.desc": "Windows / macOS (arm64 · x86_64) / Linux / Android собираются в CI, включая arm64, armv7 и x86_64 для Android.",
      "f6.title": "Локально · приватно · открыто",
      "f6.desc": "Без сети и загрузок. Android поддерживает системный выбор фото и частичный доступ, проект полностью открыт под Apache-2.0.",
      "formats.title": "Широкая поддержка форматов",
      "formats.subtitle": "От снимков с телефона до профессиональных материалов — просто откройте.",
      "pipeline.title": "Декодирование → преобразование → показ",
      "pipeline.subtitle": "Путь одного кадра — полностью на Rust.",
      "p1.title": "Декодирование", "p1.desc": "image / FFmpeg / MediaCodec выдают кадры RGBA или YUV 4:2:0, предпочитая аппаратные декодеры.",
      "p2.title": "Преобразование цвета", "p2.desc": "Матрицы и диапазоны считают шейдеры GPU; процессор лишь микширует звук или масштабирует при необходимости.",
      "p3.title": "Показ", "p3.desc": "wgpu компонует в ANativeWindow или представление egui — масштаб, панорамирование и анимации страниц идут на GPU.",
      "showcase.title": "Одно ядро — два нативных лица",
      "showcase.subtitle": "Галерея и плеер на Android, просмотрщик на компьютере.",
      "showcase.gallery": "Альбомы", "showcase.gallerySub": "12 альбомов · 486 элементов",
      "tabs.albums": "Альбомы", "tabs.photos": "Фото", "tabs.picked": "Выбранные",
      "showcase.open": "Открыть папку…", "showcase.prev": "◀ Назад", "showcase.next": "Вперёд ▶",
      "showcase.fit": "Вписать", "showcase.video": "video_2026.mp4",
      "download.title": "Скачать Hotview",
      "download.subtitle": "CI собирает каждую платформу автоматически. Всё открыто.",
      "download.note": "Все сборки — в GitHub Releases; release для Android подписывается ключом проекта в CI.",
      "footer.note": "Автор Hotview · независимый разработчик", "footer.repo": "Репозиторий",
      "footer.made": "Made with Rust + wgpu",
      "theme.toggle": "Тема: {mode} (нажмите для смены)",
      "theme.auto": "Как в системе", "theme.light": "Светлая", "theme.dark": "Тёмная",
      "lang.switch": "Переключить на {name}",
    },
  };

  const LANG_NAMES = { zh: "中文", en: "English", ja: "日本語", de: "Deutsch", ru: "Русский" };

  /* ------------------------------- language ------------------------------ */
  function detectLang() {
    let saved = null;
    try { saved = localStorage.getItem(LS_LANG); } catch (_) {}
    if (saved && SUPPORTED.includes(saved)) return saved;

    const tags = (navigator.languages && navigator.languages.length)
      ? navigator.languages
      : [navigator.language || "en"];
    for (const tag of tags) {
      const base = String(tag).toLowerCase().split("-")[0];
      if (base === "zh") return "zh";
      if (SUPPORTED.includes(base)) return base;
    }
    return "en";
  }

  let currentLang = detectLang();

  function t(key, vars) {
    const table = I18N[currentLang] || I18N.en;
    let text = table[key] || I18N.en[key] || key;
    if (vars) for (const [k, v] of Object.entries(vars)) text = text.replace(`{${k}}`, v);
    return text;
  }

  function applyLang(code, remember) {
    currentLang = SUPPORTED.includes(code) ? code : "en";
    if (remember) { try { localStorage.setItem(LS_LANG, currentLang); } catch (_) {} }

    document.documentElement.lang = currentLang === "zh" ? "zh-CN" : currentLang;

    for (const el of document.querySelectorAll("[data-i18n]")) {
      el.textContent = t(el.dataset.i18n);
    }
    document.title = t("page.title");
    const desc = document.querySelector('meta[name="description"]');
    if (desc) desc.setAttribute("content", t("page.desc"));

    for (const btn of document.querySelectorAll(".lang button")) {
      const active = btn.dataset.lang === currentLang;
      btn.classList.toggle("active", active);
      btn.setAttribute("aria-selected", String(active));
      btn.setAttribute("aria-label", t("lang.switch", { name: LANG_NAMES[btn.dataset.lang] || btn.dataset.lang }));
    }
    positionLangPill();
    updateThemeButton();
  }

  function positionLangPill() {
    const bar = document.querySelector(".lang");
    const pill = document.querySelector(".lang-pill");
    const active = document.querySelector(".lang button.active");
    if (!bar || !pill || !active) return;
    const offset = active.offsetLeft;
    pill.style.width = `${active.offsetWidth}px`;
    pill.style.transform = `translateX(${offset - 3}px)`;
  }

  /* --------------------------------- theme ------------------------------- */
  const MODES = ["auto", "light", "dark"];
  const mql = window.matchMedia("(prefers-color-scheme: light)");
  let themeMode = "auto";

  function resolvedTheme() {
    if (themeMode === "light" || themeMode === "dark") return themeMode;
    return mql.matches ? "light" : "dark";
  }

  function applyTheme(remember) {
    document.documentElement.dataset.theme = resolvedTheme();
    document.documentElement.dataset.mode = themeMode;
    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute("content", resolvedTheme() === "light" ? "#f7f7fb" : "#07080f");
    if (remember) { try { localStorage.setItem(LS_THEME, themeMode); } catch (_) {} }
    updateThemeButton();
  }

  function updateThemeButton() {
    const btn = document.querySelector(".theme-btn");
    if (!btn) return;
    btn.dataset.mode = themeMode;
    btn.setAttribute("aria-label", t("theme.toggle", { mode: t(`theme.${themeMode}`) }));
    btn.setAttribute("title", t("theme.toggle", { mode: t(`theme.${themeMode}`) }));
  }

  function initTheme() {
    let saved = null;
    try { saved = localStorage.getItem(LS_THEME); } catch (_) {}
    themeMode = MODES.includes(saved) ? saved : "auto";
    applyTheme(false);
  }

  function withViewTransition(mutate) {
    if (!reduceMotion && typeof document.startViewTransition === "function") {
      document.startViewTransition(() => mutate());
    } else {
      mutate();
    }
  }

  /* ------------------------------ interactions --------------------------- */
  function wireUI() {
    // language buttons
    for (const btn of document.querySelectorAll(".lang button")) {
      btn.addEventListener("click", () => {
        if (btn.dataset.lang === currentLang) return;
        withViewTransition(() => applyLang(btn.dataset.lang, true));
      });
    }
    window.addEventListener("resize", positionLangPill);

    // theme cycle
    const themeBtn = document.querySelector(".theme-btn");
    if (themeBtn) {
      themeBtn.addEventListener("click", () => {
        themeMode = MODES[(MODES.indexOf(themeMode) + 1) % MODES.length];
        withViewTransition(() => applyTheme(true));
      });
    }

    // follow the system while in auto mode
    const onSchemeChange = () => { if (themeMode === "auto") applyTheme(false); };
    if (typeof mql.addEventListener === "function") mql.addEventListener("change", onSchemeChange);
    else if (typeof mql.addListener === "function") mql.addListener(onSchemeChange);

    // footer year
    const year = document.getElementById("year");
    if (year) year.textContent = String(new Date().getFullYear());
  }

  /* ------------------------------ scroll reveal -------------------------- */
  function wireReveals() {
    const items = document.querySelectorAll(".reveal");
    if (!("IntersectionObserver" in window)) {
      items.forEach((el) => el.classList.add("in"));
      return;
    }
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        entry.target.classList.add("in");
        observer.unobserve(entry.target);
      }
    }, { threshold: 0.14, rootMargin: "0px 0px -8% 0px" });
    items.forEach((el) => observer.observe(el));
  }

  /* -------------------------------- counters ----------------------------- */
  function wireCounters() {
    const nodes = document.querySelectorAll("[data-count]");
    if (!nodes.length) return;
    const easeOutExpo = (x) => (x >= 1 ? 1 : 1 - Math.pow(2, -10 * x));

    const run = (el) => {
      const target = Number(el.dataset.count || 0);
      const suffix = el.dataset.suffix || "";
      if (reduceMotion) { el.textContent = `${target}${suffix}`; return; }
      const duration = 1500;
      const start = performance.now();
      const tick = (now) => {
        const p = Math.min(1, (now - start) / duration);
        el.textContent = `${Math.round(easeOutExpo(p) * target)}${suffix}`;
        if (p < 1) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    };

    if (!("IntersectionObserver" in window)) { nodes.forEach(run); return; }
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        run(entry.target);
        observer.unobserve(entry.target);
      }
    }, { threshold: 0.5 });
    nodes.forEach((el) => observer.observe(el));
  }

  /* --------------------------- spring-smoothed tilt ---------------------- */
  function wireTilt() {
    if (reduceMotion || window.matchMedia("(hover: none)").matches) return;
    const cards = Array.from(document.querySelectorAll(".tilt"));
    if (!cards.length) return;

    const state = new Map();
    for (const card of cards) state.set(card, { rx: 0, ry: 0, tx: 0, ty: 0, trx: 0, try_: 0 });
    let pointer = null;

    window.addEventListener("pointermove", (event) => {
      pointer = { x: event.clientX, y: event.clientY };
    }, { passive: true });

    const MAX = 7; // degrees
    const loop = () => {
      for (const card of cards) {
        const s = state.get(card);
        const rect = card.getBoundingClientRect();
        const cx = rect.left + rect.width / 2;
        const cy = rect.top + rect.height / 2;
        const near = pointer &&
          pointer.x > rect.left - 120 && pointer.x < rect.right + 120 &&
          pointer.y > rect.top - 120 && pointer.y < rect.bottom + 120;

        if (near) {
          const px = (pointer.x - cx) / Math.max(rect.width, 1);
          const py = (pointer.y - cy) / Math.max(rect.height, 1);
          s.trx = Math.max(-1, Math.min(1, py)) * -MAX;
          s.try_ = Math.max(-1, Math.min(1, px)) * MAX;
          s.ty = -6;
        } else {
          s.trx = 0; s.try_ = 0; s.ty = 0;
        }

        // critically damped-ish spring towards the target
        const k = 0.12;
        s.rx += (s.trx - s.rx) * k;
        s.ry += (s.try_ - s.ry) * k;
        card.style.setProperty("--rx", `${s.rx.toFixed(2)}deg`);
        card.style.setProperty("--ry", `${s.ry.toFixed(2)}deg`);
        card.style.setProperty("--ty", `${s.ty.toFixed(2)}px`);
      }
      requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
  }

  /* ------------------------------- parallax ------------------------------ */
  function wireParallax() {
    if (reduceMotion) return;
    const backdrop = document.querySelector(".backdrop");
    const art = document.querySelector(".hero-art");
    if (!backdrop && !art) return;

    let tx = 0, ty = 0, cx = 0, cy = 0;
    window.addEventListener("pointermove", (event) => {
      tx = (event.clientX / window.innerWidth - 0.5) * 34;
      ty = (event.clientY / window.innerHeight - 0.5) * 26;
    }, { passive: true });

    const loop = () => {
      cx += (tx - cx) * 0.045;
      cy += (ty - cy) * 0.045;
      if (backdrop) backdrop.style.transform = `translate3d(${(-cx * 0.6).toFixed(2)}px, ${(-cy * 0.6).toFixed(2)}px, 0)`;
      if (art) art.style.transform = `translate3d(${(cx * 0.5).toFixed(2)}px, ${(cy * 0.5).toFixed(2)}px, 0)`;
      requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
  }

  /* --------------------------- scroll progress bar ----------------------- */
  function wireScrollProgress() {
    const bar = document.createElement("div");
    bar.className = "scroll-progress";
    document.body.appendChild(bar);
    const update = () => {
      const max = document.documentElement.scrollHeight - window.innerHeight;
      const p = max > 0 ? window.scrollY / max : 0;
      bar.style.transform = `scaleX(${p.toFixed(4)})`;
    };
    window.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    update();
  }

  /* --------------------------------- boot -------------------------------- */
  function boot() {
    initTheme();
    applyLang(currentLang, false);
    wireUI();
    wireReveals();
    wireCounters();
    wireTilt();
    wireParallax();
    wireScrollProgress();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
