# UI 設計規格

設計來源：`../design/osx-photos-prototype.figcraft`（FigCraft，750 nodes，4 個 screen）。
觀測值由 `cargo xtask design --write` 從該檔抽出到 `assets/design/observed.json`；語意角色由
`assets/design/theme.json` 定義，`cargo xtask css` 產生 `assets/design/theme.css`。
所有數字都是從文件量出來的，不是估的。

## Screen 清單與流程

| Screen | 尺寸 | 內容 |
|---|---|---|
| Screen 1 · Library | 1440×900 | 側欄 + 日期分組的縮圖 grid |
| Screen 2 · Photo + Edit | 1440×900 | 檢視器 + 右側 inspector（Adjust / Info） |
| Screen 3 · Memories | 1440×900 | 精選集卡片牆 |
| Screen 4 · Months | 1440×900 | 月曆網格 |

流程全部是 `onClick → navigate`，`dissolve` 過場，`220 ms`、`easeOut`。
Library 的 `Months`/`Years` 與 `Memories`/`Shared Albums` 導向對應 screen；grid 裡任一張照片導向 Screen 2。

## 版面幾何

| 元件 | 值 |
|---|---|
| Titlebar | 高 52，水平 gap 12，左右 padding 14，底邊 1px 分隔線 |
| Sidebar | 寬 232，`gap 2`，padding 10；項目列高 26 |
| Sidebar 分隔線 | 1px |
| Main（Library） | padding `[16, 20, 16, 20]`，垂直 gap 14 |
| Month header | 高 22，gap 9 |
| Photo grid | 縮圖 164×164，gap 2 |
| Viewer（Detail） | padding 26，垂直 gap 16，置中對齊 |
| Photo（Detail） | 780×540，radius 2；左右導航按鈕 34×34，radius 17，`#ffffff@60%` |
| Detail 控制列（本實作） | 高 38（26 px 控製 + 上下 6），`−` `1:1` `Fit` `+` `NN%` `Close` |
| Detail 畫布（本實作） | 佔滿 main 那一欄剩餘高度；底色 `detail`（`#f7f7f8`）；zoom 0.02–16；全圖解到長邊 2400 |
| Caption | 垂直 gap 3 |
| Inspector | 寬 260，padding `[12, 14, 12, 14]`，垂直 gap 14 |
| Inspector tabs | 高 26，容器 `gap 2` / `padding 2`，選項高 22 |
| 一般控製 | 高 26，radius 6 |
| 滑桿 | 軌道 2px、radius 1、旋鈕 12×12 |
| Filter chip | 40×56（含 12px 標籤） |

## 顏色角色

| 角色 | 值 | 用途 |
|---|---|---|
| window | `#ffffff` | Library 主區、選中的 segment |
| titlebar | `#ededef` | 標題列 |
| sidebar | `#f2f2f3` | 左側與右側 inspector 底色 |
| detail | `#f7f7f8` | 檢視器底色 |
| hairline | `#d3d3d6` | 1px 分隔線 |
| divider | `#d6d6d8` | inspector 內部分隔 |
| text | `#2c2c2e` | 主要文字、選中 segment 文字 |
| ink | `#333335` | 控製項文字 |
| label | `#4a4a4e` | 滑桿標籤、caption |
| secondary | `#8a8a8e` | 次級文字、section 標題 |
| muted | `#7b7b7e` | 未選中的 segment |
| row | `#3f3f43` | 側欄項目 |
| row_selected_text | `#26262a` | 選中側欄項目文字 |
| accent | `#0b6fe0` | 主要動作、滑桿已填滿段 |
| track | `#c9c9cc` | 滑桿未填滿段 |
| control | `#e1e1e3` | 次級按鈕底 |
| selected | `#d7d7da` | 側欄選中底、hover |
| segmented | `#dcdcde` | 分段控製容器 |
| tile | `#e9e9eb` | 縮圖佔位底 |
| scrim | `#ffffff@60` | 導航按鈕底（半透明） |

設計中出現 62 次 `#ffffff`、45 次 `#e9e9eb`、32 次 `#3f3f43`；`#0b6fe0` 出現 9 次，是唯一的重點色。

## 字體

字族只有 `Inter`，weight 用 Regular / Medium / Semibold。觀測到的尺寸分佈（頻率）：

