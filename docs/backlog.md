# Backlog（待開發清單）

對照 `~/git/spectrum-macos-ref`（Swift 11,899 行）與目前 Rust 版（`spectrum-core` 4,550 行、
`spectrum-app` 2,477 行）逐模組比過的結果。每項都寫了「MacOS 參考規模／目前 Rust 狀態／要動什麼」，
編號給使用者選。**狀態是 2026-10-23 的實際檢查**，不是推測。

用法：跟使用者說編號即可（例：`做 F01、F02`）。做完的項目由實作的人把 `[ ]` 改成 `[x]` 並在
`docs/devlog.md` 寫一筆。

---

## 功能缺口（F）

### F01 — 資料夾清單持久化
- [x] 完成（2026-10-23）：`crates/spectrum-core/src/library_store.rs` + `AppState::{set_store, restore_store, save_store}`；`folder.add`/`folder.open`/`folder.remove` 成功後立即寫檔，啟動時載入並 rescan。測試：`crates/spectrum-core/tests/library_store.rs`(6)、`crates/spectrum-app/tests/store.rs`(6)。
- macOS：`Services/BookmarkService.swift`(98) 用 security-scoped bookmark 記住資料夾。
- 現況：`Library.folders`（`model.rs`）只在記憶體，`model.rs` 裡沒有任何 save/load → **重開程式資料夾清單就空了**，要重新 Add folder。
- 要動：把 `folders` 寫成設定檔（例 `~/.config/spectrum/library.json`），啟動時載入並逐檔驗證路徑還在；加 `folder.save` 不需要（`folder.add`/`folder.remove` 觸發寫入即可）。
- 附带：啟動時自動 `rescan`（目前要手按 Rescan）。

### F02 — 鍵盤導覽與快捷鍵
- [x] 完成（2026-10-23）：命令 `select.move`（direction + columns）與 `select.clear`；UI 在 `tiles()` 裡以實際欄數處理 ← → ↑ ↓ Home End Esc，並把選中的磁磚捲進可視區（`scroll_hold` 2 幀）。測試：`crates/spectrum-core/tests/select_move.rs`(9)、`crates/spectrum-app/tests/keyboard.rs`(6)。快捷鍵表見 `docs/ui-spec.md`「鍵盤操作」。
- macOS：方向鍵/Return/⌘ 快捷鍵選取與開圖。
- 現況：`nav.rs`(43) 有 `next_index`/`row_col`，但 `app.rs` 完全沒用（`core_use=0`、`app_use=0`）。UI 只處理 Enter（add-folder 輸入框）與點擊的 ctrl/shift。**沒有方向鍵選取、沒有 ⌘/Ctrl 快捷鍵、沒有 PageUp/PageDown。**
- 要動：把 `nav::next_index` 接進 `grid:*` 的焦點邏輯；加 `select.next`/`select.prev` 命令（走 dispatch，不直接改 model）；快捷鍵表寫進 `docs/ui-spec.md`。

### F03 — Undo / Redo 接進命令層與 UI
- [ ] 未完成
- macOS：編輯可復原。
- 現況：`undo.rs`(98) 有 `undo()`/`redo()`/`can_undo()`/`can_redo()`，**但 `command.rs` 沒有 `edit.undo`/`edit.redo`**，UI 沒有復原按鈕。
- 要動：兩條命令 + guard（無可復原時回 `Disabled` 帶原因）+ inspector 的 `Undo`/`Redo` 按鈕 + registry id；拒絕時不改 model（跟現行規則一致）。

### F04 — Flip 命令
- [ ] 未完成
- macOS：翻轉開圖就有。
- 現況：`EditOp::FlipH` 與 `CompositeEdit.flip_h` 都實作且被 `from(ops)` 處理，但 `command.rs` 只有 rotate/crop/restore → **沒有 `edit.flip`**，UI 也沒有翻轉按鈕。
- 要動：`edit.flip` 命令 + inspector 按鈕 + 測試（sidecar round-trip 要驗 flip  survives）。

