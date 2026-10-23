# Backlog（待開發清單）

對照 `~/git/spectrum-macos-ref`（Swift 11,899 行）與目前 Rust 版（`spectrum-core` 4,550 行、
`spectrum-app` 2,477 行）逐模組比過的結果。每項都寫了「MacOS 參考規模／目前 Rust 狀態／要動什麼」，
編號給使用者選。**狀態是 2026-10-23 的實際檢查**，不是推測。

用法：跟使用者說編號即可（例：`做 F01、F02`）。做完的項目由實作的人把 `[ ]` 改成 `[x]` 並在
`docs/devlog.md` 寫一筆。

---

## 功能缺口（F）

### F01 — 資料夾清單持久化
- [ ] 未完成
- macOS：`Services/BookmarkService.swift`(98) 用 security-scoped bookmark 記住資料夾。
- 現況：`Library.folders`（`model.rs`）只在記憶體，`model.rs` 裡沒有任何 save/load → **重開程式資料夾清單就空了**，要重新 Add folder。
- 要動：把 `folders` 寫成設定檔（例 `~/.config/spectrum/library.json`），啟動時載入並逐檔驗證路徑還在；加 `folder.save` 不需要（`folder.add`/`folder.remove` 觸發寫入即可）。
- 附带：啟動時自動 `rescan`（目前要手按 Rescan）。

### F02 — 鍵盤導覽與快捷鍵
- [ ] 未完成
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
- [ ] 未完成
- macOS：`Views/Detail/PhotoDetailView.swift`(1138) + `PhotoInfoPanel.swift`(591)。
- 現況：只有 260px inspector（caption、訊號 chip、Adjust/Info、rotate/flip/restore/trash）。**沒有單圖檢視、沒有縮放/平移、沒有全螢幕模式。**
- 要動：一個 `detail` 畫面（或 inspector 右側的大畫布）：全解析度顯示（走 glycin 全圖）、zoom/pan、`detail:scroll`/`button:zoom-in` 等 registry id。這是 F07 tone-map 的落點。

### F07 — HDR tone-mapping 與顯示模式（HLG / PQ / gain-map）
- [ ] 未完成
- macOS：`AVFMetalView`(1192) + `MetalShaders`(247) + `HDRImageViews`(87)。
- 現況：`hdr.rs`(330) **只偵測**訊號（cicp PQ/HLG、ICC dmnd、gain-map `hdgm`；修好之後 24 檔判為 HDR）。`hdr.toggle` 只改顯示模式與 badge，**沒有任何色彩轉換、沒有 GPU 輸出、沒有 headroom 呈現**。
- 要動：CPU 參考實作（HLG OETF/EOTF、PQ、gain-map blend）＋ `docs/testing.md` L7 的參考值測試（`max_diff ≤ 1`）；再決定要不要 wgpu shader。這是本專案最大的技術缺口。

### F08 — 互動裁剪 UI
- [ ] 未完成
- macOS：`CropOverlayView.swift`(302)。
- 現況：`crop.rs`(58) 有 `CropRect`（含 rotated/flipped 換算）、`edit.crop` 命令存在，但 **UI 沒有任何裁剪控製**；`app.rs:540` 註解自己留了「if a second input is added (crop fields, search), gate this on the field's focus」。
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

### F11 — 剪贴簿：複製／搬移到資料夾
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
- 決策點：播放必然引入非純 Rust 依賴（gstreamer 或 ffmpeg）。選項 (a) 允許 gstreamer 外掛、(b) 只顯示首幀 + 元資料、(c) 整塊延後。選完才开工。

### F17 — 陀螺儀穩定（360° 內容）
- [ ] 未完成
- macOS：`GyroCore.swift`(456) + `GyroConfig.swift`。
- 現況：完全沒有。Linux 端要讀 IMU/姿勢來源（`linuxptp`? 沒有標準 API）→ 需要可行性研究，不是直接开工的項目。

### F18 — HLG 匯出
- [ ] 未完成
- macOS：`Services/HLGExportService.swift`。
- 現況：只有 XMP sidecar 寫回編輯，沒有匯出。依賴 F07（先有 tone-map 才有匯出可談）。

### F19 — 網路磁碟
- [ ] 未完成
- macOS：`NetworkVolumeService.swift`(119)。
- 現況：無。Linux 走 `/mnt`、`gvfs`、SMB 掛載點；主要工作是「掛載點慢/斷線時不要卡住掃描」→ 需要 timeout 與可中斷掃描。

---

## 測試與紀律缺口（Q）

### Q01 — L3 AccessKit audit
- [ ] 未完成
- 要動：走 kittest 的 AccessKit 樹，斷言互動節點必有 name、slider 必有 value/range、可鍵盤到達。基礎已鋪好（badge 文字改成真 `Label` 就是為了這個）。

### Q02 — L5 廉价像素斷言
- [ ] 未完成
- 現況：只有 snapshot 基線比對（`library_all.png`、`detail_adjust.png`）。
- 要動：`harness.render()` 之後斷言「畫面不是空白」+「設計 token 顏色存在」（accent `#0b6fe0`、tile `#e9e9eb`），基線漂了也能先抓到結構問題。

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