| 尺寸 | weight | 頻率 | 角色 |
|---|---|---|---|
| 13 | Regular | 32 | 側欄項目 |
| 11 | Regular | 23 | 次級說明 |
| 14 | Semibold | 13 | 區塊標題 |
| 11.5 | Regular | 10 | 滑桿標籤 |
| 12.5 | Medium | 8 | 控製項文字 |
| 12.5 | Regular | 7 | 搜尋佔位文字 |
| 11 | Semibold | 6 | section 標題（Albums、Filters） |
| 10 | Regular | 5 | filter chip 標籤 |
| 15 | Semibold | 4 | 月份標題 |

radius 分佈：5（39 次）、4（28）、6（22）、1（14）、10（4）、17（2）。

## Spectrum 對應

設計是 macOS Photos 的版面，Spectrum 的內容需要重新映射：

| 設計元件 | Spectrum 用途 |
|---|---|
| Library popup | 圖庫根目錄（原 macOS 版是 security-scoped bookmark，Linux 直接是路徑） |
| View switch（All / Months / Years） | 時間線顯示模式；對應 `timeline::group_by_month` 與 `SortKey` |
| Sidebar 項目 | **只有 Folder view**：已加入的資料夾清單（見下節），不做資料夾樹 |
| Photo grid 164×164 gap 2 | `ThumbnailScheduler` 的縮圖，依 `group_label` 分月 |
| Caption 第二行 | EXIF 行（`ExifData`：機型、焦段、光圈、快門、ISO） |
| Inspector tabs | `Adjust` / `Info`；Adjust 內放 HDR/SDR 切換、陀螺儀穩定開關與參數 |
| 滑桿 | 陀螺儀 smoothing、sync offset、FOV scaling、crop |
| `Edit` 主色按鈕 | 進入編輯模式（rotate / flip / crop，寫 XMP sidecar） |
| `Info` 按鈕 | 切到 Info tab（HDR 訊號、色域、headroom、影片元資料） |

設計尚未涵蓋、需要補進 fig 的項目：

- HDR/SDR 切換的 badge（原 macOS 版用 badge 呈現 HDR、headroom 與 color profile）
- 影片播放控製列與渲染診斷（FPS、CV、colorspace、陀螺儀穩定指數）
- 狀態列（掃描 / 複製 / 匯入進度）
- 選取態（multi-select、marquee）與 subfolder tile

## 尚未套用的部分

`assets/design/observed.json` 與 `assets/design/theme.css` 需要 `cargo xtask design --write` 與
`cargo xtask css --write` 產生（toolchain 在 repo 內：`.toolchain/`，`cargo xtask ci` 會驗）。


## 目前的實作版面

2026-10-22 起，`crates/spectrum-app` 實際畫出的面板（尺寸全部取自 `tokens.rs`）：

| 面板 | 尺寸 | 內容 |
|---|---|---|
| `titlebar` | 高 52 | 資料夾名 + `N items`，右端 `Rescan` |
| `sidebar` | 寬 232 | **只有 Folder view**：`Folders` 清單（`名稱  數量`，選中者高亮）、路徑輸入框 + `Add folder`、`Remove folder` |
| `main` | 視窗減側欄減 inspector，padding `[16, 20, 16, 20]` | 第一列是 `toolbar`，下面才是 grid |
| `toolbar` | 高 42（控製項 26 + 上下各 6）、底邊 1px hairline | `All Photos / Months / Years` 分段、`All / Photos / Videos` 分段、`HDR only`、`Clear filter`、`Name / Date / Kind / HDR first / Size` 分段、`Natural order ⇄ Reversed` |
| 月份標題 | 15 Semibold `#2c2c2e` + 11 `#8a8a8e` 數量 | `August 2016` + `12 Photos`（混合影片時寫 `Items`），點標題摺疊該月 |
| grid | 縮圖 164×164、gap 2、行距 166 | 欄位數依可用寬度算（設計 1167 px → 7 欄）；HDR/SDR badge 畫在**磁磚內**左上角 |
| `inspector` | 寬 260，**只在有選取時出現** | caption（檔名 + 中繼）、訊號 chip、`Adjust / Info` 分段、rotate / flip / restore / trash、Info 頁的訊號與元資料列 |
| `statusbar` | 只在有訊息時出現 | 狀態列文字或被拒絕命令的原因 |

