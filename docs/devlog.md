# Development log

## 2026-08-02 — F07：tone-mapping，以及 loader 實際交回來的是什麼

**問題：** `hdr.toggle` 只換 badge，畫面沒有任何色彩變化——HLG/PQ 檔案就是照著檔案裡的編碼值直接顯示。

**先量測，因為量測結果決定了設計。** glycin 0.8 自己的 `Transform` 只有
`Icc | ImageProfile | Nclx | GainMap | ChromaPosition | ExifOrientation`，根本沒有 HLG/PQ transfer 這一類。
對 `IMG_5053.HEIC`（經 ICC profile 判為 PQ，`cicp_color_primaries=9, transfer=16`）分別要求
`R8g8b8` 與 `R16g16b16`、`apply_transformations` 開與關，取回的值**完全相同**、所有通道 ≤ 1.0、
mean 0.604。也就是說 loader 交回的是已經處理成顯示就緒的畫面，再套一層 tone-map 只會把本來正確的
圖壓暗。因此 `crates/spectrum-core/tests/tone_map.rs` 的 `only_scene_referred_signalling_gets_a_transform`
用一個只有 ICC `cicp` tag 的 HEIF fixture 斷言「profile 推斷出的訊號不得授權 tone-mapping」；會授權的只有 `nclx`（它描述的是 image item 本身）與 PNG 的 `cICP` chunk，理由相同。

**實作**在 `crates/spectrum-core/src/tone_map.rs`（334 行）：PQ EOTF/OETF（ST 2084 常數寫成分數，
`c1 = 3424/4096`、`c3 = 76544/4096`——一開始誤寫成 `3420/4096`，讓 100 nit 的中間調在編碼域偏移
0.011，被參考值 test 擋下）、HLG EOTF/OETF（BT.2100 Table 6）、sRGB EOTF/OETF、BT.709↔BT.2020
矩陣（用公布矩陣求逆，test 斷言兩個矩陣相乘等於單位矩陣、D65 灰色不變色）、extended Reinhard
壓縮、ISO 21496-1 `gain_map_blend`。`tone_map_frame` 在縮圖執行緒跑，不佔用 UI 執行緒；texture
快取以 `(path, transform)` 為 key，所以切換顯示模式不會重新解碼。

**用公開數值驗證，不是自己跟自己一致：** PQ 把 100 cd/m² 編碼為 0.5080762（±2e-6）、1000 cd/m² 為
0.7515776；把 0.5080762 解回 100.0 nit；HLG 把峰值的 1/12 編碼為恰好 0.5；HLG EOTF 在 knee 處連續
到 1.2e-6。

**UI 證據。** `crates/spectrum-app/tests/hdr.rs` 用帶 `cICP`（`colour_primaries 9,
transfer_function 18`）的 PNG fixture，其像素 loader 不會動。開啟 HDR 顯示時該檔案的格子畫出
`[217, 130, 36]`（fixture 的 `[188, 120, 60]` 獨立算出的 tone-map 結果），關掉時畫出
`[188, 120, 60]`，同一檔案的 detail canvas 也跟著變。同目錄另一個宣告
`transfer_function 13`（sRGB）的 fixture 兩種模式下都畫 `[188, 120, 60]`，證明 tone-map 沒有無差別套用。

**還沒做完：** gain-map 展開只有純函式與 test，沒有呼叫端，因為 loader 只交回 base image。檔案沒有
mastering metadata 時 PQ 的峰值假設為 1000 cd/m²，所以 4000 nit 的母帶是用猜的壓縮——曲線对中間調
掩蓋了誤差，對高光沒有。視訊 HDR（F16）完全還沒有路徑。

---

## 2026-10-07 — XMP 讀取永遠回 None（self-closing 元素）

**類型：** Bug Fix

**問題：** `xmp::read_sidecar()` 對任何寫入過的 sidecar 都回 `None`，`round_trip_all_orientations` 與 `gyro_config_round_trip_with_xml_special_chars` 兩個 test 失敗。

**根因／做法：** `write_sidecar()` 輸出的 `<rdf:Description ... />` 是 self-closing 元素，quick-xml 對它發出 `Event::Empty` 而不是 `Event::Start`，所以 `find_description_attrs()` 的匹配永遠不會發生。改成同時匹配 `Event::Start | Event::Empty`。

另外，crop 的十進位 round-trip 不穩定：寫入 6 位小數後讀回 `0.4 - 0.1 = 0.30000000000000004`，與 `CropRect` 的 `0.3` 不相等。加入 `round6()`（`value * 1e6` 四捨五入再除以 `1e6`）套在四個邊界與 `w`/`h` 上，使 round-trip 可精確比對。

**修改的檔案：** `crates/spectrum-core/src/xmp.rs`

---

## 2026-10-07 — HEIF box 型別讀錯位

**類型：** Bug Fix

**問題：** `heif::boxes()` 回傳的 box 型別是 box size 欄位，`find_colr()` 找不到 `colr`。

**根因／做法：** ISO BMFF box header 是 4-byte size + 4-byte type，程式碼把 `data[i..i+4]` 當成 type。正確位置是 `data[i+4..i+8]`。

**修改的檔案：** `crates/spectrum-core/src/heif.rs`

---

## 2026-10-07 — HDR（transfer function）偵測

**類型：** Feature

**問題：** 需要判斷一個檔案是不是 HDR，並決定顯示用 HDR 還是 SDR。

**根因／做法：** 讀 HEIF 的 `colr`/`nclx` box（ISO 14496-12 已查證的 code：13 = sRGB、16 = PQ、18 = HLG、2 = unspecified；BT.2020 primaries = 9、11）。`TransferFunction::from_code()` 對應 code，`is_hdr()` 只認 PQ/HLG，`is_bt2020_primaries()` 認 9/11，`display_mode(nclx, hdr_enabled)` 決定顯示模式。已知限制保留成 test：內容是 HLG 訊號但被標成 sRGB 的檔案會被判為 SDR，因為偵測只看 signalled 資訊。JPEG 沒有 `colr`，目前 `detect_nclx()` 回 `None`（EXIF 讀取尚未實作）。

`scan.rs` 提供 `kind_for()`、`detect_nclx()`、`scan_folder()`（非遞迴、依檔名排序保證 test 穩定）。`fixture.rs` 產生純 box 結構的合成 HEIF（不含像素），避免把個人照片放進 repo。

**修改的檔案：** `crates/spectrum-core/src/heif.rs`、`src/hdr.rs`、`src/scan.rs`、`src/fixture.rs`、`tests/hdr_detection.rs`（6 tests）

---

## 2026-10-07 — e2e test 守護（egui_kittest，headless）

**類型：** Feature

**問題：** 功能增加時要有 e2e test 守護，且這台機器是 Linux，無法跑 Xcode UI test。

**根因／做法：** 選 egui + `egui_kittest` 0.36.2（AccessKit-based，`cargo test` 可完全 headless 跑）。`AppState::render()` 與狀態分離，test 只能透過 UI 操作（click、讀 label）驅動，不能碰內部欄位，這樣才算真正的 e2e。

流程 test：`tests/e2e.rs` 的 `e2e_grid_select_and_hdr_toggle` — grid 列出兩個 fixture → 點選 HLG 檔案 → badge 顯示 `HDR` → 按 `HDR/SDR toggle` → badge 變 `SDR` → 再打開 toggle 並選 sRGB 檔案 → 仍是 `SDR`（標錯來源的守護）。

Snapshot test 可跑（Vulkan 走 lavapipe）。一開始同一個 snapshot 每次跑都差 28–70 pixels，根因是 test 把 `std::process::id()` 寫進 fixture 目錄名，而那個路徑會被渲染成 label 文字 → 每個 process 文字不同。改用固定目錄名 `spectrum-e2e-fixtures` 後，連續 3 次跑 diff = 0。

**修改的檔案：** `crates/spectrum-app/Cargo.toml`、`crates/spectrum-app/src/app.rs`、`crates/spectrum-app/tests/e2e.rs`、`test.sh`、`.gitignore`

---

## 目前測試狀態

`./test.sh` → 25 passed（edit_ops 12、hdr_detection 6、xmp_roundtrip 5、e2e 2）。

---

## 2026-10-07 — 可執行入口（eframe bin）

**類型：** Feature

**問題：** `spectrum-app` 只有 lib，跑不起來。

**根因／做法：** 加 `crates/spectrum-app/src/main.rs`，用 `eframe::run_native`，`Runner` 只是把 `eframe::App` 接到 `AppState::render()`，所以 e2e harness 和真實 app 共用同一份 render 程式碼（test 守護不會跟實際顯示分岔）。

eframe 0.36 三個要注意的點：
1. `App` trait 的方法是 `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame)`，不是舊的 `update(ctx, frame)`。
2. `AppCreator` 回 `Result<Box<dyn App>, Box<dyn Error + Send + Sync>>`，所以 creator 要寫 `Ok(Box::new(...))`。
3. `default-features = false` 會炸：winit 的 `x11`/`wayland` 是透過 eframe 的 default features 傳下去，關掉就變成 `compile_error!("The platform you're compiling for is not supported by winit")`。所以 `eframe = "0.36"` 用預設 features。

**修改的檔案：** `crates/spectrum-app/Cargo.toml`、`crates/spectrum-app/src/main.rs`

---

## 2026-10-08 — GTK4 + libglycin-gtk4 viewer 與 headless e2e

**類型：** Feature

**問題：** 把 viewer 換成 GTK4（用 `libglycin-gtk4` 把 glycin 解碼結果轉成 `Gdk.Texture`），並確認在沒有實體顯示器的環境也能跑 e2e test。

**根因／做法：** GTK4 沒有 headless GDK backend（`meson.options` 只有 x11/wayland/broadway/win32/macos/android），所以 e2e 需要虛擬顯示器。本機用 `gtk4-broadwayd` + `GDK_BACKEND=broadway` + `GTK_A11Y=test`（GTK 文件建議 test suite 用 `test` accessibility backend）+ `GSK_RENDERER=cairo`（CPU 渲染器，跨 process 產生相同 PNG）。快照流程：`window.snapshot_child(content, snap)` → `snap.to_node()` → `native().renderer().render_texture(node, rect)` → `texture.save_to_png()`。

兩個坑：
1. `GtkPicture` 沒有 paintable 時 allocation 高度是 0，產生不出 render node，整個 snapshot 變空（`to_node()` 回 `None`）。實作給 picture 一個 `set_size_request(120, 80)`，test 則等 box 把高度完整分配給 children（`settle()`）才快照。
2. GTK4 無法在 process 內注入點擊（`Gtk.Button.do_clicked` 是 vfunc，`activate()` 只發 `activate` 信號），所以 test 用 `emit_by_name::<()>("clicked", &[])` 驅動 handler。

GTK 必須在主執行緒初始化，`cargo test` 每個 test 跑在不同執行緒會觸發 `Attempted to initialize GTK from two different threads`，因此用 `gtk4::test_synced`（跑在 GTK 的 exclusive thread，可多個 test 共用一次 init）。

依賴：`libglycin-gtk4-rebind 0.1` 要求 `libglycin-rebind ^0.1.0`，所以 `libglycin-rebind` 必須 pin 在 0.1（否則兩份 crate 的 `gly` 型別不相容）。graphene 要用 `graphene-rs`（crates.io 上的 `graphene` 是另一個 crate）。建置需要 `glycin-gtk4-2.pc`（來自 `libglycin-gtk4-2-dev`）；本機只有 runtime `.so`，`test.sh` 會在找不到 pkg-config 檔時改用 `/tmp/pc` 的 shim。

**修改的檔案：** `crates/spectrum-gtk/Cargo.toml`、`crates/spectrum-gtk/src/app.rs`、`src/lib.rs`、`src/main.rs`、`tests/e2e.rs`（2 tests：邏輯 + 快照比對 baseline）、`tests/fixtures/sample.png`、`test.sh`（自動啟動 `gtk4-broadwayd`）

目前 `./test.sh` → 27 passed（edit_ops 12、hdr_detection 6、xmp_roundtrip 5、egui e2e 2、gtk e2e 2），快照連續 12 次執行 md5 相同。

---

## 2026-10-08 — 快照 flaky 根因：GtkPicture 要有真正的 paint pass

**類型：** Bug Fix

**問題：** 同一個快照 test 連續執行會偶爾得到 `snapshot produced no node`（10 次裡失敗 1 次）。

**根因／做法：** `GtkPicture` 的 snapshot cache 是在 paint 時建立的。如果 picture 在還沒被 paint 前就已經拿到 allocation（當時沒有 paintable），cache 會保持空，之後即使補上 paintable，`snap.to_node()` 仍然回 `None`；`queue_draw()` 也沒用。順序必須是「先給 paintable，再讓 frame clock 走一格」。test 改成 `settle_snapshot()`：用 `glib::MainContext::default().iteration(false)` 反覆 pump（無事件時 sleep 20 ms）直到 content box 的 snapshot 產生 node。另外 `snapshot_child` 要求 child 是該 widget 的直接子節點，所以只能對 `window` 的直接 child（content box）呼叫。

**修改的檔案：** `crates/spectrum-gtk/tests/e2e.rs`（`settle_snapshot()`）

連續 3 次執行結果相同：快照 900×381、md5 `030e14217da035d3440861759f9b4374`。

---

## 2026-10-08 — list 縮圖 + GridView 排列

**類型：** Feature

**問題：** 使用者要求檔案清單要更美觀，且照片除了預覽還要有 grid view 排列。

**根因／做法：** 左側改成 `ListBox`（每列 = 48×48 縮圖 + `name · transfer` 標籤）放在 `ScrolledWindow`，中間上方是 `GridView`（tile = 64×64 縮圖 + 標籤，min_columns 3 / max_columns 6），下方是預覽。兩邊共用同一個 `SingleSelection`，所以點選會同步。縮圖用 `set_content_fit(ContentFit::ScaleDown)` 避免把小圖放大。

gtk4-rs 0.11.5 幾個要注意的點：
1. `ContentFit` 被 `v4_8` feature gate 住，預設 features 沒有 → `Cargo.toml` 要加 `features = ["v4_12"]`（`set_max_width`/`set_max_height` 也沒有綁定）。
2. `ListItemFactory` 沒有 `new()`，要用 `SignalListItemFactory::new()`；且啟用 v4_8+ 後 `setup`/`bind` 信號給的是 `&glib::Object`（要 downcast 成 `ListItem`）。
3. `new()` 的 model/factory 參數是 `Option<impl IsA<...>>`（by value），`IsA` 沒有對 `&T` 的 blanket impl，所以 `Some(&model)` 會編譯失敗；改成 `new(None::<...>, None::<...>)` 再用 `set_model()`/`set_factory()`。
4. `glib::ListStore` 要用 `gio::ListStore::new::<glib::BoxedAnyObject>()`（glib 0.22 沒有 `glib::ListStore`），取值用 `borrow::<MediaEntry>()`。
5. `GridView` 沒有 `item_at_index`，test 只能透過 `model()`/`factory()`/`min_columns()`/`max_columns()` 斷言 grid 綁定正確。

另外修一個會 abort 的 bug：`connect_clicked` 內 `st4.borrow_mut().hdr_enabled = !st4.borrow().hdr_enabled;` 同時持有 `borrow_mut()` 和 `borrow()` → RefCell panic，而這個 panic 發生在 GLib callback 內無法穿越 C frame（`thread caused non-unwinding panic. aborting.`，SIGABRT）。改成先 `let st = &mut *st4.borrow_mut();` 再操作。

`kind_for()` 補上 `png`/`webp`：glycin 能解碼 PNG，而 box-only 的 `.heic` test fixture 沒有像素資料（`Unexpected end of file`），所以 e2e 需要一個可解碼的 fixture 才會產生縮圖。egui 與 GTK 的 fixture 目錄分開（`spectrum-egui-fixtures` / `spectrum-e2e-fixtures`），避免互相污染。

