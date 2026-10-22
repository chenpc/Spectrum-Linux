# 測試設計

研究對象：`storytold/lightcraft`、`storytold/deckcraft`、`storytold/photocraft`（純 Rust + egui）。
下面第一節是他們的做法與背後的理由，第二節是 Spectrum 要採用的分層，第三節是慣例，第四節是實作順序。

## 一、從 ArtCraft 學到的理念

**1. 可位址性（addressability）是自動化能力的根基**
LightCraft 每個互動元件每幀都會向一個 registry 註冊穩定 id 與 rect：
`crates/ui-egui/src/widgets.rs` 的 `register(ctx, id, rect)` 寫進 `ctx.data`，app 把它收成
`app.widgets: Vec<(String, Rect)>`。控制協議把這份清單公開成 `ui.widgets {filter}`，点击用
`ui.clickWidget {id}`，找不到就回 `no widget <id> on screen (see ui.widgets)`。
測試與 agent 都用 id 操作，不用座標、不用文字比對。id 同時出現在 parity checklist，`cargo xtask
parity` 在 CI 檢查引用的 id 是否還存在。

**2. 測試只走公開通道**
`crates/ui-egui/src/headless.rs` 的 `Headless` 是無視窗的完整 app session，測試用它送
`ui.set`／`ui.move`／`ui.scroll`／`ui.key`／`engine.execute {command, params}`，跟使用者與 MCP 走
同一條路。斷言讀的是結果與 app 對外暴露的欄位（`grid_scroll`、`session.selection`），不是私有實作細節。

**3. logic 與 ui 分離**
`app.logic(ctx)` 處理輸入與命令，`app.ui(ui)` 只畫。kittest harness 的寫法是
`build_ui_state(|ui, app| { app.logic(ui.ctx()); app.ui(ui); }, app)`。這樣 UI 可以被驅動，也可以被
跳過而單測邏輯。

**4. 自繪元件必須顯式暴露無障礙名稱**
自繪的 slider／button 透過 `announce()`／`announce_selected()` 設 AccessKit 名稱與狀態，
`crates/ui-egui/tests/accessibility.rs` 走整棵 AccessKit tree，規定：互動節點沒有 name → fail、
slider 沒有 value 與 range → fail、canvas 工具鍵盤無法到達 → fail。無障礙不是附加功能，是測試的
可查詢介面。

**5. 像素比對只比「完全穩定」的幀，且容差有理由**
`settle()` 逾時就 fail，因為半渲染的圖會出現誤導性的 pixel diff。兩次 run 比對時容差設 2 LSB，
理由是同一張照片可能一次走 GPU、一次走 CPU fallback。斷言不是「圖一樣」，而是「哪些像素、為什麼可以不同」。
另有「不是空白」的廉价斷言：distinct colours > 200、top bar 有亮字 > 30 px、`thumb_textures() > 0`。

**6. 跳過必須可稽核**
libtest 沒有 skipped 狀態。`lightcraft_testkit::skip!(Gpu, "why")` 除了印訊息，还把
`{need, reason, test, binary, location}` 寫進 JSONL；`cargo xtask ci` 寫 `target/ci/report.json` 與
`target/ci/summary.md`，並附本機 GPU adapter 探測。所以「全部通過」不會掩蓋「GPU 測試根本沒跑」。

**7. soak 測試守「不會 crash」而非「功能正確」**
`deckcraft` 的 `ui_soak_tests.rs`：種子化隨機輸入（點擊、拖曳、鍵、修飾鍵、打字、滾輪）跑整個視窗，
每隔幾個動作檢查不變量：無 panic、文件有效、undo／journal／texture cache 有界、平均幀時間有界
（`MAX_MEAN_FRAME = 250 ms`、`MAX_TEXTURES = 1500`、`MAX_THUMBS = 600`、`MAX_JOURNAL = 10000`）。
短跑進 CI，長跑用環境變數加 `--ignored`：`DECKCRAFT_UI_SOAK_STEPS=5000 ... -- --ignored`。

**8. 不用 OS 層合成輸入**
AGENTS.md 明令禁止 osascript／`cliclick`／accessibility 自動化：它會打到 frontmost 視窗。所有 UI
驅動都走 app 自己的通道。

**9. 測試分層，並寫明每層證明什麼**
文件白紙黑字區分：控制通道注入 egui event 不等於 OS input delivery；native acceptance 必須拒絕
stderr 的 `rendering headlessly`（`ui.screenshot` 2 秒沒 frame 會自動退回 CPU 渲染，對腳本方便但
不能證明真視窗）；截圖前要 poll `ui.inspect` 到 `loupe.pending == false`。