### F05 — 編輯要真的改變畫面
- [ ] 未完成
- macOS：改完立刻看到。
- 現況：`model.rs:405` 寫 XMP sidecar、`read_sidecar` 載回編輯，但 **縮圖完全沒套用 rotate/flip/crop**（`thumb.rs` 只 downscale + 中心方型裁剪）。所以「改了卻看不到」。
- 要動：`thumbnail.rs` 加 `apply_edit(&Decoded, &CompositeEdit)`（rotate 90/-90、flip_h、crop 依旋轉後座標系），在 downscale 之後套用；測試用合成 PNG 斷言像素位置。

### F06 — 單圖詳細檢視（zoom / pan / 1:1）
- [x] 完成（核心 + UI + 測試）
- macOS：`Views/Detail/PhotoDetailView.swift`(1138) + `PhotoInfoPanel.swift`(591)。
- 現況：`crates/spectrum-app/src/detail.rs`（250 行）— 控制列 `−` / `1:1` / `Fit` / `+` / `NN%` / `Close`，畫布拖曳平移、`Ctrl/⌘ + 滾輪`縮放；model 加 `detail` / `detail_zoom`，命令 `detail.open`（需 sole 選取）/ `detail.close` / `detail.zoom`；全圖由 thumbs worker 以長邊 2400 解（與 grid 的 164 分開快取、最多持 4 張）；registry `canvas:detail`、`panel:detail-strip` 與 6 個按鈕 id，`ids-on-detail` / `ids-always` 由 `tests/registry.rs` 驗（開著時 grid 的 id 必須不存在）。
- 還欠：上一張 / 下一張導航（macOS 有 34×34 圓形按鈕）、全螢幕、F16 的影片 preview。（F07 tone-map 已落在畫布與格子：見 F07。）

### F07 — HDR tone-mapping 與顯示模式（HLG / PQ / gain-map）
- [x] 完成（CPU 參考實作 + 接到縮圖與單圖 + 測試）
- macOS：`AVFMetalView`(1192) + `MetalShaders`(247) + `HDRImageViews`(87)。
- 做了：`crates/spectrum-core/src/tone_map.rs`(334) — PQ/HLG/sRGB EOTF 與 OETF、BT.709↔BT.2020 矩陣、extended Reinhard 壓縮、ISO 21496-1 `gain_map_blend`；`png.rs` 讀 `cICP`。縮圖執行緒在解碼後套 `apply_scene_transform`，texture 快取以 `(path, transform)` 為 key，切換顯示模式不重新解碼。
- 授權條件：只有 `nclx`（HEIF）與 `cICP`（PNG）會觸發轉換。ICC profile 推斷的訊號不會——量測顯示 glycin 對那些檔案已交回顯示就緒的畫面（見 devlog 2026-08-02）。
- 測試：15 個數學 test 用公開參考值（PQ 100 nit→0.5080762、1000 nit→0.7515776、HLG 峰值 1/12→0.5、矩陣相乘＝單位矩陣、knee 連續 1.2e-6）；2 個 UI test 用像素（`crates/spectrum-app/tests/hdr.rs`：HLG fixture 格子 `[217,130,36]` ↔ `[188,120,60]`，sRGB fixture 兩種模式都不變）。
- 還欠：gain-map 展開沒有呼叫端（loader 只交回 base image）；無 mastering metadata 時 PQ 峰值假設 1000 nit；GPU（wgpu）輸出與 headroom 呈現；視訊 HDR 屬 F16。

### F08 — 互動裁剪 UI
- [ ] 未完成
- macOS：`CropOverlayView.swift`(302)。
- 現況：`crop.rs`(58) 有 `CropRect`（含 rotated/flipped 換算）、`edit.crop` 命令存在，但 **UI 沒有任何裁剪控制**；`app.rs:540` 註解自己留了「if a second input is added (crop fields, search), gate this on the field's focus」。
- 要動：裁剪模式（拖框、8 個把手、比例預設、套用/取消），全部走 dispatch；registry id `crop:*`。