**修改的檔案：** `crates/spectrum-gtk/Cargo.toml`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`、`crates/spectrum-core/src/scan.rs`、`crates/spectrum-core/tests/scan_kind.rs`、`crates/spectrum-app/tests/e2e.rs`

目前 `./test.sh` → 30 passed（edit_ops 12、hdr_detection 6、xmp_roundtrip 5、scan_kind 2、egui e2e 2、gtk e2e 3）。

---

## 2026-10-08 — JPEG 的 EXIF / ICC transfer function 讀取

**類型：** Feature

**問題：** `detect_nclx()` 只讀 HEIF 的 `colr` box，所以 JPEG 一律回 `None`，在 UI 上永遠被顯示成 SDR（即使是 HLG/PQ 照片）。

**根因／做法：** 新增 `crates/spectrum-core/src/jpeg.rs`，只走 JPEG marker segment（不解碼像素）。這與 macOS 版 `ImagePreloadCache.detectHDR()` 相同：讀 header 屬性比解 8×8 縮圖快（舊版解縮圖約 120 ms/張，佔 grid CPU 的 25%）。判定優先順序：ICC profile name → EXIF `CustomRendered == 3`（gain map）→ EXIF `ColorSpace`。

ICC 三個要注意的點（本機用 `/usr/share/doc/gjs/examples/test.jpg` 與 `.../vxlan.jpg` 實測確認）：
1. ICC header 是 128 bytes：size(4) 在 0、`acsp` signature 在 36、tag count 在 **128**、tag list 在 **132**（不是 124/128）。
2. APP2 的 ICC chunk header 各 encoder 長度不一致（實測兩個檔案的 profile 起點是 `ICC_PROFILE\0` 後第 14 個 byte，而文件寫的是 15），所以 `parse_icc_name()` 不假設起點，改用 `acsp`（profile offset 36）反推 profile 起點。
3. `desc` tag data = ASCII count(4) + 31-byte null-terminated name + 10 reserved；`dscm` = ICC v4 UTF-16BE。

資料模型跟著調整：`hdr.rs` 新增 `SignalSource`（`Nclx` / `IccProfileName` / `ExifCustomRendered` / `ExifColorSpace`）與 `ImageSignal { transfer, source, orientation }`，`is_hdr_signal()` 對 PQ/HLG 或 `ExifCustomRendered` 回 true；`MediaEntry.nclx` 改成 `MediaEntry.signal`，`scan_folder()` 改用 `detect_signal()`（依副檔名分派 HEIF `colr` 或 JPEG EXIF/ICC）。UI（egui 與 GTK）的 badge 都改讀 `signal`。

**修改的檔案：** `crates/spectrum-core/src/jpeg.rs`（新）、`src/hdr.rs`、`src/scan.rs`、`src/fixture.rs`（`synthetic_jpeg` / `write_jpeg_fixture`，header-only fixture，不含像素）、`tests/jpeg_signal.rs`（7 tests）、`tests/hdr_detection.rs`、`crates/spectrum-gtk/tests/e2e.rs`（fixture 加入 `hlg_icc.jpg` / `sdr_exif.jpg` / `mislabeled.jpg`，6 個檔案依 path 排序後逐列斷言 `name · transfer`）、`crates/spectrum-app/src/app.rs`

目前 `./test.sh` → 37 passed（edit_ops 12、hdr_detection 6、jpeg_signal 7、scan_kind 2、xmp_roundtrip 5、egui e2e 2、gtk e2e 3），快照連續 10 次執行 md5 相同（`cfda41f55278a4ffe88d4d14ff243b08`）。

---

## 2026-10-08 — 縮圖產生與快取（`ThumbnailService`）

**類型：** Feature

**問題：** 清單與 grid 每顯示一次就重新解碼一次同一張圖；且縮圖沒有尺寸上限，小圖會被放大。

**根因／做法：** `crates/spectrum-core/src/thumbnail.rs` 新增 `ThumbnailService`：
- `scale_within(w, h, max)`：長邊上限 `max`（預設 64），保持長寬比，且**不放大**（`w<=max && h<=max` 時原尺寸回傳）。
- `get(path)`：第一次走 glycin 解碼（`set_apply_transformations(true)` 保留 EXIF 方向），把 `gly::Frame` 與目標尺寸存進 `HashMap<PathBuf, Cached>`；第二次同一 path 直接回快取條目並把 `cached` 設為 true，`hits()` 遞增、`decodes()` 不變。
- GTK 端 list 與 grid 共用同一個 service（`Ui.thumbs`），所以 grid 綁定同一檔案時是快取命中，不再解碼。

`fixture.rs` 補 `synthetic_png(w, h, rgb)`：自己寫最小合法 PNG（IHDR + stored-deflate IDAT + IEND，含 CRC32/adler32），這樣 core 的測試不需要依賴真實照片，而且 glycin 能真的解出內容（實測 100×60 → 快取命中、目標 64×38）。

注意：box-only 的 `.heic` fixture 沒有像素資料，glycin 會回 `Unexpected end of file`，所以 e2e 裡只有 `sample.png` 能進快取（`decodes()==1`、`hits()>0`）。

**修改的檔案：** `crates/spectrum-core/Cargo.toml`（加 `gio`、`libglycin-rebind`）、`src/thumbnail.rs`（新）、`src/fixture.rs`、`src/lib.rs`、`tests/thumbnail.rs`（3 tests）、`crates/spectrum-gtk/src/app.rs`（`paint_thumbnail()` 改用 service）、`crates/spectrum-gtk/tests/e2e.rs`（斷言 cap=64、decodes=1、hits>0、8×6 不被放大）

目前 `./test.sh` → 40 passed，快照連續 10 次執行 md5 相同（`1021be40d85d8d914d40486846b4c296`）。

---

## 2026-10-08 — 編輯 op 套用到預覽顯示 + XMP sidecar 寫入

**類型：** Feature

**問題：** core 已有 `EditOp`/`CompositeEdit`/`write_sidecar`，但 GTK 端沒有把 op 套用到預覽，也沒有寫 sidecar 的入口。

**根因／做法：**
1. `crates/spectrum-core/src/edit.rs` 新增 `CompositeEdit::display_size(width, height) -> (u32, u32)`：先依 rotation 決定是否交換長寬（90/270 交換），再依 crop（座標本來就是「旋轉後空間」）算出顯示尺寸。
2. `crates/spectrum-gtk/src/edit.rs`（新）`apply_edit(frame, edit) -> gdk::Texture`：`gdk::Texture` 沒有 transform API，所以走 gdk-pixbuf 當像素緩衝 — `Pixbuf::from_bytes(frame.buf_bytes(), Colorspace::Rgb, has_alpha, 8, w, h, stride)`，依 `orientation.rs` 的慣例「先 flip 再 rotate」（`flip(true)` + `rotate_simple(Clockwise/Upsidedown/Counterclockwise)`），crop 用 `new_subpixbuf()`。只支援 8-bit 格式（`R8g8b8`/`R8g8b8a8`），HDR 的 16-bit frame 會回錯誤。
3. `State` 加 `ops: Vec<EditOp>`（非破壞性、依順序），`edit_of(state)` = `CompositeEdit::from(&state.ops)`；`apply_op(state, ui, op)` push + refresh；`save_sidecar(state)` 用 `e.signal.orientation`（EXIF 讀到的）呼叫 `xmp::write_sidecar`。UI 加三個按鈕：Rotate 90°、Flip H、Save sidecar。detail 標籤在非 identity 時顯示 `name · kind · WxH`。

要注意的點：`EditOp` 不是 `Copy`，在 `connect_clicked` 的 `Fn` closure 裡要 `op.clone()`。

**修改的檔案：** `crates/spectrum-core/src/edit.rs`、`crates/spectrum-gtk/src/edit.rs`（新）、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/Cargo.toml`（加 `gdk-pixbuf = "0.22"`）、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_edit_ops_and_sidecar_roundtrip`）

e2e 內容：8×6 fixture → rotate 90 後 `display_size` 與實際 texture 都是 6×8；flip 後 rotation 變 270（`FlipH` 會把 rotation 取反，這是 `CompositeEdit` 的語意）且尺寸不變；crop (0.25,0.25,0.5,0.5) 在旋轉空間 → 3×4；寫 sidecar 後 `read_sidecar` 讀回與記憶體中的 edit 完全相同。

目前 `./test.sh` → 41 passed，快照連續 10 次執行 md5 相同（`7a3002c09aafc78463f07e5cbe0ce517`）。

---

## 2026-10-08 — HDR/SDR 開關套用到 list 與 grid 的標籤

**類型：** Feature

**問題：** 清單/grid 的標籤寫死 `display_mode(signal, true)`，所以關掉 HDR 後標籤仍顯示 HDR，只有 badge 會變。

**根因／做法：** factory 建立標籤時把 `(path, label)` 記進 `Ui.labels`，`refresh()` 依 `state.hdr_enabled` 重新整理每一筆標籤文字（`name · transfer`），list 和 grid 共用同一份 `refresh`，所以兩邊一致。

踩到的坑：一開始用 `HashMap<PathBuf, gtk::Label>`，同一個檔案在 list 和 grid 各有一個 label，HashMap 只留最後插入的那個 → 另一個 view 的標籤不會被更新（e2e 抓到 list 標籤在 toggle off 後仍是 HDR）。改成 `Vec<(PathBuf, gtk::Label)>` 後兩邊都會更新。

另外 `ListBox::bind_model` 的 callback 收到的是 model item（`&glib::Object`，這裡是 `BoxedAnyObject`），**不是** `ListItem`，所以不能 `downcast_ref::<gtk::ListItem>()` / `position()`；只有 `SignalListItemFactory` 的 setup/bind 收到的是 ListItem。

**修改的檔案：** `crates/spectrum-gtk/src/app.rs`（`Ui.labels` + `refresh()` 更新標籤）、`crates/spectrum-gtk/tests/e2e.rs`（toggle off 後 6 個 list 標籤都要是 SDR；toggle on 後依檔案檢查 list 與 grid 標籤一致，mislabeled.jpg 仍是 SDR）

目前 `./test.sh` → 41 passed，快照連續 10 次執行 md5 相同（`7a3002c09aafc78463f07e5cbe0ce517`）。

---

## 2026-10-08 — 掃描時讀取 XMP sidecar 的編輯並套用到顯示

**類型：** Feature

**問題：** sidecar 只有手動 `read_sidecar` 才會讀，掃描檔案時不會帶進來，所以重新開啟資料夾後之前儲存的編輯不會顯示。

**根因／做法：** `MediaEntry` 增加 `edit: Option<SidecarData>`，`scan_folder()` 對每個檔案用 `xmp::read_sidecar(path, orientation)`（orientation 來自 EXIF，沒有 EXIF 時用 1）填入。UI 端 `effective_edit(state, entry)`：使用者有 live ops 時用 live ops，否則用 sidecar 的編輯。detail 標籤在編輯來自 sidecar 時加上 ` (sidecar)` 後綴。

`SidecarData` 加 `edit() -> CompositeEdit` 轉換（crop 已在旋轉座標空間）。`CompositeEdit` 補 `Default` derive（rotation 0 / flip_h false / crop None）。`MediaEntry` 的 derive 從 `Eq` 改成只有 `PartialEq`，因為 `CropRect` 欄位是 `f64`，`SidecarData` 無法 `Eq`。

注意：`SingleSelection` 的 `selected-notify` 只在值改變時觸發，所以 e2e 需要兩個檔案才能觀察到 detail 更新（單檔案時 `set_selected(0)` 不會觸發）。

**修改的檔案：** `crates/spectrum-core/src/scan.rs`、`crates/spectrum-core/src/xmp.rs`、`crates/spectrum-core/src/edit.rs`、`crates/spectrum-core/tests/scan_sidecar.rs`（2 tests）、`crates/spectrum-gtk/src/app.rs`（`effective_edit()` + detail 後綴）、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_sidecar_edit_applied_on_scan`）

目前 `./test.sh` → 42 passed，快照連續 10 次執行 md5 相同（`7a3002c09aafc78463f07e5cbe0ce517`）。

---

## 2026-10-08 — 縮圖跟著編輯走（list/grid 套用 crop/rotate/flip）

**類型：** Feature

**問題：** `apply_edit()` 只用在 detail 預覽和 test，list/grid 的縮圖還是原始方向，所以旋轉後縮圖不會變。

**根因／做法：** `Ui` 增加 `thumbs_pics: Rc<RefCell<Vec<(PathBuf, gtk::Picture)>>>`（一個檔案在 list 和 grid 各有一個 Picture，所以要記錄全部）。`paint_thumbnail()` 改用 `effective_edit(state, entry)`：有編輯時先 `apply_edit(frame, edit)` 再 `frame_get_texture`，`set_size_request` 用 `scale_within(變換後的尺寸, max)`（旋轉會 interchange 長寬）。`refresh()` 每次重繪所有記錄的縮圖，所以按 Rotate/Flip 後 list 和 grid 會一起更新。

`ThumbnailService::get()` 回傳 owned `(gly::Frame, Thumbnail)`（`gly::Frame` 可 clone），重繪時走快取命中，不會重新解碼 —— e2e 斷言 `decodes()` 不變且縮圖尺寸從 8x6 變成 6x8。

另外修 test 隔離問題：`crates/spectrum-core/tests/scan_sidecar.rs` 用固定 temp 目錄，上一輪留下的 `.xmp` 會讓 `entries[0].edit.is_none()` 失敗 → 每個 test 開頭先 `xmp::delete_sidecar(&path)`。

**修改的檔案：** `crates/spectrum-gtk/src/app.rs`（`thumbs_pics`、`paint_thumbnail()`、`refresh()`）、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_thumbnail_reflects_edit`）、`crates/spectrum-core/tests/scan_sidecar.rs`

目前 `./test.sh` → 43 passed，快照連續 10 次執行 md5 相同（`7a3002c09aafc78463f07e5cbe0ce517`）。

---

## 2026-10-08 — crop 套用到顯示時用錯座標空間

**類型：** Bug Fix

**問題：** `apply_edit()` 對 `Crop` 切出的區域與 `display_size()` 不一致：`CropRect` 存在「旋轉後」的座標空間，但 `apply_edit()` 的 origin 用來源影像尺寸計算，所以 `rotation != 0` 時切到錯誤位置甚至切到界外。

**根因／做法：** 例：100×50 來源，`Rotate(90)` 後 buffer 是 50×100，`Crop(0.5, 0, 0.5, 1)` 應該切 x=25、w=25、h=100（25×100）。原本寫成 `pixel_rect(frame.width(), frame.height())` → 得到 (50, 0, 50, 50)，對 50×100 的 pixbuf 來說 x=50 已經超出寬度。修法是先算 rotated dims（rotation 90/270 時交換 w/h），origin 和 size 都用同一個 `pixel_rect(rw, rh)`。

補上 `tests/display_size.rs`（4 tests）：rotation only 交換長寬、crop only 用來源尺寸、crop 在 rotation 後用 rotated 尺寸且矩形必須在界內、crop 在 rotation 前只做座標 remap（面積不變，50×50）。

**修改的檔案：** `crates/spectrum-gtk/src/edit.rs`、`crates/spectrum-core/tests/display_size.rs`、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_crop_applied_to_display`：100×50 fixture + sidecar `Rotate(90)+Crop(0.5,0,0.5,1)` → detail 顯示 `25x100`、list/grid 縮圖 texture 也是 25×100）

目前 `./test.sh` → 44 passed，快照連續 10 次執行 md5 相同（`7a3002c09aafc78463f07e5cbe0ce517`）。

---

## 2026-10-08 — EXIF 相機/鏡頭資訊面板

**類型：** Feature

**問題：** core 只讀 EXIF 的 Orientation/ColorSpace/CustomRendered，沒有 make/model/ISO/曝光/光圈/焦距，所以詳情列無法顯示相機資訊（macOS 參考實作的 `PhotoInfoPanel` 有）。

**根因／做法：** `crates/spectrum-core/src/jpeg.rs` 新增 `ExifData`（make、model、ISO、ExposureTime、FNumber、FocalLength，RATIONAL 存成 `(numerator, denominator)`）與格式化：`shutter_speed()` 對 sub-second 輸出 `1/250s`、長曝光輸出 `2s`/`2.5s`；`aperture()` 輸出 `f/2.8`；`focal_length()` 輸出 `55mm`；`summary()` 只組合存在的欄位。Make/Model 在 IFD0，ISO/ExposureTime/FNumber/FocalLength 在 Exif 子 IFD。UI 加一個 `info` 標籤顯示 `summary()`，沒有 EXIF 時顯示空字串（不能留上一個檔案的殘留文字）。

test fixture 的 TIFF 產生器兩個坑：
1. SHORT inline 值已經佔滿 4 位元組 value 欄位，ISO 那筆多寫了一個 `write_u32_le(0)` → data area 往後移 4 位元組，字串讀成 `""` / `"non"`。
2. ASCII 的 count 含結尾 NUL，所以超過 4 位元組的名字必須放在 data area（count ≤ 4 才是 inline）。