字體：設計的字族 `Inter` 尚未 vendored（OFL，尚未放進 `assets/fonts`）；缺字時 egui 會畫 tofu 方框，
所以 `theme::install_fonts` 會載入系統 CJK 字型當 fallback（候選清單 `theme::CJK_CANDIDATES`，
可用 `SPECTRUM_CJK_FONT` 指定，例如 `arphic/uming.ttc` 這類繁中造型）。啟動時會印出實際載入的路徑。

與 fig 原型的差異（刻意為之）：
- 原型側欄是 Albums / Filters / 年份清單；這裡改成只放資料夾清單，因為 Spectrum 的來源是「使用者加入的資料夾」，篩選與排序移到 `toolbar`。之後要以 **Add folder** 增加資料夾，所以側欄需要一個路徑輸入框。
- 移除資料夾只從清單拿掉，**不刪檔案**（`folder.remove` 的測試直接斷言檔案還在）。
- 磁磚顯示**真實縮圖**（`thumbs::Worker` 一條解碼執行緒 → `ThumbnailService`：glycin 解碼 →
  `downscale` box-average → `crop_center_square` 中心方型裁剪）。egui 0.36 不公開 `UvRect`，所以
  裁剪在像素層面做：4:3 照片被裁掉兩邊而不是壓扁。解不了的檔案（影片、未知格式）永久走純色佔位
  + 檔名；`thumb_asked` / `thumb_failed` 保證不會每帧重排同一檔。解碼器只在 real window 與
  `tests/thumbnails.rs` 裡掛，所以快照不受解碼時間影響。
- 原型的 Screen 1 沒有 inspector 欄（`Main` 一路到視窗右緣）；這裡選取一張才出現 inspector，所以沒選取時 grid 是 7 欄、選取後變 5 欄。
- 側欄與工具列的相對位置照設計：側欄從 titlebar 下面開始，工具列只佔 `main` 那一欄。

實作時踩到的版面教訓（都有測試或註解守護）：

1. **面板高度會裁切內容但節點仍留在樹裡**：把 view 分段放在 52 px 標題列的第二列時，它的 rect 落在 y 57–83（工具列的位置），點擊會打到工具列。標題列只能有一列。
2. **分段控製不能依 `available_width` 分配-cell 寬度**：這樣 track 會吃到整列寬度，把後面的控製項推到視窗外，節點存在但點不到。`widgets::segmented` 改成依文字寬 hug，`no_control_sits_outside_the_window` 測試斷言每個 Button / TextInput 的 rect 中心都在視窗內。
3. **`Grid` 的欄寬是依「看過的 cell」算的**：32 個檔案時後面的欄位收到 HDR badge 的寬度（~42 px），磁磚互相重疊 122 px。改成自己算欄位數（`columns_for_width`）+ 每列 `ui.horizontal`。
4. **`horizontal_wrapped` 在垂直 `ScrollArea` 裡拿到的寬度是無限的**：一條 wrapped 列跑到 5236 px。欄位數必須在 scroll area **外面**量（`ui.available_width()`），再傳進去。
5. **`Ui::put` 會推進父 ui 的 cursor**：把 badge 用 `put` 疊在磁磚上，下一塊磁磚就從 pill 的右緣開始（實測 x 從 432 變成 306）。改成先在 `allocate_ui_with_layout(164×164)` 的子 ui 裡摆 content，父層 pitch 才不會被影響。
6. **畫出來的文字對 AccessKit 不存在**：badge 用 `painter.text` 之後，`query_all_by_label("HDR")` 變成 0 個。pill 底色可以畫，文字必須是真的 `Label`。
7. **`egui_kittest` 會把 app 包在 `outer_margin(8)` 的 Frame 裡**，所以測試裡每個面板都往內縮 8 px（1440 px 畫面只有 6 欄，真視窗是 7 欄）。欄位數改用設計自己的寬度驗（`grid_columns_match_the_design_widths`），畫面測試則斷言「不能空整欄」。
8. **詳細檢視取代同一欄的 toolbar/grid**：所以 `ids`（grid 清單）與 `ids-always`（標題列、側欄、資料夾列）分開列，`registry.rs::always_ids_survive_the_detail_view` 斷言開著時 grid 的 id **不存在**、關掉後又回來 — 腳本才不会點到沒畫的東西。
9. **`ui.allocate_rect` 必須在蓋在它上面的圖片之後**：egui 把游標交給「最後一個 rect 包含游標」的元件，圖片先画、畫布rect 後分配，拖曳才落在畫布上（實測順序反過來時 `dragged()` 永遠 false、pan 都是 0）。
10. **`count_and_consume_key(COMMAND, …)` 不認得只有 `ctrl` 的按鍵**：`Modifiers::cmd_ctrl_matches` 要求 pattern 的 `command` 位也設置；egui 的 `i.modifiers` 又只由 `ModifiersChanged` 事件更新（注入的 `Event::Key` 不更新它）。所以 zoom 快捷鍵同時試 `CTRL` 與 `COMMAND` 兩個 pattern（`zoom_key`）。
11. **egui 的拖曳距離是從「上一幀記住的按下位置」算的**：把 press/move/release 塞進同一批事件只會得到一次點擊；測試裡先輸一幀純 `PointerMoved` 清掉上一次點擊的按下位置，再把 press / move / release 分幀送（實測未清時 70 px 的拖曳量測成 +284 px）。
12. **平移的上限是圖片超出畫布的一半**：比畫布小的圖片不能平移（永遠置中），比較大的拖到邊緣就停 — 不會出現一條背景色（`clamp_pan`、`panning_stops_at_the_picture_edges`）。
13. **HDR 顯示（`button:hdr` / `hdr.toggle`）的語意是「把 scene-referred 壓進 SDR 範圍」**：開啟時
    HLG/PQ 檔案的像素經 `spectrum_core::tone_map`，關閉時直接顯示檔案編碼值（不認識 HDR 的檢視器看到的
    樣子）。只有 `nclx`（HEIF）與 `cICP`（PNG）——描述影像資料本身的訊號——會觸發轉換；ICC profile 推斷
    的訊號不會，因為 loader 已把那些檔案渲染成顯示就緒畫面。texture 以 `(path, transform)` 為 key，来回
    切換不重新解碼（像素證據：`crates/spectrum-app/tests/hdr.rs`）。