### F09 — 匯入面板
- [ ] 未完成
- macOS：`Views/Import/ImportPanelView.swift`(463)。
- 現況：`import.rs`(186) 有 `ImportModel::scan/toggle/rows`（按天分組），但 `core_use=0`、`app_use=0` → **完全沒接**。且日期鍵用檔案 mtime，因為 **EXIF `DateTimeOriginal` 還沒解析**（`import.rs:20` 註解講了）。
- 要動：先補 EXIF `DateTimeOriginal`（`jpeg.rs`/`heif.rs` 的 tag 解析），再做匯入 UI（來源→目標、按天展開、衝突改名規則沿用 `clipboard.rs` 的「 1」計數）。

### F10 — 搜尋
- [ ] 未完成
- macOS：`Views/SearchResultsView.swift`(166)。
- 現況：`search.rs`(100) 有 `normalize_query`/`search`，沒接 UI，也沒有 `search.set` 命令。
- 要動：titlebar/toolbar 的搜尋欄（`field:search`）+ 命令 + 結果過濾走 `filter`。

### F11 — 剪貼簿：複製／搬移到資料夾
- [ ] 未完成
- macOS：`Services/FolderClipboard.swift`。
- 現況：`clipboard.rs`(90) 有 `set_clipboard`/`paste`/`rename_dir`（含 collision 計數），**命令層沒有 `clip.copy`/`clip.cut`/`clip.paste`**，UI 沒有入口。
- 要動：三條命令 + 目標選單（從 library 的資料夾裡選）+ 測試（cut 後來源消失、paste 可重複）。

### F12 — 資料夾監看（自動 rescan）
- [ ] 未完成
- macOS：`Services/FolderMonitor.swift`。
- 現況：只有手動 `library.rescan`；新增檔案不會自己進清單。
- 要動：Linux 用 inotify（純 std 沒有 → 需要 `inotify` crate 或自己 syscall 包裝；後者會碰 `unsafe`，與 `#![forbid(unsafe_code)]` 衝突 → **需要你決定**：允許 `inotify` crate、用輪詢 + mtime 比對、或維持手動）。

### F13 — 設定頁
- [ ] 未完成
- macOS：`Views/SettingsView.swift`(380)。
- 現況：無任何設定 UI；字體候選、縮圖尺寸/LRU 上限、trash 根目錄都寫死或走環境變數。
- 要動：`settings` 畫面 + 持久化（跟 F01 同一個設定檔）+ 命令 `settings.set`。

### F14 — 多選 marquee、subfolder tile
- [ ] 未完成
- macOS：`PhotoGridView.swift`(1121) 有 marquee 拖選與次資料夾 tile。
- 現況：`select.rs`(57) 有 click/ctrl/shift 語意；**沒有框選、沒有次資料夾 tile**（目前只掃單層資料夾）。
- 要動：拖曳框選（走 dispatch `select.rect`）；subfolder tile + `folder_cache.rs`(90，寫好但沒人用) 用於次資料夾清單快取。

### F15 — 字串檔與中文化（i18n）
- [ ] 未完成
- macOS：`Localizable.xcstrings`。
- 現況：UI 字串寫死英文（`All Photos`、`Add folder`…），只有 CJK 字型 fallback 讓中文檔名不變 tofu。
- 要動：抽字串表 + `zh-TW` 資源；snapshot 測試要固定 locale，否則基線會漂。

### F16 — 影片播放
- [ ] 未完成 — **需要使用者決策**
- macOS：`VideoController`(355) + `VideoPlayerNSView`(136) + `VideoControlBar`(110) + `LivePhotoPlayerView`(93)。
- 現況：`mp4.rs`(157) 只解析 colr/transfer 等元資料；不能播放，tile 也沒有 duration。
- 決策點：播放必然引入非純 Rust 依賴（gstreamer 或 ffmpeg）。選項 (a) 允許 gstreamer 外掛、(b) 只顯示首幀 + 元資料、(c) 整塊延後。選完才開工。

### F17 — 陀螺儀穩定（360° 內容）
- [ ] 未完成
- macOS：`GyroCore.swift`(456) + `GyroConfig.swift`。
- 現況：完全沒有。Linux 端要讀 IMU/姿勢來源（`linuxptp`? 沒有標準 API）→ 需要可行性研究，不是直接開工的項目。