**修改的檔案：** `crates/spectrum-core/src/jpeg.rs`、`crates/spectrum-core/src/hdr.rs`（`ImageSignal.exif`）、`crates/spectrum-core/src/scan.rs`、`crates/spectrum-core/src/fixture.rs`（`ExifFixture`）、`crates/spectrum-core/tests/exif_metadata.rs`（5 tests）、`crates/spectrum-gtk/src/app.rs`（`ui.info`）、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_exif_metadata_panel`）

目前 `./test.sh` → 54 passed，快照連續 10 次執行 md5 相同（`e0145a2769602bb45452afd562b01f43`，含新的 info 列）。

---

## 2026-10-08 — badge 區分 HDR 種類（gain map / HLG / PQ）

**類型：** Feature

**問題：** 所有 HDR 檔案的 badge 都顯示 `HDR`，看不出是 gain map、HLG 還是 PQ。macOS 參考實作的 `HDRFormat` 會區分（gainMap → `HDR`、hlg → `HLG`）。

**根因／做法：** `crates/spectrum-core/src/hdr.rs` 新增 `badge_label(signal, hdr_enabled)`：`SignalSource::ExifCustomRendered`（EXIF `CustomRendered == 3`，即 gain map）→ `HDR`；HLG → `HLG`；PQ → `PQ`；其他 → `SDR`。`display_mode()` 不變（仍是 SDR/HDR 的渲染決策），UI 的 list/grid 標籤與詳情 badge 改用 `badge_label`。

**修改的檔案：** `crates/spectrum-core/src/hdr.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-core/tests/badge_label.rs`（2 tests）、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（新增 `gainmap.jpg` fixture，共 7 個檔案）

目前 `./test.sh` → 56 passed，快照連續 10 次執行 md5 相同（`1beb6338c8da12b0960ffa4b4e5c0fea`，基線已重新產生）。

---

## 2026-10-08 — MP4/MOV 的 colr 也能讀（影片不再一律被判為 SDR）

**類型：** Feature

**問題：** `detect_signal()` 只走 HEIF 的 property container（`meta`/`iprp`/`ipco`），MP4/MOV 的 `colr` 位於 `moov > trak > mdia > minf > stbl > stsd > <sample entry>`，所以影片一律顯示 SDR。

**根因／做法：** `find_colr()` 改成依 box 類型決定要跳過幾 bytes 才到 children（`container_skip()`）：

| box | skip | 原因 |
|---|---|---|
| `meta`/`moov`/`mvex`/`trak`/`mdia`/`minf`/`stbl` | 4 | ISO 14496-12 full box（version + flags） |
| `iprp`/`ipco`/`ipro` | 0 | HEIF property container 是**一般 box**，不是 full box（fixture 原本寫成 full box，已改成跟規格一致） |
| `stsd` | 8 | full box + `entry_count` |
| `avc1`/`hvc1`/`av01`/`mp4v` | 42 | VisualSampleEntry 的固定欄位：6 reserved + data_reference_index + pre_defined/reserved + width/height + 兩個 resolution + reserved + frame_count + bytes_per_row + bits_per_pixel + pre_defined |

一開始寫 36 是錯的（少算了 pre_defined/reserved 等 6 bytes），test 立刻抓到：`find_colr` 把 42 bytes 前的 `colr` 當成一個 type 全 0 的 box。

新增 `fixture::synthetic_mp4()` / `write_mp4_fixture()` 產生 `ftyp + moov{trak{mdia{minf{stbl{stsd{avc1{colr}}}}}}}`（只有 box 結構，沒有像素）。

**修改的檔案：** `crates/spectrum-core/src/heif.rs`（`container_skip`）、`crates/spectrum-core/src/scan.rs`（`detect_signal` 支援 mp4/mov）、`crates/spectrum-core/src/fixture.rs`、`crates/spectrum-core/tests/mp4_colr.rs`（4 tests）、`crates/spectrum-gtk/tests/e2e.rs`（加入 `hlg_video.mp4` fixture，list/grid 變成 8 項，row 3 必須是 `hlg_video.mp4 · HLG` 且 detail 要寫 `video`）

目前 `./test.sh` → 62 passed；快照基線重新產生，連續 10 次執行 md5 相同（`2358a15338effa5da02c1f66494989a6`）。

---

## 2026-10-08 — MP4/MOV 的 colr 支援與真實 fixture 回歸

**類型：** Feature + Bug Fix

**問題：** `detect_signal()` 原本只對 heic/heif 讀 `colr`，所以影片一律被判為 SDR。另外所有測試都用自己寫的合成 fixture，涵蓋不到真實編碼器的 box 佈局。

**根因／做法：** `heif.rs` 的 `container_skips()` 依 ISO 14496-12 決定每個 container 的 children 起點：`meta`/`moov`/`mvex`/`trak`/`mdia`/`minf`/`stbl` 是 full box（4 bytes version/flags），`iprp`/`ipco` 是一般 box（0），`stsd` 多一個 entry_count（8），visual sample entry 有 42 bytes 固定欄位。

實測 FFmpeg 寫的 MP4 有兩處不合 spec：
1. `moov`/`trak`/`mdia`/`minf`/`stbl` 被寫成一般 box（沒有 version/flags）→ 每個 container 同時嘗試 spec 偏移與 0。
2. `compressorname` 寫滿 32 bytes（1 byte 長度 + 31 bytes 字串 `Lavc62.11.100 libx265`），不是 spec 的 `string[4]`，所以 children 起點變成 78。

`colr` 也可能裝 ICC profile（不是 nclx），所以 `find_colr()` 只接受 content 以 `nclx` 開頭的 box。

順带修一個誤判：`signal_for_jpeg()` 在沒有 ColorSpace tag 時仍回 `ExifColorSpace`，等於把「只有 orientation 的 EXIF」當成色彩訊號。新增 `SignalSource::ExifMetadataOnly`，只有真的有 ColorSpace 才宣稱 ExifColorSpace。

**修改的檔案：** `crates/spectrum-core/src/heif.rs`、`crates/spectrum-core/src/scan.rs`、`crates/spectrum-core/src/hdr.rs`、`crates/spectrum-core/src/fixture.rs`、`crates/spectrum-core/tests/mp4_colr.rs`（4 tests）、`crates/spectrum-core/tests/real_fixtures.rs`（6 tests，用從 macOS 版複製過來的真實 fixture）、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_real_fixtures_badge`）

目前 `./test.sh` → 69 passed；快照連續 10 次執行 md5 相同（`2358a15338effa5da02c1f66494989a6`）。

---

## 2026-10-08 — ISO 21496-1 gain map（JPEG APP2）

**類型：** Feature

**問題：** macOS 版 `detectHDR()` 的第一優先是 gain map 輔助資料，我們之前只有 `EXIF CustomRendered == 3` 這條路徑。所以「有 gain map 但 ICC 名稱寫 sRGB」的檔案會被漏判成 SDR。

**根因／做法：** `parse_jpeg()` 現在也認 APP2 segment 裡 payload 以 `urn:iso:std:iso:ts:21496:-1`（28 bytes，NUL 補齊）開頭的資料。ISO 21496-1 結構：

| 欄位 | 長度 | payload 內偏移 |
|---|---|---|
| URN | 28 | 0 |
| version (min + writer) | 4 | 28 |
| control flags | 1 | 32 |
| HDR headroom (baseline num/den, alternate num/den) | 16 | 33 |
| per-channel metadata | 40 × n | 49 |

headroom 的 baseline 讀在 payload offset **33/37**（我第一版寫 37/41，差了 4，test 立刻抓到）。

要注意 APP2 也可能是 ICC chunk（payload 以 `ICC_PROFILE\0` 開頭），兩者用不同 prefix 區分，不能混。

`SignalSource::GainMap` 排在 ICC 名稱之前（與 reference 的優先順序一致），badge 顯示 `HDR`。

**修改的檔案：** `crates/spectrum-core/src/jpeg.rs`、`crates/spectrum-core/src/hdr.rs`、`crates/spectrum-core/src/fixture.rs`、`crates/spectrum-core/tests/gain_map.rs`（6 tests）、`crates/spectrum-gtk/tests/e2e.rs`（新增 `app2_gainmap.jpg` fixture 與 badge 斷言）

目前 `./test.sh` → 75 passed；快照連續 10 次執行 md5 相同（`2a55e4b77cebaf749a35f5171cfd1c1a`）。

---

## 2026-10-08 — info 面板顯示 transfer function 的判定來源

**類型：** Feature

**問題：** info 面板之前只有 EXIF 摘要，看不出 transfer function 是從哪裡來的（`nclx`、ICC 名稱、gain map 還是 EXIF ColorSpace）。這正是 mislabel 案例最需要的資訊。

**根因／做法：** `ImageSignal` 加了 `headroom: Option<(u32, u32)>`（ISO 21496-1 的 gain map headroom），並新增 `ImageSignal::describe()` 與 `TransferFunction::name()`。UI 的 info 列變成 `describe · EXIF 摘要`，例如：

- `HLG (nclx)` — HEIF/MP4 的 `colr` box
- `HLG (ICC name)` — JPEG ICC 描述
- `gain map 4.0× (ISO 21496-1)` — APP2 gain map（含 headroom）
- `sRGB (EXIF)` — 只有 EXIF ColorSpace
- `EXIF metadata only` — 有 EXIF 但沒有色彩訊號

要注意 `describe()` 報的是「實際做出決定的來源」，不是「檔案裡有哪個欄位」。ICC 名稱如果不是 BT.2100 標記（例如 `sRGB IEC61966-2.1`），transfer 其實來自 EXIF ColorSpace，所以 describe 是 `sRGB (EXIF)`。我第一版 test 假設成 `sRGB (ICC name)`，跑下去才發現並修正。

**修改的檔案：** `crates/spectrum-core/src/hdr.rs`、`crates/spectrum-core/src/scan.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-core/tests/describe.rs`（3 tests）、`crates/spectrum-gtk/tests/e2e.rs`（info 列斷言）

目前 `./test.sh` → 78 passed；快照連續 10 次執行 md5 相同（`2a55e4b77cebaf749a35f5171cfd1c1a`，快照選取的 fixture 是沒有 signal 的 `sample.png`，所以 info 列是空的，baseline 不變）。

---

## 2026-10-08 — MP4/MOV 的 codec 與 duration

**類型：** Feature

**問題：** 影片之前只讀到 transfer function，沒有 codec 與長度，info 面板對影片幾乎是空的。

**根因／做法：** 新增 `crates/spectrum-core/src/mp4.rs`：

- `VideoInfo { codec: Option<String>, duration_secs: Option<f64> }`（不能 derive `Eq`，`duration_secs` 是 `f64`）。
- `strict_boxes()`：嚴格 box 解析，要求 box 正好鋪滿 content（`i == data.len()`、每個 size ≥ 8）。
- `walk()` 沿用 `heif::container_skips`；`mvhd_duration()`：version 0 → timescale 在 content[12]、duration 在 content[16]；version 1 → timescale 20、duration 是 content[24] 的 64-bit。
- `stsd` 的 `entry_count` 起點有兩種寫法（plain box 與 full box），所以 `[0, 4]` 都試，並要求解析出的 box 數正好等於 count，避免把 entry 本體當成 box。

需要 `strict_boxes` 的原因：`container_skips` 對 generic container 會先試 skip 4，把 version/flags 當成「size = 0（延伸到檔案結尾）」的 box，這樣會產生一個假 box，它的 type 其實是第一個 child 的 type，`mvhd` 的欄位就會讀錯。

真實 fixture 驗證（`tests/fixtures/hlg_video.mp4`，FFmpeg 寫的）：codec `hvc1`、timescale 1000、duration 251 → **0.251 s**；`mdhd` 交叉驗證 60000 / 15015 = 0.25025 s，兩者一致。我原本以為是 261.1 s（把 hex 群組讀錯位置），test 跑出 0.251 後逐欄位核對才確認。

UI 的 info 列變成 `describe · codec · duration · EXIF 摘要`，例如 `HLG (nclx) · hvc1 · 0.3s`。

**修改的檔案：** `crates/spectrum-core/src/mp4.rs`（新）、`crates/spectrum-core/src/scan.rs`（`MediaEntry.video`）、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-core/src/fixture.rs`（`synthetic_mp4` 加 `mvhd`）、`crates/spectrum-core/tests/video_info.rs`（4 tests）、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`

目前 `./test.sh` → 82 passed；快照連續 10 次執行 md5 相同（`2a55e4b77cebaf749a35f5171cfd1c1a`）。

---

## 2026-10-08 — 縮圖快取加上 LRU 上限

**類型：** Feature

**問題：** `ThumbnailService` 的快取沒有上限，掃描大資料夾時記憶體會一直成長。

**根因／做法：** 加入 `with_capacity(max, capacity)`，內部用 `order: Vec<PathBuf>` 記錄使用順序（最久未用在前）：hit 時 `touch()` 把該檔移到末端，插入後超過 capacity 就移除頭部並計數 `evictions()`。`set_capacity()` 可以即時縮小並立刻修剪，`clear()` 用來把快取清成可預測的狀態。app 改用 `ThumbnailService::with_capacity(64, 256)`。

兩個要注意的點：
1. `get()` 的 hit 分支必須先 `let frame = c.frame.clone();` 再 `self.touch(path)`，否則 `self.cache.get(path)` 的 immutable borrow 還沒結束就呼叫 `&mut self` → `error[E0502]: cannot borrow *self as mutable because it is also borrowed as immutable`。
2. e2e 要先 `clear()` 再設 capacity：`App::new` 已經透過同一個 service 畫過所有縮圖（list 和 grid 各一次），計數會包含那些，直接斷言絕對值會失敗。

**修改的檔案：** `crates/spectrum-core/src/thumbnail.rs`、`crates/spectrum-core/tests/thumbnail_lru.rs`（3 tests）、`crates/spectrum-gtk/src/app.rs`（`paint_thumbnail` 公開 + capacity 256）、`crates/spectrum-gtk/tests/e2e.rs`（新 test `e2e_gtk_thumbnail_cache_is_bounded`）

目前 `./test.sh` → 86 passed；快照連續 10 次執行 md5 相同（`2a55e4b77cebaf749a35f5171cfd1c1a`）。

---

## 2026-10-08 — list/grid 可依名稱、日期、類型、HDR 排序

**類型：** Feature

**問題：** 清單和 grid 只有掃描順序（依路徑），沒有排序選項。

**根因／做法：** `MediaEntry` 加了 `mtime`（`scan_folder()` 從 `DirEntry::metadata().modified()` 取得），新增 `crates/spectrum-core/src/sort.rs`：`SortKey { Name, DateModified, Kind, Transfer }` 與 `sort_entries(entries, key) -> Vec<usize>`（回傳顯示順序的索引；比較完一律回退到小寫檔名，保證穩定且可重現）。UI 加 `gtk::DropDown`（Name / Date / Kind / HDR first），`apply_sort()` 重排 `state.entries` 並把 `selected` 維持在同一個檔案。

兩個坑：
1. `rebuild_store()` 讀的是 `state.entries`，所以必須先把排序結果寫進去再 rebuild。第一版先 rebuild 才更新 state，結果順序完全沒變（test 抓到了）。
2. `store.remove_all()` / `append` 會同步觸發 `items-changed` → `selected-notify` → handler 再去 borrow state → `RefCell already borrowed`，而這個 panic 在 GLib callback 內無法 unwind（`thread caused non-unwinding panic. aborting.`）。所以 `rebuild_store()` 先 clone entries，`apply_sort()` 也不跨越 store 操作持有 borrow。

另外 row 的重建發生在下一個 main-loop pass，所以 e2e 用輪詢（`settle()`）等順序穩定，不能 `set_selected()` 後立刻斷言。

**修改的檔案：** `crates/spectrum-core/src/sort.rs`（新）、`crates/spectrum-core/src/scan.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-core/tests/sort_order.rs`（5 tests）、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_sort_changes_row_order`）

目前 `./test.sh` → 92 passed；快照連續 10 次執行 md5 相同（`0ddd493d0acc4bfda9eaaedbd9d8c02a`，baseline 因為加入 dropdown 重新產生）。

---

## 2026-10-08 — list/grid 篩選（依類型 / 只看 HDR）

**類型：** Feature

**問題：** 只有排序沒有篩選，HDR 檔案混在一堆一般檔案裡不好找。

**根因／做法：** 新增 `crates/spectrum-core/src/filter.rs`：`Filter { kind: Option<MediaKind>, hdr_only: bool }`、`matches()`、`filter_entries(entries, filter) -> Vec<usize>`。`sort.rs` 加了 `sort_indices(entries, key, idx)`，讓「先篩後排」可以組合（`sort_entries` 變成 `sort_indices` 的滿集合特例）。

UI 左側加了 `filter_kind`（All kinds / Photos / Videos）和 `filter_hdr_only`（checkbox）。`State` 的模型改成 `entries`（掃描結果）+ `display: Vec<usize>`（顯示順序）+ `selected`（`display` 中的位置），讀取處一律走 `entry_at()`，不再假設 `entries[selected]`。

三個要注意的點：
1. `apply_view()` 必須在 `rebuild_store()` 之前就算出新的位置。`store.append()` 會把 `SingleSelection` clamp 到 0 並同步觸發 `selected-notify`，handler 會把 `state.selected` 蓋掉，結果 detail 停在第一列。
2. 被篩掉的檔案不能保持 `selected = None`：GTK 的 `SingleSelection` 在 model 非空時一定有一個位置。所以語意改成「選取的檔案被篩掉時，退回第一個可見項目」。
3. gtk4-rs 0.11.5 的 `CheckButton` 沒有 `active()` getter（只有 builder 的 `active(self, bool)`），要用 `b.property::<bool>("active")`；`set_label()` 收 `Option<&str>`。

**修改的檔案：** `crates/spectrum-core/src/filter.rs`（新）、`crates/spectrum-core/src/sort.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-core/tests/filter.rs`（3 tests）、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_filter_by_kind_and_hdr`）