14. **eframe 對不可見視窗不跑 pass**：`ui.screenshot`（`ViewportCommand::Screenshot`）要由 pass 執行，而本機 Xwayland（mutter）把視窗標為不可見，所以請求會逾時（`request_nudged` 每 120ms `request_repaint`、5s 回報原因，`main.rs` 另有 300ms 心跳）。自動化渲染改用 `harness.render()` 讀真實像素。

## 詳細檢視（本實作新增）

原型的 Screen 2 是「檢視器 + inspector」，但它的照片是一張固定 780×540 的圖，沒有 zoom/pan 的控製。
本實作按設計語彙補上可用的詳細檢視：

| 元件 | 行為 |
|---|---|
| 控制列 | `−` `1:1` `Fit` `+`、`NN%` 讀數、`Close`；26 px / radius 6，與工具列同語彙 |
| 畫布 | 佔滿 main 那一欄；拖曳平移、`Ctrl/⌘ + 滾輪`縮放、一般滾輪平移 |
| zoom 值 | 「顯示像素 / 原始像素」，所以 `1.0` 就是 1:1、讀數就是該數 × 100；`Fit` = `None`（沒有明確值） |
| 上限 | 0.02–16（`DETAIL_MIN_SCALE` / `DETAIL_MAX_SCALE`）；命令層夾住，UI 不能自己亂跳 |
| 全圖 | 由 worker 以長邊 2400 解（與 grid 的 164 分開快取），上限 4 張同時持有 |

規則：
- `detail.open` 只在**恰好選一個**時可執行；`detail_index()` 也只在 sole 選取時回 `Some`，所以多選時畫面自動回 grid，不需要命令。
- `detail.zoom` 的 `in`/`out` 以**模型存的 zoom** 為準，但当 zoom 是 `None`（Fit）時用 UI 傳進來的 `current`（實際 fit 比例），才不會從 1:1 開始跳。
- zoom 存在 model（經命令），pan 存在 UI：只有畫布在乎圖片在哪。zoom 改變時畫布重新置中（`zoom_seen` 比對，不靠 model 的一次性旗標）。
- 關掉詳細檢視會連 zoom 一起清掉，下一張從 Fit 開始。
- 影片（F16 尚未實作）沒有 preview：畫布仍註冊 `canvas:detail`，控制列仍可驅動，圖片區顯示 `No preview available`。