### F18 — HLG 匯出
- [ ] 未完成
- macOS：`Services/HLGExportService.swift`。
- 現況：只有 XMP sidecar 寫回編輯，沒有匯出。依賴 F07（先有 tone-map 才有匯出可談）。

### F20 — 「Add Folder」流程（瀏覽器、拖曳、Enter/Esc）
- [x] 完成（2026-08-03）
- macOS：`SidebarView.addFolder()` 用 `NSOpenPanel`（`canChooseDirectories = true`、`allowsMultipleSelection`、prompt "Scan"）＋側欄拖曳 ＋ `--add-folder` CLI。
- 做了：`spectrum-core/src/folder_browser.rs`（只列目錄、跳過 `.` 開頭、不分大小寫排序、每目錄帶自己那層的媒體檔數、不遞迴）；`spectrum-app/src/browser.rs`（懸浮視窗：目前路徑、26 px 目錄列、`Up`/`Cancel`/`Scan`、空目錄與讀不到目錄都有文字說明）；`app.rs` 加 `button:browse-folder`、拖曳資料夾走 `RawInput::dropped_files`、`Esc`/`Enter` 的優先次序。
- 一切變動仍走 `folder.add`：重複會被擋、原因進 `notice_text()`、**視窗保持開著**；成功才關視窗並清掉輸入框；F01 的持久化照舊（加入即寫設定檔）。
- 測試：`crates/spectrum-core/tests/folder_browser.rs`(5)、`crates/spectrum-app/tests/add_folder.rs`(10：開／走／Scan／Cancel／重複／讀不到／空目錄／Esc 不清選取／Enter 確認／拖曳目錄與拖曳檔案／registry id 只在開著時存在）。`docs/ui-spec.md` 新增 `ids-on-browser` 區塊與〈Add Folder 瀏覽器〉幾何表。
- 設計偏離：原型側欄只有 Add folder / Remove folder 兩顆按鈕，`Browse…` 是新增（記錄在 ui-spec）。
- 還欠：一次加**多個**目錄（macOS 的 panel 可多選）；目前一次一個。拖曳到「側欄」而非整窗（macOS 是側欄）。
- [ ] 未完成
- macOS：`NetworkVolumeService.swift`(119)。
- 現況：無。Linux 走 `/mnt`、`gvfs`、SMB 掛載點；主要工作是「掛載點慢/斷線時不要卡住掃描」→ 需要 timeout 與可中斷掃描。

---

## 測試與紀律缺口（Q）

### Q01 — L3 AccessKit audit
- [x] 完成
- 做了：`crates/spectrum-app/tests/accesskit.rs`（6 test）。走整棵 AccessKit 樹，把 **widget registry 承諾的 id** 跟樹裡節點對起來：
  - `button:*` / `segment:*:label` / `folder:*`（18 個）必須各自對應一個**同邊界**的 `Button` 節點，且 `label` 非空；兩邊數量必須相等，所以日後把控件改成純繪製會被這條擋下。
  - `field:add-path` 是 `TextInput`，accessible name 來自 hint（`placeholder="Folder path"`）。
  - `group:<key>` 是 clickable `Label`（不是 Button），節點落在 registry rect 內且文字含年份。
  - 格子（tile）的檔名、 metadata 行、`SDR`/`HDR` badge 都以 `Label` 出現在樹裡 → 螢幕閱讀器讀得到。
  - AccessKit 的 press 會走 `dispatch`：對 `"HDR / SDR toggle"` 節點 `click_accesskit()` 之後 `lib.hdr_enabled` 真的翻了。
  - 選取/詳細視圖切換時樹會跟著變（無選取時沒有 `Restore`；開 detail 後 toolbar 的 `Clear filter` 消失、`1:1` 出現）。
- 還欠（新增 Q08）：tile 是繪製的，樹裡沒有可操作節點 — 可讀但不可點，靠鍵盤導覽選取（F02）補。

### Q08 — tile 對 assistive tech 不可操作
- [ ] 未開始
- 由 Q01 的實測發現：格子是 `widgets::tile` 繪製的，AccessKit 樹裡只有檔名／metadata／badge 的 `Label`，沒有可 press 的節點。控制通道與滑鼠能選，螢幕閱讀器不能。
- 要動：把 tile 改成 `ui.put(rect, Sense::click())` 之類會產生節點的控件（或給 tile 一個透明 `Button` 覆蓋層），然後在 `tests/accesskit.rs` 把 tile 加進 button 集合。

### Q02 — L5 廉價像素斷言
- [x] 完成
- 做了：`crates/spectrum-app/tests/pixels.rs`（4 test）。顏色一律從 `theme::Tokens::photos()` 取（token 由設計檔產生），所以斷言跟著設計走，不是抄 hex；容差沿用 snapshot 的 2 LSB。
- 量測到的數字（1440×900）：grid 螢幕 tile 色 366 899 px、window 白 595 609 px、distinct 715；選取之後 accent 3 068 px（**未選取時 accent 是 0 px**，設計如此）；detail 畫布色 873 345 px，而 grid 只有 145 px。
- 斷言：非空白（distinct > 200）、格子區被 tile 色填滿（> 100 000 px）且不是整窗單色、選取才會出現 accent（before < 100、after > 1 000）、detail 畫布色 > 500 000 px 而 grid < 5 000 px。
- 注意：文字 token（`label #1d1d1f`）在螢幕上 0 px —— 小字全是反鋸齒，所以斷言只挑大面積色塊，不挑文字色。

### Q03 — L6 soak
- [ ] 未完成
- 要動：種子隨機 egui 事件（點擊/鍵/捲動/命令），不變量：不 panic、selection 合法、texture/LRU 有界、單幀 ≤ 250ms、來源檔案不被改。短跑進 CI，長跑 `#[ignore]`。

### Q04 — L7 色彩參考值
- [ ] 未完成（依賴 F07）
- 要動：合成 HEIF fixture → tone-map 輸出 vs 手算參考值，`max_diff ≤ 1`，不依賴 GPU。

### Q05 — L8 corpus + skip 記錄（含 `skip!` helper 本身）
- [ ] 未完成
- 現況：`spectrum-testkit` 裡**沒有 `skip!`**，也沒有 corpus 下載與 sha256 驗證。所以「素材缺席」现在是空心通過，不是可见的跳過。
- 要動：`skip!(Need|Corpus, "why")` 寫 JSONL（例 `target/ci/skips.jsonl`）+ `xtask corpus --download` sha256 pin（不符就删檔重抓）+ `xtask ci` 把跳過清單带進報告。

### Q06 — `xtask ci` 產報告
- [ ] 未完成
- 現況：`cargo xtask ci` 只做 fmt / token drift / snapshot，綠就過。
- 要動：產 `target/ci/report.json` + `target/ci/summary.md`（每步結果、測試數、跳過清單、耗時）。

### Q07 — 修 `docs/testing.md` 的誠實度
- [ ] 未完成
- 現況：表格把 L0–L8 全標「是」，但 L3/L6/L7/L8 其實沒做（本文件 Q01/Q03/Q04/Q05）。文件跟事實不符會讓人誤判完成度。

---

## 我建議的批次

| 批次 | 項目 | 理由 |
|---|---|---|
| 今天可做完 | **F01 → F02 → F03 → F04 → F05** | 都是「實作已在 core、缺 dispatch/UI」的小缺口；每項都能帶測試，做完程式就「重開還在、能用鍵盤、能復原、改了看得到」 |
| 接下來 | **Q01 → Q02 → Q03 → Q07** | 自動化補齊，之後大項目的改動才有網 |
| 大工程 | **F06 → F07（含 Q04）→ F08** | 詳細檢視是 tone-map 的落點，tone-map 是整個 port 的核心技術風險 |
| 需要你先決策 | **F16（影片依賴）**、**F12（inotify vs 輪詢）**、**F17（IMU 來源）** | 涉及「純 Rust」約束或平台可行性，我不會自行選 |
| 排隊 | F09、F10、F11、F13、F14、F15、F18、F19 | 有明確參考實作，可以單獨做，互不阻塞 |