目前 `./test.sh` → 96 passed；快照連續 10 次執行 md5 相同（`5d4fb80462ed17d55ef2c97285d9654e`，baseline 因加入篩選控件重新產生）。

---

## 2026-10-08 — 篩選加 transfer function 與狀態列

**類型：** Feature

**問題：** 篩選只有 `kind` 和 `HDR only`，無法單獨看 HLG 或 PQ；而且 UI 上看不出目前顯示了幾項、用的是哪個篩選和排序。

**根因／做法：** `Filter` 加 `transfer: Option<TransferFunction>`，`label()` 變成組合式（例如 `all kinds · HDR only · HLG`）。UI 左側加 `transfer_box`（Transfer: any / HLG / PQ）與 `status` 列，`refresh()` 裡更新成 `{可見} of {總數} · {filter} · sorted by {sort}`。

要注意 gain map 的 transfer code 是 `Unspecified`，所以 `HDR only` 會包含它、但 `HLG` / `PQ` 不會。這是刻意保留的差別：badge 顯示「判定來源」，filter 顯示「transfer code」。

**修改的檔案：** `crates/spectrum-core/src/filter.rs`、`crates/spectrum-core/tests/filter.rs`（+1 test，共 4）、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_transfer_filter_and_status`）

目前 `./test.sh` → 98 passed；快照連續 10 次執行 md5 相同（`6f7fc23dd47ebb093c924731df33c887`，baseline 因加入 status 列重新產生）。

---

## 2026-10-08 — 方向鍵導覽（邏輯寫成可測的 index 函數）

**類型：** Feature

**問題：** grid 只能用滑鼠點選。GTK4 沒辦法在 process 內注入按鍵（`Gtk.EventControllerKey` 的信號不能從測試端觸發），所以導覽邏輯必須能單獨測，key controller 只做接線。

**根因／做法：** 新增 `crates/spectrum-core/src/nav.rs`：

- `Direction { Up, Down, Left, Right, Home, End }`
- `next_index(current, n, columns, dir) -> Option<usize>` — Left/Right 走一格、Up/Down 走一列；**不 wrap**，超出範圍回 `None`（選取留在原位）
- `row_col(pos, columns) -> (usize, usize)`

UI 端 `move_selection()` 讀 `display.len()`、`selected`、`grid.min_columns()`，套用 `next_index` 後 `set_selected()`；`connect_key_pressed` 只把按鍵對應到 `Direction` 再呼叫它。

要注意兩件事：
1. gtk4-rs 0.11.5 的 `GridView` 沒有「實際欄數」getter（只有 `min_columns()`/`max_columns()`），所以導覽假設 `min_columns()`；test 必須先把這個值釘住。
2. `window.add_controller(key)` 要 by value（`IsA` 沒有對 `&T` 的 blanket impl）；`connect_key_pressed` 回 `glib::Propagation`（`Stop` = 已處理）。

**修改的檔案：** `crates/spectrum-core/src/nav.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-core/tests/nav.rs`（5 tests）、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_keyboard_navigation`）

e2e 走的路徑：6 個檔案、3 欄 → Down 0→3、Right→4、Up→1、Home→0、End→5，再 Down 超過尾端必須回 `None` 且選取留在 `f.png`。

目前 `./test.sh` → 104 passed；快照連續 10 次 md5 相同（`6f7fc23dd47ebb093c924731df33c887`，UI 沒變所以 baseline 不變）。

---

## 2026-10-08 — ICC profile 的 `cicp` / `dmnd` tag（profile 名字不再是唯一線索）

**類型：** Feature

**問題：** JPEG 的 HDR 判斷只看 ICC profile **名字**裡有沒有 `BT.2100` / `HLG` / `PQ` 字樣。但 ICC.2 新增了正式的 `cicp` tag（直接寫 primaries / transfer / matrix / full-range），還有 `dmnd`（display condition，媒體白點 cd/m²）。實例：profile 名字是 `Display P3`（完全沒有 2100 字樣）但 `cicp.transfer = 18` → 這才是真正該走 HLG 的來源。

**根因／做法：** 新增 `crates/spectrum-core/src/icc.rs`：

- `IccInfo { name, cicp, media_white_point }`、`parse_icc(profile) -> Option<IccInfo>`
- profile base 由 `acsp`（offset 36）反推，tag count 在 128、tag entry 在 132（sig 4 + offset 4 + size 4），offset 相對 base
- `cicpType`（ICC.2 CICP amendment）：signature(4) + reserved(4) + primaries(1) + transfer(1) + matrix(1) + full_range(1) = 12 bytes，code 與 HEIF `nclx` 相同（PQ=16、HLG=18）
- `dmnd`：dataType(4) + 媒體白點 u32；>= 1000 cd/m² 視為 HDR（PQ）
- `desc`/`dscm` 的 count 位置寫成 `usize::from(data.starts_with(b"desc")) * 8`：正規 profile 以 type signature 開頭，但部分寫入器把 count 放在 0

`SignalSource` 增加 `IccCicp`、`IccMediaWhitePoint`；`signal_for_jpeg()` 優先序變成 **gain map → cicp → CustomRendered==3 → dmnd >= 1000 → ICC name → EXIF ColorSpace**。`JpegInfo.icc_name` 改成 `icc: IccInfo`（`tests/real_fixtures.rs:58` 跟著改）。

實測驗證：掃 `/usr/share/color/icc/ghostscript/*.icc` 共 52 個真實 profile，`acsp` 都在 36、tag count 都在 128。

**修改的檔案：** `crates/spectrum-core/src/icc.rs`、`crates/spectrum-core/src/hdr.rs`、`crates/spectrum-core/src/jpeg.rs`、`crates/spectrum-core/src/fixture.rs`（`ExifFixture` 增加 `cicp`/`dmnd`，`icc_profile()` 寫出正規 descType + cicp/dmnd tag）、`crates/spectrum-core/tests/icc_cicp.rs`（5 tests）、`crates/spectrum-gtk/tests/e2e.rs`（新增 `cicp_hlg.jpg` fixture，10 項）

e2e 守住的行為：名字 `Display P3` + `cicp(9,18,0,0)` 的檔案 badge 必須是 `HLG`、info 是 `HLG (ICC cicp)`；且優先序不能退步 — `app2_gainmap.jpg` 仍是 `gain map 4.0×`、`hlg_icc.jpg` 仍是 `HLG (ICC name)`、`mislabeled.jpg` 仍是 `SDR`。

目前 `./test.sh -- --test-threads=1` → 109 passed（core 95 + egui e2e 2 + gtk e2e 14，其中新增 `icc_cicp.rs` 5 tests）；快照連續 10 次 md5 相同（`6f7fc23dd47ebb093c924731df33c887`）。另外把 baseline 故意改一個像素驗證比較是真的會失敗（`snapshot differs from baseline (23010 vs 19210 bytes)`），確認後已還原。

## 2026-10-09 — HEIF 也能讀 ICC profile（cicp / dmnd）

**類型：** Feature

**問題：** `detect_signal()` 對 HEIF 只看 `colr` 裡的 `nclx` box。但 HEIF 也可以放 ICC profile（`colr` 的 `prof` 類型或 `iccp` box），profile 裡的 `cicp` / `dmnd` 同樣能表達 HDR。常見情況是 `nclx.transfer = 2`（unspecified）但 profile 有 `cicp` → 這時應該由 profile 決定。

**根因／做法：**

- `crates/spectrum-core/src/heif.rs` 新增 `find_icc(data)`：`colr` 的 `colour_type == "prof"` → profile 是 content[4..]；`iccp` 是 full box（size + type + flags + data）→ content[4..]；`nclx` 的 `colr` 跳過。
- `crates/spectrum-core/src/hdr.rs` 新增 `icc_signal(icc)` 與 `signal_for_heif(nclx, icc)`，優先序：**nclx（transfer 非 unspecified）→ ICC cicp → ICC dmnd >= 1000 cd/m² → ICC name**。nclx 只有 `transfer = 2` 時等於「沒說 transfer」，所以讓 profile 決定。
- `scan.rs` 的 heif/mp4 分支改用 `signal_for_heif(find_colr(..).and_then(parse_colr), find_icc(..).and_then(parse_icc))`，並新增 `detect_icc(path)`。
- `fixture.rs` 新增 `synthetic_heif_with_icc()`、`synthetic_heif_iccp()`、`write_heif_fixture_with_icc()`（box-only，不含像素，所以 repo 不會有真實照片）。

**修改的檔案：** `crates/spectrum-core/src/heif.rs`、`crates/spectrum-core/src/hdr.rs`、`crates/spectrum-core/src/scan.rs`、`crates/spectrum-core/src/fixture.rs`、`crates/spectrum-core/tests/heif_icc.rs`（6 tests）、`crates/spectrum-gtk/tests/e2e.rs`（新增 `heif_icc_cicp.heic` fixture，11 項）

e2e 守住：`heif_icc_cicp.heic`（nclx 2/2 + profile `Display P3` + cicp(9,18,0,0)）badge 必須是 `HLG`、info 必須是 `HLG (ICC cicp)`；排序位置 3、badge-on 清單同步更新。

---

## 2026-10-09 — 側欄 list 被擠到 46 px，快照看不到縮圖

**類型：** Bug Fix

**問題：** 快照基線裡側欄完全沒有縮圖。探測發現側欄的 `ScrolledWindow` 只有 **46 px** 高（`GtkScrolledWindow bounds=(0,153,240,46)`）：2 個 DropDown + 1 個 CheckButton + 4 個 Button + 2 個 Label 把 list 擠到只能顯示 1 列。

**根因／做法：**

- `crates/spectrum-gtk/src/app.rs`：`list_scroll.set_min_content_height(200)`，rotate/flip/save 三個按鈕合成一個橫向 `edit_row`（`set_homogeneous(true)`），視窗預設尺寸 900×420 → 900×520。
- 快照測試改為先把可解碼的那列捲進可视區再斷言：row 的 `bounds()` 是 list 自己的座標系，所以可見性要拿 **scroll offset**（`vadjustment().value()`）比較，不是拿 viewport 的 native bounds 比較（一開始用後者會誤判 `row (0,432,348,23) viewport (0,0,348,200)`）。
- 新增 `red_pixels(path, max_x)`（用 `gdk_pixbuf::Pixbuf` 讀快照 PNG）斷言側欄確實渲染出縮圖 — 只斷言「row 可見」不夠，picture 節點要有真正的 paint pass 才會進快照，所以測試在斷言前 `queue_draw()` + pump 60 次。
- `list_scroll.set_overlay_scrolling(false)`：overlay scrollbar 的淡入/淡出是動畫，會讓快照 byte 數在 29090 / 28980 之間跳（10 次跑約 2 次不一致）；關掉後 10/10 相同。

**修改的檔案：** `crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_snapshot_matches_baseline` 加 viewport 高度、可見性、縮圖渲染三個斷言）

目前 `./test.sh -- --test-threads=1` → **115 passed**；快照 900×483、側欄縮圖 48 個紅像素、連續 10 次相同。

## 2026-10-09 — 子資料夾瀏覽與封面檔（port macOS `FolderReader.listSubfolders`）

**類型：** Feature

**問題：** Rust 版只能掃描單一資料夾。macOS 版的 `FolderReader.listSubfolders` 會列出子資料夾並附上封面（`coverPath` / `coverDate`），規則是：非遞迴、跳過隱藏檔、**圖片優先於其他媒體**、同群組內取字典序最小者（macOS 用 O(n) min-scan 而非整份排序，因為大目錄排序比較器要 CFURL→String bridge，成本顯著）。

**根因／做法：**

- 新增 `crates/spectrum-core/src/cover.rs`：
  - `first_cover(dir) -> Option<(PathBuf, SystemTime, MediaKind)>`：image 群組贏過 media 群組（即使 media 名字更小），同群組取最小名，跳過 `.` 開頭的檔案，非遞迴。
  - `list_subfolders(dir) -> Vec<SubfolderInfo>`：依名稱排序保證 UI 順序穩定。
  - `cover_label(info)`：`folder · cover-file` 或 `folder · (no cover)`，這是列的可讀標籤。
- `crates/spectrum-gtk/src/app.rs`：`State` 加 `folder`，`Ui` 加 `subfolders` ListBox 與 `subfolder_paths`（列索引 → 路徑，避免解析標籤字串）；`rebuild_subfolders()` 重建列，`open_folder()` 重新掃描 + `apply_view()`，且 `path == state.folder` 時直接回傳。
- 信號改用 **`row-activated`** 而不是 `row-selected`：Single selection 的 ListBox 在 `append()` 第一列時會自動選取並發出 `row-selected`，初始建立就會誤導航進去（實測：初始 2 列被 `open_folder(a-folder)` 清成 0 列）。

**修改的檔案：** `crates/spectrum-core/src/cover.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_subfolder_cover_and_navigation`）

e2e 守住：`a-folder`（`z.png` + `a.mp4`）列標籤必須是 `a-folder  ·  z.png`（圖片贏過字典序更小的 `a.mp4`）、`b-folder` 是 `b-folder  ·  (no cover)`；點擊 `a-folder` 後 `state.folder` 變成該資料夾、entries = 2、status `2 of 2`、且因為 a-folder 沒有子資料夾，subfolder 列必須變空；再經同一入口打開 `b-folder` → entries 0、detail `(no selection)`。

快照基線因為側欄多了 subfolder 區而更新（900×609，側欄縮圖 48 紅像素），連續 10 次相同。

目前 `./test.sh -- --test-threads=1` → **123 passed**（GTK e2e 15 tests、core cover 7 tests）。

## 2026-10-09 — 檔名/資料夾搜尋（port macOS `SearchResultsView`）

**類型：** Feature

**問題：** 沒有搜尋。macOS 版 `.searchable()` → `SearchResultsView` 同時比對照片檔名與資料夾名稱。

**根因／做法：**

新增 `crates/spectrum-core/src/search.rs`，語義照 macOS `search()` + `walkDirectory()`：

- query 先 trim，**長度 < 2 直接不回結果**（macOS `guard trimmed.count >= 2`）
- 大小寫不敏感的 substring 比對
- **遞迴**走訪（與 `scan_folder` 的非遞迴不同），跳過 `.` 開頭的檔案，只收媒體檔（`kind_for`）
- 資料夾比對用 `HashSet<PathBuf>` 去重
- 照片先依走訪順序取前 **200** 筆、**再**按 mtime 遞減排序（順序很重要：cap 是「走訪到的前 200」，不是「最新的 200」）

UI：`Ui` 加 `search: gtk::Entry`、`results: gtk::ListBox`、`result_rows`（列索引 → 路徑 + is_folder）、`results_scroll` / `sub_scroll`。`run_search()` 在 query 太短時隱藏結果區、顯示 subfolder 區；否則結果區取代 subfolder 區（資料夾在前、照片在後，標籤 `name · folder` 或 `name · photo|video`）。`open_and_select()` 打開檔案的父資料夾再選取該檔。信號用 `connect_changed`（`set_text` 會觸發 `changed`，`search-changed` 不會）。

**修改的檔案：** `crates/spectrum-core/src/search.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_search_results_and_navigation`、`plain_row_text`）

e2e 守住：1 字元 query → 結果區隱藏、subfolder 區可見；`"hlg"` → 3 列，第一列 `hlg-sub · folder`，照片列 `hlg_nested.png · photo`、`hlg_photo.png · photo`；點擊照片列後 `detail` 以 `hlg_photo.png` 開頭且資料夾仍是 root；點擊資料夾列 → 進入 `hlg-sub`（1 entry）。

`plain_row_text()` 是必要的：`row_label_text()` 假設列的 child 是 `gtk::Box` 且最後一個 child 是 Label，而搜尋/subfolder 列直接把 `gtk::Label` 當 child（之前回空字串）。

**快照不穩定（已修）：** 解碼像素後差異恰好是 19 px，在 x=9、y=46..64 的垂直線（基線 `(0,0,0)` vs 不穩定 `(50,50,50)`）→ **GtkEntry 閃爍游標**。單測模式 `--exact e2e_gtk_snapshot_matches_baseline` 下 30 次跑 11 次不同。`set_focusable` 在 gtk4-rs 0.11.5 沒有綁定（只有 `gtk4-sys` 的 `gtk_widget_set_focusable` / `gtk_widget_grab_focus`），FFI 需要 upcast：`let entry: gtk::Widget = app.ui.search.clone().upcast();`。單獨 `set_focusable(false)` **不會**停游標；`set_editable(false)` 會（不可編輯的 GtkEntry 不畫游標）→ 基線重新產生後單測模式 20/20、全測 20/20 穩定。