## 鍵盤操作

| 按鍵 | 行為 | 走的命令 |
|---|---|---|
| `←` `→` | 可見清單裡前後一個 | `select.move {"direction": "left|right", "columns": N}` |
| `↑` `↓` | 上下一个**列**（N = 實際欄數） | 同上，`up` / `down` |
| `Home` / `End` | 第一個 / 最後一個（無選取時 `End` 直接落最後、其餘落第一個） | 同上 |
| `Esc` | 詳細檢視開著時先關掉它，否則清空選取 | `detail.close` / `select.clear` |
| `Enter` | path 欄有焦點時提交路徑；否則對**唯一選取**開詳細檢視 | `folder.add` / `detail.open` |
| `Ctrl/⌘ + +` / `−` | 詳細檢視裡放大 / 縮小 | `detail.zoom {"action": "in|out", "current": scale}` |
| 畫布拖曳 | 平移圖片 | 不經命令（pan 是 UI 狀態） |
| `Ctrl/⌘ + 滾輪` | 以畫布為基準放大 | `detail.zoom` |
| 一般滾輪 | 平移 | 同上 |

規則：
- 全部經 `dispatch()`，不直接改 model；越界時命令回 `{"moved": false}` 且**不改選取**（不是錯誤）。
- `columns` 由繪圖當下的實際欄數傳入（`columns_for_width(wrap_width)`），所以 `↓` 跟畫面的列對得起來；有 inspector 時欄數會變少，這是对的。
- path 欄有焦點時導航鍵不進格子（`path_focused`），否則輸入路徑時會把選取搬走。
- 鍵盤移動後會對該磁磚 `scroll_to_rect_animation(..., ScrollAnimation::none())`，連續要求 2 幀（`scroll_hold`）：第一次選取同時出現 inspector 会让網格變窄、該幀的 offset 會被重置，1 幀不夠。
- egui 的 `TextEdit` 預設 `return_key = Enter` 會**吃掉** Enter，所以 path 欄用 `.return_key(None)`，Enter 由 app 自己判（並要求焦點）。另外 `typed.clicked()` 曾經會直接提交路徑 — 點一下輸入框就會改資料夾清單，已修（`tests/keyboard.rs` 有回歸測試）。

## 元件 id 清單（addressability）

每個 id 都必須在每幀的 registry 裡出現（前綴結尾的 `:` 表示「至少一個該前綴的 id」）。
`crates/spectrum-app/tests/registry.rs` 會讀下面四個區塊：`ids` 是 grid 畫面每幀都要有，`ids-on-selection`
是「有選取時必須出現、沒選取時必須不存在」，`ids-on-detail` 是「詳細檢視開著時必須出現、關掉時必須
不存在」。區塊讀不到或為空會被當失敗（避免空心 checklist）。
`ids` 是 **grid 畫面**每幀都要有；詳細檢視會**取代**同一欄的工具列與 grid，所以開著時那些 id 必須
不存在（由 `ids-on-detail` 測試斷言）。`ids-always` 是兩種畫面都要有的最小清單（標題列、側欄、
資料夾列與輸入框）。

```ids
# grid 畫面每幀都要有（詳細檢視開著時這些必須都不存在）
panel:toolbar
grid:scroll
# toolbar
segment:view
segment:kind
segment:sort
button:filter-hdr
button:filter-clear
button:sort-order
# grid
group:
tile:
badge:
```

```ids-on-selection
# inspector：有選取時才出現
panel:inspector
inspector:scroll
badge:inspector
segment:tab
button:hdr-toggle
button:rotate-cw
button:trash
```

```ids-always
# 兩種畫面都要有
panel:titlebar
panel:sidebar
sidebar:scroll
button:rescan
field:add-path
button:add-folder
button:remove-folder
folder:
```

```ids-on-detail
# 只有在詳細檢視開著時存在；關閉時必須都不存在
panel:detail-strip
canvas:detail
button:detail-close
button:detail-zoom-in
button:detail-zoom-out
button:detail-1x
button:detail-fit
```

