# Spectrum — instructions for agents

Spectrum 是 macOS Spectrum（Sony HLG 照片／影片檢視器，約 23.5k 行 Swift）的 Linux 原生移植。
整個專案只有 Rust；UI 用 egui／eframe，測試用 `egui_kittest`。開發紀律以 FigCraft／ArtCraft
（`storytold/*`，純 Rust + egui 同類專案）為模板，測試分層細節見 `docs/testing.md`。

## 每次 session 的起點
1. `docs/ui-spec.md`（版面、尺寸、元件 id）、`docs/testing.md`（測試分層與慣例）、
   `docs/dependencies.md`（依賴決定）、`docs/devlog.md` 最後幾條（最近做過什麼、踩過什麼）。
2. 順序固定：orient → plan → implement + test → verify（看結果）→ record（devlog）→ commit。
3. 使用者沒指定任務時，從 `docs/testing.md` 第四節「實作順序」的下一未完成項開始，不要跳步驟。

## 不可妥協

### 參考碼與素材
- macOS Swift 碼在 repo 外的本地參考複製（`~/git/spectrum-macos-ref`），**只當行為參考**：可以讀、可以對照，
  但 **不得把任何 Swift 檔案 commit 或 push** 到 `Spectrum-Linux`。
- **不得 commit 個人照片、個人路徑或真實媒體**。測試素材一律由 `crates-testkit`（`spectrum-testkit`）
  程序生成。外部素材若要測，走 sha256 pin 的 corpus（gitignored），缺席就 `skip!` 並記錄。
  `~/Pictures/...` 這類資料夾只能作為執行參數，不出現在 repo、fixture 或文件裡。
- 字型、圖示、任何 asset 必須是自己画的或可再分散的授權，並在 `docs/dependencies.md` 記來源。

### 不會 crash
- 產品碼不得出現 `unwrap()`／`expect()`／`panic!`／`unreachable!`／`todo!`／`unimplemented!`
  （workspace clippy 已 deny；只有 `spectrum-testkit` 有 `#[allow]` 並附理由）。回 `Result`／`Option`，
  資料來的索引一律 `.get()`，字串只在 char boundary 切，float 要 finite，size 要有上限。
- `unsafe_code = "forbid"`（workspace）。
- **每個 crash 修復都要附一條「修復前會 panic／會紅」的回歸測試。**
- 每個 command id 都要過 junk-parameter sweep（各種垃圾參數不得 panic，只能回 `CommandError`）。
  新增 parser 或 command 就擴這條 sweep。

### 架構
- **一切經 command registry。** 任何 model 變更都走 `spectrum-core::command::dispatch(id, params)`；
  UI、快捷鍵、未來的 CLI／MCP 派發同一組 id。被拒絕的 dispatch **不得改 model**，原因放 `AppState.notice`。
- **UI 薄。** `spectrum-app` 只讀 model、送 command；顏色只來自 `theme`／`tokens`（由 fig
  → `observed.json` → `theme.json` → `tokens.rs`，`cargo xtask design`／`theme` 在 CI 驗證）。
- **程式化呼叫不開 dialog**（不 `rfd`、不阻塞式 modal）；command 只「請求」UI。
- **分層由 cargo 強制**：`spectrum-core`（不得依賴 egui/eframe）→ `spectrum-app`（egui/eframe）；
  `spectrum-testkit` 不依賴 `spectrum-core`，只當 dev-dependency。
- **只有 Rust**，不寫 JS/TS，不引入 C/C++ 依賴。
- 日期／calendar 運算集中在 `spectrum-core::timeline`（有測試），不要在 UI 或各模組各寫一份。

## 測試紀律

分層表（L0–L9，各層證明什麼）在 `docs/testing.md` 第二節。這裡是規則。

**1. 可位址性（addressability）**
每個互動元件每幀都要 `widgets::register(id, rect)`（寫進 `ctx.data`，app 收成清單，`inspect()` 公開）。
id 穩定、記在 `docs/ui-spec.md`。斷言是「**每幀必須存在這些 id**」（`panel:sidebar`、`panel:inspector`、
`grid:scroll`、`tile:<name>`、`badge:hdr`、`segment:view`…）：id 缺席就 **fail**，不是「找不到就跳過」。
測試與 agent 都用 id 操作，不用座標、不用文字比對。

**2. 只走公開通道**
驅動用 `egui_kittest` 送 egui event（click、key、type、scroll）＋ command id；斷言讀結果與 app
對外暴露的欄位。不戳私有實作欄位。**禁止 OS 層合成輸入**（`xdotool`／`ydotool`／`wmctrl`／
`scrot` 之類）：它會打到 frontmost 視窗，而且證明不了 app 自己的行為。

**3. logic 與 ui 分離**
`app.logic(ctx)` 處理輸入與命令，`app.ui(ui)` 只畫，讓邏輯可以單測、UI 可以被驅動或跳過。

**4. 自繪元件必須顯式給無障礙名稱**
自繪的 button／slider／tile 要設 AccessKit name（與 value／range、選取狀態）。audit 測試走整棵
AccessKit tree：互動節點沒有 name → fail；slider 沒有 value 與 range → fail；鍵盤無法到達 → fail。
無障礙是可查詢的測試介面，不是附加功能。