另有一種不穩定（30 次跑 1 次）：`snapshot differs from baseline (27414 vs 37479 bytes)` + `snapshot has no rendered thumbnail in the sidebar`，是 picture 還沒 paint pass（舊問題，與游標無關）。

目前 `./test.sh -- --test-threads=1` → **130 passed**（core 110、egui e2e 2、GTK e2e 16）。基線 `crates/spectrum-gtk/tests/snapshots/gtk_detail.baseline.png` = 900×775，側欄縮圖 48 紅像素。

## 2026-10-09 — 時間軸：按月份分組（降序）+ 月份過濾

**類型：** Feature

**問題：** macOS 版的時間軸網格會「按月份分組、降序排列」，Rust 版沒有分組，也沒有按月份的過濾。

**根因／做法：**

新增 `crates/spectrum-core/src/timeline.rs`：

- `month_key(t: SystemTime) -> String` 用 **UTC** 日曆（跨機器/時區可重現），格式 `YYYY-MM`；epoch 前的時間回 `unknown`
- `group_by_month(entries, order)`：`order` 是顯示順序（已過濾+排序），群組內保留該順序，群組依 key **降序**；`unknown` 群組強制排最後（因為 `unknown` 字典序比 `2026-10` 大，單純 `cmp` 會把它排最前）
- `group_label(group)` → `2026-10  ·  3`（UI 列文字）

`civil_from_days`（Howard Hinnant）實作時月份對應寫錯：正確是 `m = mp + 3`（`mp < 10`）否則 `mp - 9`；寫成 `mp + 1` 時 30000 個隨機日期對照 `datetime.date` 有 16776 個錯，修正後 0 個錯。

`Filter` 加 `month: Option<String>`，`matches()` 比對 `month_key(e.mtime)`，`is_empty()`/`label()` 一起更新（label 變成 `all kinds · 2026-10`）。

UI：`Ui` 加 `timeline: gtk::ListBox` + `timeline_months`（列索引 → key，`""` = 不過濾）+ `timeline_scroll`（`set_min_content_height(100)`、`set_overlay_scrolling(false)`）。`rebuild_timeline()` 在 `apply_view()` 內重建（第 0 列是 `all months · N`），`select_month()` 設 filter 後 `apply_view()`。信號用 `connect_row_activated`（同 subfolder：Single selection 的 ListBox 在 `append()` 時會自動選取第一列，`row-selected` 會造成重入）。

注意 `rebuild_timeline()` 不能同時持有 `timeline_months` 的 borrow 又呼叫 `append_timeline()`（會 `already borrowed` panic）→ 先 `clear()` 再逐列 append。

**修改的檔案：** `crates/spectrum-core/src/timeline.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-core/src/filter.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_timeline_groups_and_month_filter`、`set_mtime` 測試輔助）、`crates/spectrum-core/tests/timeline.rs`、`crates/spectrum-core/tests/filter.rs`

e2e 守住：固定 mtime（`a_oct.png` = 1791158400 → 2026-10、`b_jun.png` = 1780272000、`c_jun.png` = 1780358400 → 2026-06）→ 時間軸列必須是 `all months · 3`、`2026-10 · 1`、`2026-06 · 2`（降序）；點擊 `2026-10` 列 → status 含 `1 of 3` 與 `2026-10`、display = `[0]`；點擊 `all months` 列 → status `3 of 3` 且不含月份。

快照測試另加時間軸守衛：poll 到 `timeline.height() > 0`（allocation 只在 paint pass 後存在）後要求 `>= 100`，且第 0 列以 `all months` 開頭——避免側欄被擠壓的問題再次出現。

目前 `./test.sh -- --test-threads=1` → **138 passed**（GTK e2e 17、core timeline 6、core filter 5）。基線更新為 900×881（側欄多了時間軸區），連續 12 次相同，側欄縮圖 48 紅像素。

## 2026-10-09 — 多選（Ctrl/Shift）模型 + 狀態列顯示選取數量

**類型：** Feature

**問題：** macOS 版支援 Cmd/Shift 多選，Rust 版只有單選。

**根因／做法：**

新增 `crates/spectrum-core/src/select.rs`：

- `ClickMods { ctrl, shift }`、`Selection { primary: Option<usize>, set: Vec<usize> }`（`set` 永遠升序且唯一）
- `apply_click(sel, index, mods) -> Selection`：
  - 一般點擊 → `set = [index]`、`primary = index`
  - Ctrl 點擊 → toggle（已在選取內則移除）
  - Shift 點擊 → 從 anchor 到該列（含兩端）**取代**整個 set，且**不移動 anchor**（Finder 語意：anchor 是起點，Shift 只延伸範圍；實作第一版每次點擊都改 `primary`，導致第二次 Shift 從上一次的目標延伸，測試失敗後修正）

UI：`Ui` 加 `multi: Rc<RefCell<Selection>>` 與 `mods: Rc<RefCell<ClickMods>>`；`EventControllerKey` 的 `connect_key_pressed` / `connect_key_released` 追蹤 Ctrl_L/R、Shift_L/R（`connect_key_released` 的回車型必須回 `()`，不能回 `glib::Propagation`）。`ui.selection.connect_selected_notify` 內呼叫 `record_click()`，狀態列在 `count() > 1` 時加上 ` · selected N`。

`record_click()` 是必要的：`set_selected()` 只在值改變時才發 notify，所以啟動時選取第 0 列、`move_selection()`、`open_and_select()` 這些程式化選取不會產生記錄 → 模型與畫面不一致（測試第一次跑就是 `startup selection = []`）。因此三個 `set_selected` 點與 `App::new` 都改走 `record_click()`。

**修改的檔案：** `crates/spectrum-core/src/select.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_multi_select_with_modifiers`）、`crates/spectrum-core/tests/select.rs`

e2e 守住：啟動 = `[0]`；一般點擊 1 → `[1]`；Ctrl 點擊 2 → `[1,2]` 且狀態列含 `selected 2`；Shift 從 anchor(2) 延伸到 0 → `[0,1,2]`；Ctrl toggle 1 → `[0,2]`；一般點擊 2 → `[2]` 且狀態列不再顯示數量。

已知限制：視覺高亮仍是單列（`gtk::SelectionMode::Single`），多選目前只有模型與狀態列計數；要真正多列高亮需要把 grid 的 model 換成 `gtk::MultiSelection`，但 ListBox 與 GridView 共用同一個 selection model 的現況會分裂，列為後續工作。

新增 `e2e_gtk_snapshot_multi_select`：選取兩列後狀態列必須含 `selected 2`，並比對 `crates/spectrum-gtk/tests/snapshots/multi_select.baseline.png`（900×881，側欄 3 列縮圖 = 144 紅像素）。

目前 `./test.sh -- --test-threads=1` → **146 passed**（core 125、egui e2e 2、GTK e2e 19）。`gtk_detail` 基線未變（單選時狀態列沒有計數），兩個快照各自連續 10~12 次相同。

## 2026-10-09 — 刪除到垃圾桶（freedesktop Trash）+ 快照需等圖片尺寸穩定

**類型：** Feature

**問題：** macOS 版刪除會優先進垃圾桶；Rust 版沒有刪除功能。

**根因／做法：**

新增 `crates/spectrum-core/src/trash.rs`：

- `delete_paths(paths, root, time) -> Vec<(PathBuf, DeleteOutcome)>`，`DeleteOutcome = Trashed(dest) | Removed | Failed(String)`
- 依 freedesktop 規範：檔案移到 `<root>/files/<name>`，並寫 `<root>/info/<name>.trashinfo`：
  ```
  [Trash Info]
  Path=<絕對路徑>
  Time=<ISO 8601 UTC>
  ```
- 名稱衝突依規範加 ` 1`、` 2`…（整個名字加後綴，不拆副檔名）
- `root` 由參數傳入；`SPECTRUM_TRASH_ROOT` 可覆蓋 → e2e 不會動到使用者真正的垃圾桶
- 垃圾桶無法建立（例如 root 的父目錄是檔案）→ 退回永久刪除，記錄為 `Removed`
- `iso8601_utc(t)` 共用 `timeline.rs` 的 `civil_from_days`（改成 `pub(crate)`），避免重複實作

UI：`Ui` 加 `delete` 按鈕（`Delete to trash`，放在 edit_row）。點擊後用 `ui.multi` 的全部索引（上一輪的多選模型）取得路徑，刪除後必須**重新 scan**：`open_folder()` 有 `if folder == path { return; }` 的守衛，同一個資料夾不會重掃，所以清單會停在已刪除的列。另外要先清空 `multi`（索引已失效）再 `apply_view()`。

**修改的檔案：** `crates/spectrum-core/src/trash.rs`、`crates/spectrum-core/src/timeline.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_delete_selected_rows_to_trash`、`red_pixels_total`、`list_row_count`）、`crates/spectrum-core/tests/trash.rs`

e2e 守住：選取 2 列 → 按刪除 → 狀態列 `deleted: 2 to trash`、清單 0 列、`multi` 清空、`trash/files/{a.png,b.png}` 與 `trash/info/*.trashinfo` 存在且 `Path=` 指向原路徑。

快照穩定性新發現：`GtkPicture` 由 glycin 非同步填入，視窗會跟著圖片的自然尺寸長大 → 同一測試單獨跑是 900×881、在完整套件裡是 900×920（差 205k 像素）。只檢查「有紅像素」不夠，必須**poll 到 picture 的 width/height 連續 10 次不變**再快照。另外 `gtk4-rs` 0.11.5 沒有 `ListBox::row_count()` / `child_count()`，測試改用 `row_at_index(i)` 逐列數（`list_row_count`）。

目前 `./test.sh -- --test-threads=1` → **154 passed**（core 132、egui e2e 2、GTK e2e 20）。兩個快照：`gtk_detail.baseline.png` 900×881（側欄多了 Delete 按鈕）、`multi_select.baseline.png` 900×881；完整套件連續 10 次、單獨跑連續 6 次皆相同。

## 2026-10-09 — 剪貼簿（Copy/Cut）+ 貼上 + 子資料夾重新命名

**類型：** Feature

**問題：** macOS 版有資料夾管理（複製/剪下/貼上/重新命名），Rust 版沒有。

**根因／做法：**

新增 `crates/spectrum-core/src/clipboard.rs`：

- `ClipboardMode { Copy, Cut }`、`Clipboard { mode, paths }`、`set_clipboard(paths, mode)`
- `paste(clip, dest_dir) -> Vec<PasteOutcome>`：Copy 保留來源（`fs::copy`），Cut 移動（`fs::rename`）；衝突沿用垃圾桶的 ` 1` 計數規則（`trash::unique_name` 改成 `pub(crate)` 共用）
- `rename_dir(path, new_name) -> Result<PathBuf, String>`：拒絕空字串、含 `/` 或 `\0` 的名字、非目錄、目標已存在
- 注意 `fs::copy` 回 `u64`、`fs::rename` 回 `()`，`if/else` 會型別不符 → 用 `match` 且 `fs::copy(...).map(|_| ())`

UI：`Ui` 加 `clipboard`、`copy`/`cut`/`paste`/`rename` 按鈕與 `rename_entry`（`set_editable(false)`，避免閃爍游標讓快照不穩定）。`set_clipboard_from_selection()` 從上一輪的多選模型取路徑（原本是 closure 被兩個 `connect_clicked` 共用會 move → 改成頂層函數）。Paste 後重新 `scan_folder` + `apply_view()` 並清空 `multi`；狀態列顯示 `pasted copied/moved: N ok, M failed`。Rename 成功會 `open_folder()` 到新路徑，失敗只在狀態列顯示 `rename failed: ...`。

**修改的檔案：** `crates/spectrum-core/src/clipboard.rs`、`crates/spectrum-core/src/trash.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_clipboard_copy_cut_paste_and_rename`）

e2e 守住：Copy → paste 到同一資料夾產生 `a.png 1`（衝突計數）；Cut → 開 `sub/` → paste 移動檔案且來源消失；`rename_entry` 設 `sub-renamed` → 目錄存在且 `state.folder` 跟著新路徑；設 `bad/name` → 狀態列 `rename failed: invalid name`。

快照：兩個基線都要重新生成（側欄多了按鈕列）。另外 `multi_select` 基線在完整套件下原本會差 205k 像素，因為 `GtkPicture` 由 glycin 非同步填入、視窗跟著自然尺寸長大 → 加入「poll 到 picture 的 width/height 連續 10 次不變」才快照；重新生成基線後完整套件 10 次、單獨跑 6 次皆相同（現在是 900×1000）。

目前 `./test.sh -- --test-threads=1` → **161 passed**（core 138、egui e2e 2、GTK e2e 21）。

## 2026-10-09 — 依檔案大小排序 + 排序方向（升冪/降冪）

**類型：** Feature

**問題：** 排序只有 Name/Date/Kind/HDR first，且方向固定（Date 永遠最新優先），無法反轉；也沒有依大小排序。

**根因／做法：**

`crates/spectrum-core/src/sort.rs`：

- `SortKey` 加 `Size`（自然順序 = 最大優先）
- 新增 `SortDirection { Natural, Reversed }`（`Natural` 是各 key 的預設順序，所以 Date 仍是最新優先、既有測試不變；`Reversed` 連 name tie-break 一起反轉）
- `sort_indices(entries, key, idx, dir)` 新增第 4 個參數；`sort_entries_dir(entries, key, dir)` 為方便封裝
- `MediaEntry` 加 `size: u64`，`scan_folder` 從 `item.metadata()` 一次讀 mtime + size

UI：`State` 加 `sort_dir`，`Ui` 加 `reverse` 按鈕（點擊切換 Natural/Reversed 並 `apply_view()`），排序下拉多一個 `Size`（index 4）。狀態列變成 `... · sorted by {key} ({direction})`。