**10. 每個 crash 修復都要有「修復前會 panic」的回歸測試**，產出碼禁 `unwrap`／`expect`／`panic`／`unsafe`
（clippy `deny`，`clippy.toml` 只在測試開放）。

**11. 语料庫不進 repo**：`cargo xtask corpus --download` 抓 gitignored 的 `corpus/`，逐檔 sha256 驗證，
不符就刪檔重抓；測試素材一律程序生成或 CC0。

## 二、Spectrum-Linux 的測試分層

| 層 | 內容 | 工具 | 進 CI |
|---|---|---|---|
| L0 | core 單元：XMP round-trip、HLG 訊號偵測、timeline 分組、selection 語意、thumbnail LRU、trash | `cargo test -p spectrum-core` | 是 |
| L1 | 命令層：每個 command id 的 dispatch、guard、參數錯誤回可行動的錯誤、拒絕時不改 model | `cargo test -p spectrum-core --test command`（23 條命令，14 tests）；`spectrum-cli` JSON-lines 外殼待建 | 是 |
| L2 | UI 行為：透過 label/rect 驅動 grid 選取、view 切換、HDR badge、inspector tab | `egui_kittest` | 是 |
| L3 | UI 結構 audit：AccessKit tree 裡互動節點必有 name、slider 必有 value/range、可鍵盤到達 | `egui_kittest` AccessKit | 是 |
| L4 | 可位址性：每幀 registry 必含預期 id（`panel:sidebar`、`panel:inspector`、`grid:scroll`、`tile:<name>`、`badge:hdr`、`segment:view`）；id 缺就 fail | registry 斷言 | 是 |
| L5 | 像素 snapshot：headless 渲染、`settle` 後比，容差 2 LSB；「非空白」廉价斷言 | `Headless` + `egui_kittest::snapshot` | 是 |
| L6 | soak：種子隨機輸入 + 不變量（無 panic、texture cache ≤ 600、journal 有界、平均幀 ≤ 250 ms） | `egui_kittest` | 短跑進 CI，長跑 `--ignored` |
| L7 | 色彩參考值：合成 HEIF fixture → HLG tone-map 輸出 vs 手算參考值，`max_diff ≤ 1` | core + CPU 參考實作 | 是（不依賴 GPU） |
| L8 | corpus：外部素材 sha256 pin；缺席時 `skip!(Corpus, ...)` 記錄 | `xtask corpus` | 是（記錄跳過） |
| L9 | native acceptance：真視窗、Wayland 色管理、HDR 輸出、螢幕閱讀器 | 真機人工／nightly | 否，明標為非 CI |

`cargo xtask ci` 跑 L0–L8，並產生 `target/ci/report.json` 與 `target/ci/summary.md`（含跳過清單與本機
GPU／HDR 探測）。

## 三、慣例

- 每個新功能至少一個「拿掉功能就會紅」的 test。
- 測試透過公開通道驅動：id、label、command id；不戳私有欄位。
- 不 commit 媒體素材；fixture 由 `fixture.rs` 程序生成。snapshot baseline 是例外（小、確定性）。
- snapshot baseline 命名 `<screen>_<state>.png`；`*.diff.png`／`*.new.png` 不進 repo（已在 `.gitignore`）。
- 跳過一律用 `skip!(Need, "why")`，不用裸 `return`。
- crash 修復必附回歸測試，且該測試在修復前會 panic。
- UI 測試不用 OS 合成輸入。
- 每個 bug fix／新功能在 `docs/devlog.md` 追加一條（問題／根因／修改檔案）。
- 上面的「不可妥協」與測試紀律以 repo 根 `AGENTS.md` 為準；本文件只寫分層與實作順序。

## 四、實作順序（先測試骨架，再畫 UI）

1. **command registry**（`spectrum-core::command`）：`{id, label, shortcut, params, enabled, run}`；UI 只按 id 派發。✅ 已做（23 條命令 + `crates/spectrum-core/tests/command.rs`），模型在 `spectrum-core::model::Library`。
2. **widget registry**（`spectrum-app::widgets::register`）：每個互動元件註冊 id + rect；`inspect()` 洩露清單。
3. **`logic(ctx)` / `ui(ui)` 分離**，讓 L1 可單測邏輯。
4. **testkit `skip!`**（移植 LightCraft 的 190 行）+ `xtask ci` 的 `report.json`／`summary.md`。
5. **AccessKit audit**（L3）與 **registry 斷言**（L4）。
6. **soak**（L6）。
7. 之後才把 `theme.rs`／`widgets.rs` 接成完整畫面，並補 L5 snapshot。

已存在的 `theme.rs` 與 `widgets.rs` 是在這條順序之前寫的草稿，尚未接進 app；會按上面的規則重寫
（加 registry 註冊與 AccessKit 名稱）。