**5. 像素 snapshot 只比「完全穩定」的幀，容差要有理由**
`settle()` 逾時就 fail（半渲染的圖會出現誤導性 diff）。容差 2 LSB 的理由是同一張圖可能一次走 GPU、
一次走 CPU fallback。斷言寫成「哪些像素、為什麼可以不同」，不是「圖一樣」。另外要有廉價斷言：
distinct colours > N、主要面板非空白。baseline 命名 `<screen>_<state>.png`；
`*.diff.png`／`*.new.png` 不進 repo。egui 0.36 在版面改動後第一次跑會 diff：改完版面要用
`UPDATE_SNAPSHOTS=1 cargo test -p spectrum-app --test e2e` 重生成，並在 commit／devlog 寫明重生成過。

**6. 跳過必須可稽核**
libtest 沒有 skipped 狀態。跳過一律 `skip!(Need, "why")`（寫 JSONL），`cargo xtask ci` 產
`target/ci/report.json` 與 `target/ci/summary.md`（含跳過清單與本機 GPU／HDR 探測）。
「全部通過」不得掩蓋「那層根本沒跑」。裸 `return` 跳過 = 違規。

**7. soak 守「不會 crash」，不守「功能正確」**
種子化隨機輸入（點擊、拖曳、鍵、修飾鍵、打字、滾輪）跑整個視窗，每隔幾個動作查不變量：
無 panic、選取永遠合法（index 在範圍內、切資料夾不越界）、texture／thumb cache 有上限、
undo journal 有上限、平均幀時間有上限（250 ms）、**磁碟上的檔案沒被改**。
短跑進 CI；長跑 `#[ignore]` + 環境變數（例：`SPECTRUM_UI_SOAK_STEPS=5000 ... -- --ignored`）。

**8. 每個新功能至少一條「拿掉功能就會紅」的測試**；每個 bug fix 附修復前會失敗的回歸測試。
斷言要帶 reason（例：`"the cicp profile, not the first profile, decides the transfer"`），
這樣失敗訊息自己說明是哪條紀律破了。

**9. 測試素材**
`crates/spectrum-testkit` 產生 HEIF／JPEG／MP4／PNG／ICC 合成檔。fixture 的 tag size 必須等於
實際寫入的長度（寫錯會偽裝成「偵測不到 HDR」）。**每個平行測試一個固定 fixture 目錄名**
（`/tmp/spectrum-egui-<tag>`），共用目錄會互相改寫檔案、造成 snapshot 漂移與假失敗。

**10. 誠實宣稱**
文件與 devlog 只能寫做過且驗證過的等級；未實作的層（目前：widget registry、logic/ui 分離、
AccessKit audit、soak、junk-parameter sweep、`skip!`＋ci report）要標為未做，grade by behaviour，err low。

## 品質門檻（每次 commit 前）
```bash
export RUSTUP_HOME=$PWD/.toolchain/rustup CARGO_HOME=$PWD/.toolchain/cargo PATH="$PWD/.toolchain/cargo/bin:$PATH"
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings   # 0 error，CI gate
cargo test --workspace
cargo run -q -p xtask -- ci                              # fmt/clippy/test + design + theme
```
一個 commit 一條工作（新功能／bug fix／重構不混在一起），訊息寫改了什么與測試結果（例：
`HEIF: pick the ICC profile that carries the HDR signal (226 tests passed)`）。
每次完成一段就 commit 並 push 到 `main`。

## 執行與看結果
- 執行：`cargo run -p spectrum-app -- <照片資料夾>`（要在 repo 根目錄跑，字型路徑
  `assets/fonts/Inter.ttf` 是相對路徑）。背景啟動：
  `DISPLAY=:0 nohup ./target/debug/spectrum-app <dir> >/tmp/spectrum-app.log 2>&1 &`。
  Vulkan 的 radv warning 屬正常。
- 看掃描結果：`cargo run -q -p xtask -- scan <dir>`（每檔的 transfer 訊號、`cicp p/t/m`、
  gain map、ICC 描述、HDR 計數）。這是驗證色彩偵測最快的路徑。
- **UI 改完要看結果**：用 `egui_kittest` snapshot 產 PNG 並實際讀圖；本機沒有 `scrot`／
  ImageMagick／`xdotool`，不要指望截真实視窗。
- 平行 agent：各自 `CARGO_TARGET_DIR`，只改自己擁有的檔案，做完刪掉 target 目錄。

## 文件紀律
- 每個新功能／bug fix 在 `docs/devlog.md` 追加一條：**問題／根因與做法／修改的檔案／測試／注意（踩到的）**。
- 版面與元件 id 改動 → 同步改 `docs/ui-spec.md`；測試等級或慣例改動 → 同步改 `docs/testing.md`；
  新增依賴 → 記 `docs/dependencies.md` 與理由。
- 寫作風格：平實用詞、附測量數據（ms、像素、測試數）、不做無依據的斷言。
- 對話與文件用**繁體中文**；程式碼、路徑、指令、錯誤訊息、模型輸出保持原文。