**修改的檔案：** `crates/spectrum-core/src/sort.rs`、`crates/spectrum-core/src/scan.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（新增 `e2e_gtk_sort_by_size_and_direction`，並更新 4 處狀態字串斷言）

e2e 守住：固定 byte 數的三個 fixture（a=100、b=300、c=200）→ 初始 Name 順序 `[0,1,2]`；選 Size → `[1,2,0]` 且狀態含 `sorted by Size (natural)`；點 reverse → `[0,2,1]` 且含 `(reversed)`；換回 Name 時方向仍保留 → `[2,1,0]`。

核心測試 `crates/spectrum-core/tests/sort_size_and_direction.rs` 6 個：Size 自然/反轉、Name 反轉、Date 自然仍是最新優先、同 size 落到 name tie-break（反轉時也反轉）、`scan_folder` 記錄 size。

目前 `./test.sh -- --test-threads=1` → **168 passed**（core 144、egui e2e 2、GTK e2e 22）。兩個基線重新生成後完整套件連續 10 次皆相同。

## 2026-10-09 — 多選視覺高亮（MultiSelection + 從核心模型鏡射）

**類型：** Feature / Bug Fix

**問題：** 多選模型可以有多個索引，但畫面只高亮一行：list 是 `SelectionMode::Single`，grid 的模型也是 `SingleSelection`。

**根因／做法：**

- `Ui.selection` 改成 `gtk::MultiSelection`，grid 直接用這個模型高亮。
- `gtk_list_box_set_selection_model` 在 gtk4-rs 0.11.5 **沒有綁定**（`set_selection_model` 不存在），所以 ListBox 仍用自己的內部模型：把它改成 `SelectionMode::Multiple`，再透過已綁定的 `unselect_all()` + `select_row(row)` 把核心 `select::Selection` 鏡射上去。
- `record_click(state, ui, idx, mods)` 變成唯一路徑：算出核心 set → 鏡射到 grid 模型與 list rows → 更新 `state.selected` → `refresh()`。測試沒有按鍵注入，所以 e2e 也走同一條路徑。
- `syncing` 旗標：鏡射時 `select_item` / `select_row` 會觸發 `selection-changed` / `row-selected`，不加旗標會被當成使用者點擊（遞迴）。
- `item_count`：`App::new` 直接 `store.append`（沒經過 `rebuild_store`），鏡射需要這個長度。

**順手修掉的 bug：** `apply_view()` 的 fallback 寫成 `order.first()`，但 `order` 存的是 entry index，不是 display position → 被篩掉時 `selected` 變成 entry index（例如 2）。之前測試會過是因為 `SingleSelection.set_selected(2)` 被 clamp 到 `n_items-1 = 0`；改用 MultiSelection 後不會 clamp，所以必須寫成「position 0」。

**修改的檔案：** `crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`

e2e 守住：`e2e_gtk_multi_select_with_modifiers` 每一步都比較核心 set 與兩個視圖的高亮（grid 用 `is_selected(i)`，list 用 `selected_rows()`）；`e2e_gtk_snapshot_multi_select` 快照前要求兩個視圖都是 `[0, 2]`。另外 `e2e_gtk_delete_selected_rows_to_trash` 原本只改 `ui.mods` 卻傳 `ClickMods::default()`，Ctrl 語意沒生效 → 改為直接傳 `ClickMods { ctrl: true, shift: false }`。

快照：兩個基線重新生成。高亮色 (53,132,228) 像素：單選基線 11128、多選基線 39202（兩行），證明多行高亮真的渲染出來。

目前 `./test.sh -- --test-threads=1` → **168 passed**（core 144、egui e2e 2、GTK e2e 22），完整套件連續 10 次皆相同。

## 2026-10-09 — 子資料夾依 cover 日期排序（macOS `subfoldersAreInOrder`）

**類型：** Feature

**問題：** 側邊欄的子資料夾是字母序，但 macOS 版是「最新 cover 日期優先，同日期字母序，沒有 cover 的排最後」（`PhotoGridView.subfoldersAreInOrder`）。

**根因／做法：**

- `crates/spectrum-core/src/cover.rs` 新增 `subfolders_are_in_order(a, b) -> bool` 與 `sort_by_cover(infos) -> Vec<SubfolderInfo>`。
- `list_subfolders()` 保持字母序（原始清單必須可預測），UI 順序由 `sort_by_cover()` 決定。
- `crates/spectrum-gtk/src/app.rs` 的 `rebuild_subfolders()` 改用 `sort_by_cover(&list_subfolders(&st.folder))`；`subfolder_paths` 也照排序後的順序 push，所以列索引與路徑仍然對得上。

**修改的檔案：** `crates/spectrum-core/src/cover.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`

e2e 守住：`e2e_gtk_subfolders_sorted_by_cover_date`（`/tmp/spectrum-subfolder-sort-fixtures`，5 個子資料夾、固定 mtime）要求順序為 `z-newest`、`m-middle`、`a-oldest`、`b-tie`（同日期 → 字母序）、`c-empty`（無 cover → 最後）；並檢查 `subfolder_paths` 順序與列索引一致，activate 第 0 列必須進入 `z-newest`（不是字母第一的 `a-oldest`）。快照基線 `subfolder_sort.baseline.png`。

核心測試 `crates/spectrum-core/tests/cover_sort.rs` 6 個：最新日期優先、同日期字母序、無 cover 排最後（即使名字字母更前面）、全無 cover 時字母序、排序是輸入的排列、真實目錄的端到端順序。

目前 `./test.sh -- --test-threads=1` → **175 passed**（core 150、egui e2e 2、GTK e2e 23），完整套件連續 10 次、新測試單獨跑連續 12 次皆穩定。

## 2026-10-09 — FolderListCache（子資料夾清單快取，macOS §4.10）

**類型：** Feature

**問題：** 每次刷新視圖都會重讀目錄。macOS 的 `FolderListCache` 把子資料夾清單放在記憶體，加上「本次 session 已掃描」旗標，三個條件同時滿足才 early-return，避免重複 I/O。

**根因／做法：**

新增 `crates/spectrum-core/src/folder_cache.rs`：

- `memory: HashMap<PathBuf, Vec<SubfolderInfo>>` + `scanned: HashSet<PathBuf>`
- `set_entries()` 同時標記為已掃描（對應 macOS `setEntries`）
- `can_reuse()` 檢查三個條件：已掃描、快取存在、每個快取中的 coverPath 檔案仍存在（cover 被刪代表子資料夾被刪 → 強制重掃）
- `invalidate(parent)` 清除快取與已掃描旗標；`clear()` 清空全部
- `entry_for_child(child, under)` 從 parent 的快取查某個子資料夾
- `list()` = 可重用則回傳快取，否則重掃並寫入；`sorted()` = 快取 + `sort_by_cover()`（UI 順序）

UI：`Ui` 新增 `cache: Rc<RefCell<FolderListCache>>`，`rebuild_subfolders()` 改用 `cache.sorted(&folder)`。刪除、貼上、重命名後對受影響的目錄 `invalidate()`（重命名要 invalidate **parent**，因為 parent 的快取 entry 還寫著舊名字）。

**快照不穩定的根因：** `set_editable(false)` 必須在第一次 paint 之前設定。原本寫在 snapshot 前 → 完整套件中仍出現游標（x=9、y=46..64 共 19 px 差異，單跑穩定但套件跑不穩定）；移到 `App::new` 之後、settle 之前就不會再變。

**修改的檔案：** `crates/spectrum-core/src/folder_cache.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`

e2e 守住：`e2e_gtk_folder_list_cache_is_reused_until_invalidate`（`/tmp/spectrum-folder-cache-e2e`）——初始列為 `b`、`a`（依 cover 日期）；確認 sidebar 建立時確實用過快取（`is_scanned`）；掃描後新增的 `c` 不會出現（快取重用）；`invalidate()` 後重掃，順序變 `b`、`c`、`a`。基線 `folder_cache.baseline.png`。

核心測試 `crates/spectrum-core/tests/folder_cache.rs` 7 個：掃描後快取導致新目錄不可見、set_entries 設定已掃描旗標、invalidate 強制重掃、cover 被刪強制重掃（且 cover 變 None）、entry_for_child 查詢、clear 清空、sorted 使用 cover 日期而快取本身維持字母序。

目前 `./test.sh -- --test-threads=1` → **183 passed**（core 157、egui e2e 2、GTK e2e 24），完整套件連續 10 次、新測試單獨跑連續 12 次皆穩定。

## 2026-10-09 — 編輯歷史（undo/redo）

**類型：** Feature

**問題：** 套用的 edit op（crop/rotate/flip）無法撤回。`Crop` 沒有可從單一 op 推導的反向 op（需要知道之前的 crop 範圍），所以「反向 op」做法不成立。

**根因／做法：**

新增 `crates/spectrum-core/src/undo.rs`：

- `Change { path, ops_before, ops_after }` — 記錄「变更前後的 ops 清單」而不是反向 op，語義完全精確（ops 清單就是 UI 實際渲染的內容）
- `UndoStack` 兩堆疊：`record()` 推入 undo 並**清空 redo**（標準行為）；`undo()` 彈出到 redo 堆；`redo()` 彈回 undo 堆
- `record()` 跳過 no-op（`ops_before == ops_after`），避免空變更污染歷史
- `apply(path, ops_before, op)` 記錄並回傳新的 ops 清單

UI（`crates/spectrum-gtk/src/app.rs`）：`Ui` 新增 `history: Rc<RefCell<UndoStack>>`、`undo`、`redo` 按鈕（sidebar 新增 `history_row`）。`apply_op()` 在 push op 前記錄變更。`undo_last()` / `redo_next()` 還原 ops，並用 `select_path()` 把選取移到該變更所屬的檔案（若該檔被過濾掉則不動）。status 在歷史非空時追加 ` · history {u} undo, {r} redo`。

**修改的檔案：** `crates/spectrum-core/src/undo.rs`、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`

e2e 守住：`e2e_gtk_undo_redo_edit_ops`（`/tmp/spectrum-undo-fixtures`）——rotate→flip 後 undo 回到 rotate、redo 回到 flip、undo 兩次清空 ops 且 `history 0 undo, 2 redo`、新變更清空 redo 分支、以及**跨檔案**：編輯 b.png 後選取 c.png，undo 必須把選取移回 b.png（`selected == Some(1)`）。

快照注意：新增 UI 列會改變視窗高度，所有基線必須重新產生（900×1001 → 900×1041）。

核心測試 `crates/spectrum-core/tests/undo.rs` 7 個：record/undo 語義、redo 移回、新變更清 redo（先前未撤销的變更仍在 undo 堆）、LIFO 順序、no-op 不記錄、`apply()` 回傳新 ops 且保留 path、clear 清空兩堆。

目前 `./test.sh -- --test-threads=1` → **191 passed**（core 164、egui e2e 2、GTK e2e 25），完整套件連續 10 次、新測試單獨跑連續 12 次皆穩定。

## 2026-10-09 — 狀態列任務進度（StatusBarModel，macOS §4.11）

**類型：** Feature

**問題：** 掃描／複製／刪除沒有進度與完成訊息；macOS 版有 `StatusBarModel`（`isActive`、`progressDone/Total`、`doneMessage`、`globalLabel`、10 秒自動消失）。

**根因／做法：** 把 10 秒計時器建模成**明確的截止時間**（`message_expiry` / `global_expiry`）而不是真的 sleep：`finish(message, now)` 與 `finish_global(message, now)` 記錄 `now + ttl`，`tick(now)` 清除過期項。測試因此不需要等 10 秒，也不依賴牆鐘時間。

`begin()` 開始新任務時會取消前一個計時器並清除完成訊息（實作選擇：新任務取代完成訊息，否則訊息會永遠掛在那）。`update(done, label)` 對 determinate 任務把 `done` 夾到 `total`，避免顯示 `12/10`。`is_visible()` 語義同 macOS：`active || done_message || global_label`。`text()` 優先序：進行中任務 > 完成訊息 > 背景標籤。

UI 端 `Ui` 新增 `bar`、`progress`（sidebar 列）與 `clock`（可注入時鐘，e2e 用固定時間推進 TTL）。`refresh()` 每次 `tick(clock)` 後渲染該列。`open_folder`、刪除、貼上三個 handler 各自 `begin_total` → 逐項 `update` → `finish`。

**修改的檔案：** `crates/spectrum-core/src/status_bar.rs`（新）、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_status_bar_progress_and_expiry`）、`crates/spectrum-core/tests/status_bar.rs`。

**測試：** 核心 7 個（含夾總、TTL 到點清除、新任務取消計時器、優先序）；e2e 走固定時鐘：啟動時該列隱藏 → 開子資料夾顯示 `scanned 2` → 9 秒仍在、10 秒消失 → determinate 顯示 `pasting d.png 1/3` → 真實 handler 產生 `pasted 1`、`deleted 1` → 新任務取代完成訊息且被取消的計時器不會清掉新任務。基線 `status_bar.baseline.png` 900×1066（其他基線不受影響，因為該列隱藏時不佔空間）。

**注意：** `record_click(idx, ClickMods::default())` 會重置選取；`apply_view()` 在 paste 後已為第一個可見列記錄過一次點擊，所以此處若用 Ctrl 點擊會變成 2 個檔案。

## 2026-10-09 — Import 面板：日期分組與展開/縮合（macOS §9）

**類型：** Feature

**問題：** 沒有 import 面板的日期分組；macOS 版 `ImportPanelModel` 按 EXIF 日期分組、最新優先，且可全部展開/縮合或單群組雙擊切換。

**根因／做法：** `crates/spectrum-core/src/import.rs` 新增 `ImportModel`：`ImportItem { path, date_key }`、`ImportGroup { key, indices, expanded }`、`ImportRow::Header(group) | Item(item)`。`scan(dir)` 只收媒體檔（`kind_for`），群組按日期新到舊、群組內按檔名，所以面板可確定。`toggle(group)`、`set_all_expanded(v)`、`rows()`（依展開狀態產生列）、`group_label(i)`（`2023-11-14  ·  3`）、`remove_items(paths)`（移除已搬移項目並丟掉空群組，展開狀態按 key 保留）。

EXIF `DateTimeOriginal` 尚未解析，所以分組用檔案 mtime（與 `SortKey::DateModified` 同源）；這是與 macOS 版的已知差異。

UI：`Ui` 新增 `import_list`、`import_scroll`、`import_model`、`import_rows`（列索引 → 模型列）、`import_expand`、`import_collapse`。`rescan_import()` 保留各群組展開狀態（先抓 `Vec<(key, expanded)>` 再 scan），`render_import_rows()` 依 `rows()` 重建。`connect_row_activated`：Header → toggle + 重建；Item → 若該檔在目前視圖可見則選取它。`open_folder()` 會 `rescan_import()`。

**修改的檔案：** `crates/spectrum-core/src/import.rs`（新）、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-gtk/src/app.rs`、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_import_panel_date_groups_and_expand`）、`crates/spectrum-core/tests/import.rs`。

**測試：** 核心 6 個（UTC 日界、群組新到舊、群組內名 ascending、rows 依展開狀態、remove_items 丟空群組且展開狀態按 key 保留、非媒體檔跳過）。e2e：固定 mtime → 7 列（3 header + 4 item）→ 點 header 只縮合該群組 → collapse all 只剩 3 列 → expand all 回 7 列 → 點 item 列主清單選取跟著走 → rescan 保留展開狀態。

**注意：** 新增面板列會改變視窗高度，所有基線需重新產生：現為 900×1207（`status_bar.baseline.png` 900×1232）。基線剛產生後的第一次完整套件出現 1 次 snapshot 差異（`multi_select`），之後連續 22 次完整套件皆穩定。

## 2026-10-09 — Import 分組改用 EXIF DateTimeOriginal

**類型：** Feature

**問題：** Import 面板按 mtime 分組，但 macOS 版 `ImportPanelModel` 用 EXIF `DateTimeOriginal`；EXIF 日期尚未解析（上一輪記錄為已知差異）。

**根因／做法：** `crates/spectrum-core/src/jpeg.rs` 新增 `TAG_DATETIME_ORIGINAL = 0x9003`、`ExifData.date_time_original`，以及 `ExifData::date_key()`：只看前 10 字元、要求 `b[4]==':'`、`b[7]==':'` 且數字位正確，故 `"2020:01:02 03:04:05"` 與純日期 `"2020:01:02"` 可解析，`"2020-01-02 03:04:05"`（連字號）會被拒絕。`crates/spectrum-core/src/import.rs` 的 `scan()` 改為 `(path, key, mtime)`，群組 key 先取 `exif_date_key(path)`（僅 JPEG），沒有 EXIF 日期才退回 `date_key(mtime)`；排序為 `b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))`（新到舊、群組內名 ascending）。

`fixture.rs` 的 `ExifFixture` 新增 `date_time_original: Option<&'a str>`，`exif_block` 寫出 type 2 (ASCII) entry。兩個 fixture bug 一併修正：(1) `n0` 必須計入 Exif 子 IFD 的**指標** entry（`n0 = 1 + colorspace + make + model + usize::from(n1 > 0)`），否則只有日期的 EXIF 區塊讀不到日期；(2) `has_exif` 未包含 `date_time_original`，orientation=1 且只有日期時會產生僅 SOI+EOI 的檔案。

**修改的檔案：** `crates/spectrum-core/src/jpeg.rs`、`crates/spectrum-core/src/import.rs`、`crates/spectrum-core/src/fixture.rs`、`crates/spectrum-gtk/tests/e2e.rs`（`e2e_gtk_import_panel_uses_exif_date`）、`crates/spectrum-core/tests/exif_date.rs`（新）。

**測試：** 核心 5 個（讀取日期並轉 key、malformed/missing 清單、import 按 EXIF 日期分組且 EXIF 勝過 mtime、無 EXIF 日期退回 mtime、EXIF 與 mtime 同群組）。e2e：`photo.jpg` EXIF `2020:01:02` + mtime 2023-11-14、`plain.png` mtime 2021-06-05 → 列必須為 `2021-06-05 · 1`、`plain.png`、`2020-01-02 · 1`、`photo.jpg`。

**注意（snapshot 不穩定根因）：** `set_editable(false)` 不足以讓基線穩定。`GtkEntry` 被 focus 時會畫 focus ring 與 blinking caret：實測 40 次連續 snapshot 有 32 次與第一次不同（差異區 x=0..525, y=39..72，以及 19 px 的 caret 直列 x=9, y=46..64）。修法是在 `window.present()` **之前**把 entry 移出 focus chain（`freeze_search()` → `set_editable(false)` + `freeze_focus()` 遞迴 `set_property("focusable", false)`），之後 40 次 snapshot 差異為 0。基線需在此條件下重新產生（900×1207，`status_bar` 900×1232）；連續 12 次完整套件 + 12 次單測皆穩定。

## 2026-10-22 — 設計 tokens 管線（fig → tokens.rs）

**類型：** Feature

**問題：** UI 設計在 `../design/osx-photos-prototype.figcraft`（4 個 screen、750 layers），但沒有機制保證 app 用的顏色／尺寸／字級真的等於設計檔，也沒有機制防止設計檔改了而 app 沒跟上。

**根因／做法：** 仿 ArtCraft 的 `cargo xtask parity`／`xtask assets` 做法，把設計檔當成可驗證來源：

- `xtask/src/design.rs` 讀 `.figcraft`（zip → `document.json`），抽出全部**觀測值**到 `assets/design/observed.json`：screens、palette（含頻率）、type scale、corner radii、gap／padding 直方圖、元件框（Titlebar 52／Sidebar 232／Inspector 260／tile 164×164 gap 2）、prototype flow（一律 dissolve 220 ms easeOut），並記錄 fig 的 sha256。
- `assets/design/theme.json` 是**人工定義**的語意角色；`design::check_theme()` 要求每個角色的 hex 必須真的出現在觀測调色板，否則 CI fail（防止角色漂移）。
- `xtask/src/theme.rs` 由 `theme.json` 產生 `crates/spectrum-app/src/tokens.rs`（只有值：`Color32` 常數、radius/size/space/text 常數）。
- `cargo xtask design`／`cargo xtask theme` 預設是**驗證**（產生值與已 commit 檔不同就 fail），`--write` 才寫入；`cargo xtask ci` = fmt → clippy `-D warnings` → test → design → theme。

**修改的檔案：** `xtask/src/main.rs`、`xtask/src/design.rs`、`xtask/src/theme.rs`、`xtask/Cargo.toml`、`Cargo.toml`（新增 `serde`；workspace members 移掉 `crates/spectrum-gtk`）、`assets/design/theme.json`、`assets/design/observed.json`、`crates/spectrum-app/src/tokens.rs`、`docs/ui-spec.md`、`.cargo/config.toml`（`cargo xtask` alias）、`clippy.toml`。

**測試：** `xtask/src/theme.rs` 3 個（`#ffffff`／`#0b6fe0`／`#ffffff@60` → egui 建構式、無效 hex 回 None、整數保持一位小數、整份 theme 每個群組都生成）；`cargo xtask design` 驗證 25 個顏色角色全部存在於設計调色板。

**注意：** Rust toolchain 改裝在 repo 內（`.toolchain/rustup`、`.toolchain/cargo`，rustc 1.99.1），`gitignore` 排除；用 `.toolchain/cargo/bin` 加入 PATH 即可 build。

---

## 2026-10-22 — 測試設計改採 ArtCraft 理念，UI 改用 egui

**類型：** Refactor

**問題：** 原本同時有 `spectrum-app`（egui）與 `spectrum-gtk`（GTK4 + glycin）兩條 UI 路；GTK 路需要 11 個 apt 套件與 broadway 虛擬顯示器，且沒有 headless 渲染測試路徑。需要一套能進 CI 的 UI 測試模型。

**根因／做法：** 讀 `storytold/lightcraft`、`storytold/deckcraft`、`storytold/photocraft` 的 egui 實作後，把它們的理念寫成 `docs/testing.md`，並定出 Spectrum 的 L0–L9 分層與實作順序：先 **command registry** → **widget registry（可位址性）** → **`logic`/`ui` 分離** → **`skip!` testkit + ci report** → **AccessKit audit** → **registry 斷言** → **soak**，才接完整畫面。workspace members 移掉 `crates/spectrum-gtk`（目錄保留但不建置）。

**修改的檔案：** `docs/testing.md`（新）、`docs/ui-spec.md`（新）、`Cargo.toml`。

**注意：** `crates/spectrum-app/src/theme.rs` 與 `widgets.rs` 是在上述順序之前寫的草稿，尚未接進 app，會按測試規則重寫（補 registry 註冊與 AccessKit 名稱）。

## 2026-10-22 — Command registry（步驟 1）

**類型：** Feature

**問題：** UI 動作散在 widget 事件裡，測試與自動化無法用同一套入口驅動，也無法證明「人能做的，程式也能做」。

**根因／做法：** 仿 LightCraft 的 `CommandSpec`：

- `crates/spectrum-core/src/model.rs` 新增 `Library`（UI 無關的 app 模型：資料夾、entries、selection、view、tab、`hdr_enabled`、filter、year、sort、status、collapsed、`trash_root`）與 `compose(current, op)`；`compose` 與 `CompositeEdit::from` 的語意在 test 內互相驗證。
- `crates/spectrum-core/src/command.rs` 23 條命令（`library.rescan/select/clear`、`grid.click`、`view.set`、`month.open`、`year.open`、`group.toggle/collapseAll/expandAll`、`hdr.toggle`、`inspector.tab`、`filter.kind/hdr/clear`、`sort.set`、`edit.rotate/flipH/crop/restore`、`photo.trash`、`status.clear`），每條有 `id/label/group/shortcut/params/enabled/run`；`dispatch()` 先查 guard 再跑，拒絕時回 `CommandError::{Unknown, Disabled, BadParam, Failed}`，**不會改動 model**。參數讀取一律回錯誤（`want_object`、`f64_arg` 要求 finite、`usize` 走 `usize::try_from`）。
- 編輯寫 XMP sidecar，不碰原檔；`orientation_of()` 用 JPEG EXIF orientation（其他格式為 1），寫與讀用同一來源以保證反向合成一致。

**修改的檔案：** `crates/spectrum-core/src/model.rs`（新）、`crates/spectrum-core/src/command.rs`（新）、`crates/spectrum-core/src/lib.rs`、`crates/spectrum-core/Cargo.toml`（`serde_json`）、`crates/spectrum-core/tests/command.rs`（新，14 tests）。

**測試：** 14 個：registry id 唯一且 metadata 完整、`describe()` 全覆盖、未知 id 的錯誤訊息含 id、`view.set` 改 model 且壞參數不改 model、`hdr.toggle` 讓 HLG badge 變 SDR、選取語意（out-of-range 丟棄、ctrl toggle、shift 延伸、拒絕不改選取）、月份／年份 key 驗證與可见數、群組縮合互不影響、filter/sort 改變可见且拒絕保留前值、編輯需要恰好一張選取、**原檔 byte-identical**、crop 越界/零寬拒絕且不寫 sidecar、trash 移動後 rescan + 清選取 + status 文字、非 object 參數一律拒絕不 panic。

---

## 2026-10-22 — Fixture 移到 dev-only crate、清掉全部 clippy 錯誤

**類型：** Refactor

**問題：** `crates/spectrum-core/src/fixture.rs`（合成媒體素材）在產品 crate 裡，帶來 6 個 `unwrap`/`expect`；`cargo clippy --workspace --all-targets -- -D warnings` 因此無法通過，而 `-D warnings` 是 ci 的閘門。另有 11 個既有 clippy 錯誤散在 core（`unwrap`、`== None`、manual `rem_euclid`、`RangeInclusive::contains`、`&PathBuf`、redundant closure、`sort_by_key`）。

**根因／做法：** 依 ArtCraft 的 testkit 慣例（`lightcraft_testkit` 是 dev-dependency only、與產品碼分開），新增 `crates/spectrum-testkit`，把 `fixture.rs` 移過去（**不依賴 `spectrum-core`**，純位元組產生），crate root 顯式 `#![allow(clippy::unwrap_used, clippy::expect_used)]` 並在文件註明理由：fixture 寫檔失敗必須高調失敗，否則測試會對空目錄做斷言，那是錯誤結果而非明顯結果；產品碼仍守不 panic 規則。26 個測試檔的 import 改寫為 `spectrum_testkit::fixture`。

既有 clippy 錯誤逐個修掉而非放宽：`cover.rs`／`folder_cache.rs` 的 `unwrap` 改 `is_none_or`／`if let`；`import.rs` 改 `is_none()`；`crop.rs`／`orientation.rs` 改 `rem_euclid(360)`；`jpeg.rs` 改 `(0xD0..=0xD9).contains(&marker)` 並把 doc comment 後空行改一般註解；`scan.rs`／`sort.rs` 改 `and_then(parse_icc)`／`is_some_and(is_hdr_signal)`；`search.rs` 改 `sort_by_key(Reverse(mtime))`；`fixture.rs` 的 PNG scanline 改整列 `repeat`（消除 same-item push）並去掉多餘 `as_bytes()`。

**修改的檔案：** `crates/spectrum-testkit/{Cargo.toml,src/lib.rs,src/fixture.rs}`、`crates/spectrum-core/src/{cover.rs,folder_cache.rs,import.rs,jpeg.rs,scan.rs,sort.rs,search.rs,crop.rs,orientation.rs,model.rs,lib.rs}`、`Cargo.toml`（members + workspace deps）、`crates/{spectrum-core,spectrum-app}/Cargo.toml`（dev-dependency）、26 個 test 檔、`crates/spectrum-core/tests/{trash.rs,sort_order.rs,real_fixtures.rs,select.rs}`。

**注意：** 给 core 加 `serde_json` 之後，`tests/select.rs` 的 `assert_eq!(sel.set, vec![1])` 出現型別推斷歧義（`serde_json::Value: PartialEq<usize>`），已把期望值標成 `vec![1usize]`。`fixture.rs` 的 `next += date.len()+1` 是死賦值且用 `unwrap()`，一併移除。

**驗證：** `cargo xtask ci`（fmt → clippy `-D warnings` → test → design → theme）全綠；workspace **204 tests passed / 0 failed**（含 app 的 kittest e2e 與 snapshot）。Rustfmt 與 Clippy 元件已用 `rustup component add rustfmt clippy` 裝進 repo 內 toolchain。

## 2026-10-22 — 多資料夾、側欄改成 Folder view、工具列接手 view/filter/sort

**類型：** Feature

**問題：** 使用者要求側欄只留 Folder view，之後用 Add folder 加入資料夾；但 `Library` 只有一個 `folder`，沒有「加入／切換／移除資料夾」的模型與命令，而 view/filter/sort 原本住在側欄。

**根因／做法：**
- `spectrum-core::model`：`folder: PathBuf` 換成 `folders: Vec<FolderSlot>` + `active: usize`，`FolderSlot { path, count }`（`count` 在該資料夾被掃過才知道）；新增 `folder()`（空清單回空路徑，不 panic）、`add_folder()`（不是目錄回 `Failed`、重複回 `BadParam`，成功後自動開啟）、`open_folder()`、`remove_folder()`（只從清單移除，**不刪檔案**）、`rescan()` 順帶更新 active 的 `count`。
- `spectrum-core::command`：新增 `folder.add`（`Cmd+T`）、`folder.open`、`folder.remove`（guard `NEEDS_A_FOLDER`，`disabled_reason` 給「the library has no folders」而不是含糊的「nothing is selected」）。
- `spectrum-app`：側欄只剩 `Folders` 清單 + 路徑輸入框 + `Add folder` / `Remove folder`；被拒絕的路徑**留在輸入框**讓使用者改，原因顯示在狀態列。view/filter/sort 移到新的 `toolbar` 面板（高 42）。

**修改的檔案：** `crates/spectrum-core/src/{model.rs,command.rs,lib.rs,timeline.rs}`、`crates/spectrum-app/src/{app.rs,widgets.rs}`、`crates/spectrum-core/tests/{folders.rs(新),command.rs}`、`crates/spectrum-app/tests/e2e.rs`、`docs/ui-spec.md`。

**測試：** 新增 core 5 個（加入即開啟、切換重掃並丟棄越界選取、重複/不存在/缺參數一律拒絕且不改 model、移除後檔案仍在磁碟上、清單空了 `folder.remove` 回 `Disabled`）；app e2e 新增 4 個（側欄不再有 Library/Filter 字樣、輸入框加入資料夾、壞路徑被拒絕且文字留在框裡、移除資料夾後檔案仍在）、以及 `no_control_sits_outside_the_window`（每個 Button / TextInput 的 rect 中心必須在視窗內）。**220 tests passed / 0 failed**，`cargo xtask ci` 全綠。

**注意（踩到的兩個版面 bug，都轉成測試）：**
1. 52 px 的標題列塞兩列時，第二列的節點 rect 落在 y 57–83（工具列區域），點擊會打到工具列 → 標題列只留一列，右端用 `ui.with_layout(Layout::right_to_left(..))` 在**同一列**靠右。
2. `widgets::segmented` 原本用 `available_width()/n` 當 cell 寬 → track 吃掉整列寬度，把後面的控制項推出視窗右緣，節點存在但點不到（e2e 的 `Videos`、`Size` 點擊無效）。改成 cell 依文字寬 hug，並加 `no_control_sits_outside_the_window` 守住這條。

**副作用：** `ViewMode::arg()` / `InspectorTab::arg()` / `SortKey::parse|arg()` / `SortDirection::parse()` / `MediaKind::parse|arg()` 把字串↔列舉的對映集中到 core，command 與 UI 共用同一來源；`timeline::civil_from_days` 改 `pub`（UI 格式化日期不引入時間 crate）。egui 0.36 沒有 `SidePanel/TopBottomPanel`（統一為 `Panel`）、也沒有 `FontFamily::Strong`/`TextStyle::Strong`（粗體改由 `RichText::strong()` 負責）；平行測試共用同一個 fixture 目錄會互相改寫檔案，改成每個測試一個固定目錄名。

## 2026-10-22 — HEIF 有多個 ICC profile：抓第一個會漏掉 HDR

**類型：** Bug fix

**問題：** 拿使用者的一批手機拍攝檔（24 張 iPhone HEIC + 1 張 Sony HIF + 7 支影片，路徑只在命令列參數裡）跑 `cargo xtask scan`，結果回報 **0 HDR**、每張照片的 transfer 都是 `unspecified`，但這些檔案在 macOS Spectrum 裡是 HDR。

**根因／做法：**
- `IMG_5034.HEIC` 的 `ipco` 裡有 **4 個 `colr`**：前兩個是 Display P3／純描述 profile，第三個的 `cicp` 是 `p12 t8`（BT.2020 primaries + BT.709 transfer，SDR），第四個才是 `p12 t16`（BT.2020 10-bit + PQ）並帶 `hdgm`／`gmap` gain-map 中繼資料。`find_icc()` 只取第一個 → HDR 訊號被丟掉。
- `spectrum-core::heif`：新增 `find_icc_profiles()`，把 `colr(prof)` 與 `iccp` 全部收集（`find_icc()` 保留，文件說明改用新的）。
- `spectrum-core::icc`：`IccInfo` 新增 `gain_map: bool`（`hdgm` tag 的內容以 `gmap` 開頭）；新增 `pick_icc()`，排序為「`cicp` 指向 HDR transfer → `hdgm` → 任何 `cicp` → `dmnd >= 1000` → 有描述 → 任何可解析的 profile」。只挑「第一個有 `cicp` 的」仍會讀成 SDR，所以 HDR transfer 必須優先。
- `spectrum-core::hdr`：新增 `SignalSource::IccGainMap`（gain map 只說明「有 HDR 版本」，不指定 transfer，因此 transfer 維持 `Unspecified`，由 source 判定 HDR）；`is_hdr_signal()` 納入該 source；`describe()` 顯示 `gain map (ICC hdgm)`。
- `is_bt2020_primaries()` 改按 ITU-T H.273：9/10/12/13，**移除 11**（11 是 sRGB IEC 61966-2-4，不是 BT.2020）。ICC 的 `cicp` tag 用的就是同一組 H.273 code point。
- `spectrum-core::scan`：`detect_signal()`／`detect_icc()` 改走 `find_icc_profiles()` + `pick_icc()`。

**修改的檔案：** `crates/spectrum-core/src/{heif.rs,icc.rs,hdr.rs,scan.rs,lib.rs}`、`crates/spectrum-testkit/src/{fixture.rs,lib.rs}`、`crates/spectrum-core/tests/hdr_profiles.rs(新)`、`xtask/src/main.rs`。

**測試：** fixture 新增 `synthetic_heif_with_profiles()`（多個 `colr(prof)`，照 box 順序）與 `icc_gain_map()`；新增 5 個測試（SDR profile 在前的 HEIC 讀成 PQ／`IccCicp`、只有 `hdgm` 的 profile 仍算 HDR 且 transfer 為 unspecified、`pick_icc` 的優先序、SDR 檔案維持 SDR、H.273 primaries 代碼）。**226 tests passed / 0 failed**，`cargo clippy --workspace --all-targets -- -D warnings` 0 error，`cargo xtask ci` 全綠。同一資料夾的掃描結果從 **0 HDR → 24 HDR**，每張顯示 `PQ`、`cicp p12 t16 m0`、`gain-map(hdgm)`。

**注意（踩到的）：** fixture 的 `hdgm` tag 宣告 28 bytes 卻只寫 20 bytes，`parse_icc()` 讀 `off..off+size` 越界回 `None`，於是 `pick_icc()` 退回 SDR profile、測試斷言 source 變成 `Nclx`。tag 的 size 必須等於實際寫入的長度，這類 fixture bug 會偽裝成「偵測不到 HDR」。 Sony HIF 沒有 `iccp`／`colr`，維持 `sRGB`（來自 EXIF），是預期結果。

## 2026-10-22 — 把 FigCraft 的開發紀律寫進 AGENTS.md（測試為重點）

**類型：** Docs

**問題：** FigCraft／ArtCraft 的紀律只散在 `docs/testing.md` 與 session 記憶裡，repo 沒有 `AGENTS.md`；agent 進來時沒有「不可妥協」清單，測試規則也可能被繞過（裸 `return` 跳過、拿座標點 UI、宣稱未實作的測試等級）。

**根因／做法：**
- 新增 repo `AGENTS.md`：session 起點（ui-spec／testing／dependencies／devlog）、不可妥協（參考碼只讀不搬、不得 commit 個人素材、不會 crash、架構分層、一切經 command registry、UI 薄、只 Rust）、**測試紀律**（可位址性、只走公開通道、logic/ui 分離、AccessKit 名稱、settled-frame snapshot 與容差理由、跳過必可稽核、soak 不變量、每條新功能都要「拿掉就紅」、斷言帶 reason、fixture tag size 與平行目錄規則、誠實宣稱）、品質門檻、執行與看結果、文件紀律。
- 把紀律變成可強制：`spectrum-app/src/lib.rs` 與 `main.rs` 補上 `#![forbid(unsafe_code)]` 與 `#![deny(clippy::unwrap_used, expect_used, panic, unimplemented, todo, unreachable)]`（原本只有 `spectrum-core` 與 `xtask` 有）。
- FigCraft 的「測試即行為檔」模式（parity 表每列引用 `*_tests` 模組名）對應到我們的 id＋devlog；FigCraft 用 chrono，我們把 calendar 運算集中在 `spectrum-core::timeline`，AGENTS.md 記的是「不要各寫一份」。

**修改的檔案：** `AGENTS.md(新)`、`crates/spectrum-app/src/{lib.rs,main.rs}`、`docs/testing.md`。

**測試：** `cargo clippy --workspace --all-targets -- -D warnings` 0 error（補上 deny 後 app 碼仍然乾淨，因為 `unwrap` 只出現在測試）、**226 tests passed / 0 failed**、`cargo xtask ci` 全綠。
## 2026-10-23 — 快照重建、字型回退、網格改寫
`UPDATE_SNAPSHOTS=1` 重建 `library_all.png`、`detail_adjust.png`（1440×900）。UI 規格刪掉不存在的 `panel:inspector` / `inspector:scroll`（registry 測試抓出來：規格寫的 id 跟實際註冊的不一樣）。theme 加入 `main_inline=20`、`main_padding=16`（設計 Main frame 的 padding 是 `[16, 20, 16, 20]`），重產 `tokens.rs`。
網格原本用 `Grid`：它按看過的儲存格算欄寬，32 個檔案時後面幾欄收縮到 HDR badge 的寬度，tile 彼此疊了 ~122px。改成依實際寬度算固定欄數（`horizontal_wrapped` 在垂直 ScrollArea 裡拿到無限寬，會跑成幾千 px）。badge 從 tile 下方移到 tile 內左上角，設計的行距 166px 才成立（放下面會變 185px）。CJK 字型走系統載入（本機 NotoSansCJK，或 arphic 的 uming.ttc），`SPECTRUM_CJK_FONT` 可覆蓋；啟動 log 會印載入的路徑。
教訓：`Grid` 的欄寬由先出現的儲存格決定，內容寬度不一致時不能用它；`Ui::put` 會推進父 ui 的游標，疊放的元件要放進獨立的 sub-ui，否則下一個 tile 從 pill 的右邊開始（實測 x 從 432 跳到 306）。
驗證：`cargo test --workspace` 240 通過、`cargo xtask ci` 全綠、`cargo clippy --workspace --all-targets -- -D warnings` 0 error。

## 2026-10-23 — 縮圖真的進到格子
`ThumbnailService::get` 改回傳 `Decoded { width, height, rgba }`（自己實作 box-average `downscale`，不再依賴 glycin 的 `set_scale`——它對 8064×6048 的 HEIC 無效，每檔 ~850ms、~146MB RGBA）。App 端 `thumbs::Worker`：一條解碼執行緒、LRU 上限 64 張，每帧最多上傳 3 張、每帧只補 3 個新請求（`thumb_asked` / `thumb_failed` 保證不會每帧重排同一檔）。tile 有 texture 就畫圖（`alt_text` 帶檔名，AccessKit 找得到）、沒有就保留 placeholder 色與檔名；解不了的（`.MOV`、`video/quicktime`）永久走 placeholder。
`crop_center_square` 在 core 裡做中心方型裁剪：egui 0.36 不公開 `UvRect`，所以裁剪在像素層面做，4:3 照片被裁掉兩邊而不是壓扁。測試用合成的 PNG 驗證「紅圖在 red tile、藍圖在 blue tile、影片仍是 placeholder」，以及沒掛 decoder 時快照仍是 placeholder（快照不依賴解碼時間）。
順手修了 `testkit::synthetic_png`：stored deflate block 的長度欄位寫成 `u16`，超過 65535 bytes 會被無聲截斷，glycin 回報 `IDAT or fDAT chunk does not have enough data for image`；改成切多個 block。200×120 以上的 PNG 之前其實是壞檔。
驗證：新增 `crates/spectrum-app/tests/thumbnails.rs`（3 個測試），`cargo test --workspace` 244 通過、`xtask ci` 全綠。

## 2026-10-23 — 控制通道要主動要求重繪，以及這個環境的截圖限制
`ui.screenshot` 靠 `ViewportCommand::Screenshot`，該命令要由 eframe 在**繪製 pass** 時執行；`--control` 開著但視窗沒在畫時，`rx.recv()` 會無限等（實測卡在 `ui.screenshot`）。改成 `request_nudged`：每 120ms `request_repaint()`、150ms 輪詢、5s 回報「視窗可能被遮蔽或最小化」，`main.rs` 在控制通道開啟時加 300ms 心跳重繪。
本機 Xwayland（mutter rootless）會把我們的視窗標為不可見（winit `is_visible()` false，X11 `GetImage` 回 `Match`），eframe 因此只跑 logic 不跑 pass：`ui.inspect` 答得出來、`ui.screenshot` / `ui.click` 答不出來。所以自動化改用 `egui_kittest` 的離頭渲染（`harness.render()`），live 視窗只在啟動後幾秒可截圖。
驗證：kittest 的 `harness.render()` 渲染出真實縮圖（12 張合成圖 + 1 檔影片，格子裡 49 種顏色、placeholder 不再是主色）；`cargo test --workspace` 244 通過。
教訓：eframe 對不可見視窗不會產生 pass，任何依賴繪製的命令（截圖、event 觸發的點擊）都要主動 `request_repaint` 並在逾時時說清楚原因，而不是讓測試卡死。

## 2026-10-23 — 缺口清單成一份可選的 backlog
把 macOS 參考版（11,899 行 Swift）跟 Rust 版（core 4,550 + app 2,477）逐模組對過，寫成 `docs/backlog.md`：**F01–F19 功能缺口**、**Q01–Q07 測試與紀律缺口**，每項附「MacOS 參考規模／現況證據／要動什麼」。證據用的是 `grep -c crate::module`（例如 `nav`/`search`/`import`/`clipboard`/`undo`/`folder_cache` 在 core 與 app 都是 0 處使用 — 實作在、沒人接）與 `command.rs` 的命令清單（沒有 `edit.undo`/`edit.redo`/`edit.flip`/`clip.*`）。
同步修了三處文件與事實不符的地方：`docs/testing.md` 的分層表加「狀態」欄（L3/L6/L7/L8 明確標未做）、第四節實作順序改成實際進度（`theme.rs`/`widgets.rs` 早已接進完整畫面）、`cargo xtask ci` 目前只做 fmt/token/snapshot 三步而非 L0–L8；`AGENTS.md` 的 registry 規則例子改掉不存在的 `panel:inspector`（它只在 `ids-on-selection`），並把「無指定任務時的起點」從 testing.md 改成 `docs/backlog.md`。另外把文件裡混進的簡體字統一回繁體。
驗證：`cargo fmt --all --check`、`cargo xtask ci` 全綠、`cargo test --workspace` 全部 suite 0 failed。

## 2026-10-23 — F01：資料夾清單持久化
**問題：** `Library.folders` 只在記憶體，重開程式後側欄全空，要重新 Add folder。
**根因與做法：** 新增 `crates/spectrum-core/src/library_store.rs`：`{version, active, folders:[{path,count}]}` 寫在 `<config>/spectrum/library.json`（`$SPECTRUM_LIBRARY_STORE` > `$XDG_CONFIG_HOME` > `$HOME/.config`）；`load` 逐檔檢查 `is_dir()`，不見的放進 `dropped`、重複與空路徑折疊、`active` 夾到存活清單；`save` 先寫 `.tmp` 再 rename，避免寫到一半崩掉把檔案寫壞。版本不符或 JSON 壞掉時**回錯誤**，不当成空清單（否则使用者的資料夾會被無聲清掉）。App 端 `AppState::{set_store, restore_store, save_store}`，`run()` 在 `folder.*` 成功後立刻寫檔，寫檔失敗把錯誤進狀態列；`restore_store` 對「不見的資料夾」產生 `Skipped N folder no longer on disk: …` 訊息。命令列给的資料夾永遠會被 add + open（已在清單裡就只 open）。
**修改的檔案：** `crates/spectrum-core/src/{library_store.rs,lib.rs}`、`crates/spectrum-app/src/{app.rs,main.rs}`、`docs/backlog.md`。
**測試：** `crates/spectrum-core/tests/library_store.rs` 6 條（round-trip、missing drop+active clamp、duplicate/blank collapse、malformed+錯版本回錯誤且檔案不被改、apply 會 rescan 所以 99 這種舊 count 不被相信、save 自建父目錄）；`crates/spectrum-app/tests/store.rs` 6 條（add 立即寫檔、重啟讀回並 rescan 出真實 count、消失的資料夾被報出來、壞檔保留命令列資料夾、remove  rewrite 且不刪檔案、沒設 store path 就不寫檔）。`cargo test --workspace` 256 通過、clippy `-D warnings` 0、`xtask ci` 綠。
**注意：** 測試自己寫 raw JSON 時要先建 `<dir>/spectrum/` 目錄（`save` 會自建，手寫不會）；第一版就踩了這個（3 條 `NotFound`）。

## 2026-10-23 — F02：鍵盤導覽，以及兩個輸入框的 bug
**問題：** `nav.rs` 有 `next_index`/`row_col` 但 app 完全沒用：沒有方向鍵、沒有快捷鍵；`select.rs` 只有滑點。
**根因與做法：** 命令層加 `select.move`（`{"direction": ..., "columns": N}`，走 `nav::next_index`）與 `select.clear`；語意：**只在可見清單上走**（篩選、摺疊的月份自然跳過），越界回 `{"moved": false}` 且不改選取（不是錯誤），冷啟動時 `End` 落最後一個、其他鍵落第一個。UI 在 `tiles()` 算完實際欄數之後、借用 model 繪圖之前處理按鍵，所以同一幀就顯示新選取；選中的磁磚用 `scroll_to_rect_animation(..., ScrollAnimation::none())` 捲進可視區，並用 `scroll_hold = 2` 連續要求兩幀。`ScrollArea` 加 `id_salt("grid")`。
**順手修掉兩個輸入框的實錯：**
1. `typed.clicked()` 也會觸發 `folder.add` — 想輸入路徑而點一下輸入框，就**靜默改了資料夾清單**。改成只有 `Add folder` 按鈕或 Enter 提交。
2. egui `TextEdit` 預設 `return_key = Enter` 會**吃掉** Enter，所以 app 自己的 `key_pressed(Enter)` 永遠 false，Enter 从来不提交。改成 `.return_key(None)`，Enter 由 app 判且要求該欄有焦點（新增 `path_focused`，同時用來讓方向鍵在輸入時不搬選取）。
**修改的檔案：** `crates/spectrum-core/src/{command.rs,nav.rs,lib.rs}`、`crates/spectrum-app/src/app.rs`、`docs/ui-spec.md`、`docs/backlog.md`。
**測試：** `tests/select_move.rs` 9 條（欄序、Home/End、越界不改 model、摺疊群跳過並回 `Disabled`、篩選生效、單欄、clear、未知 direction/缺 columns 為 `BadParam`、不動檔案也不寫 sidecar）；`tests/keyboard.rs` 6 條（←→ 走一個、↓ 走一整列且用實測欄數、Home/End/Esc、越界不動、鍵盤選取的磁磚落在 `grid:scroll` 內、輸入框搶走方向鍵且點擊不會加資料夾、Enter 提交且不會加兩次）。`cargo test --workspace` 271 通過、clippy 0、`xtask ci` 綠（snapshot 未變）。
**注意：** 第一次選取會同時出現 inspector → 網格變窄 → 該幀的 scroll offset 被重置，所以「捲進可視區」要連續要求兩幀；單幀測試會一直失敗（實測：`tile y=1039` 對比 `scroll 123..876`）。另外 `egui_kittest` 的離頭渲染不會推進時間，任何依賴 `ScrollAnimation` 的動畫都不會有進展 — 用 `ScrollAnimation::none()`。

## 2026-10-23 — F06：單圖詳細檢視（zoom / pan / 1:1）
**問題：** 選取一張之後只有 260 px 的 inspector，沒有任何單圖畫面 — 看不到全解析度、不能縮放、也沒有 F07 tone-map 可以落的位子。原型的 Screen 2 是「檢視器 + inspector」，但它的照片是一張固定的 780×540 圖，沒有縮放控件，所以設計只給語彙、不給幾何。
**根因與做法：**
- 新增 `crates/spectrum-app/src/detail.rs`：控制列（`−` `1:1` `Fit` `+`、`NN%` 讀數、`Close`，沿用 26 px / radius 6）+ 畫布（拖曳平移、`Ctrl/⌘ + 滾輪`縮放、一般滾輪平移）。詳細檢視**取代**同一欄的 toolbar/grid，不是疊在上面。
- zoom 的定義是「顯示像素 / 原始像素」，`1.0` 就是 1:1、讀數就是它 × 100、`Fit` 就是 `None`（沒有明確值）。zoom 存 model（經 `detail.zoom` 命令，夾在 0.02–16），pan 存 UI — 只有畫布在乎圖片在哪。命令的 `in`/`out` 在 zoom 為 `None` 時用 UI 傳的 `current`（實際 fit 比例），否则從 Fit 放大會跳到 1:1 的倍率而不是画面的倍率。
- `Library.detail` + `detail_index()`：`detail_index()` 只在 **sole 選取**時回 `Some`，所以多選時畫面自動回 grid，不需要額外命令；`rescan()` 發現那張圖不見了就關掉；`detail.close` 連 zoom 一起清掉，下一張從 Fit 開始。重新置中用 UI 端的 `zoom_seen` 比對，不用 model 的一次性旗標（第一版用 `detail_recentre` 旗標，等於讓 UI 往 model 塞垃圾，刪掉了）。
- `thumbs::Worker` 改成帶 `Request { path, max, capacity }`，每種尺寸有自己的 `ThumbnailService`，所以 grid 的 164 px 快取不會把詳細檢視的全圖擠掉；詳細檢視要長邊 2400、最多同持 4 張。
**修改的檔案：** `crates/spectrum-app/src/{detail.rs,app.rs,thumbs.rs,lib.rs}`、`crates/spectrum-core/src/{model.rs,command.rs}`、`crates/spectrum-app/tests/{detail.rs,registry.rs}`、`crates/spectrum-core/tests/detail.rs`、`docs/{ui-spec.md,backlog.md}`。
**測試：** `spectrum-core/tests/detail.rs` 8 條（open 需 sole、多選自動不顯示、close 清 zoom、zoom 以 `current` 繼續、上下限可 escape、未知 action 為 `BadParam`、rescan 掉檔案就關掉、五個命令都不動檔案也不寫 sidecar）；`spectrum-app/src/detail.rs` 7 條單元（fit 取兩軸較小者、明確 zoom 被夾、比畫布小的圖不能平移、平移上限 = 超出的一半且畫布不會露出背景、bounds 與命令一致）；`tests/detail.rs` 7 條（Enter 開/Esc 關且 toolbar 消失、`Ctrl+ +/−` 縮放且 Esc 順序、Close 按鈕回 grid、`+/1:1/Fit` 改變實際繪製尺寸（1:1 = 圖自己的 1600 px）、拖 70 px 圖片移 70 px、拖過頭停在邊緣、影片開畫布但 `image_rect == Rect::NOTHING` 且控件仍可點）；`tests/registry.rs` 加 1 條（詳細檢視開著時 `ids-always` 還在、`ids` 的 grid id 必須全部消失、關掉後回來）。`cargo test --workspace` 292 通過、clippy `-D warnings` 0、`xtask ci` 綠（snapshot 未變）。
**注意：**
1. `ui.allocate_rect(canvas, Sense::drag())` 要在圖片**之後**分配：egui 把游標交給最後一個 rect 含住游標的元件，順序反了 `dragged()` 永遠 false。
2. `count_and_consume_key(Modifiers::COMMAND, key)` 不認得只有 `ctrl` 的事件（`cmd_ctrl_matches` 要求 pattern 的 `command` 位也設置），而 `i.modifiers` 只由 `ModifiersChanged` 更新，注入的 `Event::Key` 不會改它 — 所以 `zoom_key()` 同時試 `CTRL` 與 `COMMAND`。
3. 拖曳距離是依「上一幀記住的按下位置」算：press/move/release 塞同一批事件只會得到點擊，而且前一次點擊的位置會被當成拖曳起點（實測 70 px 量成 +284 px）。測試先輸一幀純 `PointerMoved` 再分幀 press/move/release。
4. `ui.put(rect, Image…)` 不推進 cursor，但會注册元件 rect；要讓畫布拿到游標，就得讓它最後注册。
