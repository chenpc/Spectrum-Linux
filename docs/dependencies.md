# 建置與執行依賴

**UI 走 egui**（`crates/spectrum-app`）。`crates/spectrum-gtk` 保留在樹裡但**不是 workspace member**，預設不建置、不測試。

- 純 Rust 路徑（`spectrum-core`、`spectrum-testkit`、`spectrum-app`、`xtask`）**不需要任何 apt 套件**。
- Rust toolchain 裝在 repo 內：`.toolchain/rustup` 與 `.toolchain/cargo`（`gitignore` 排除）。用它：
  ```bash
  export RUSTUP_HOME="$PWD/.toolchain/rustup" CARGO_HOME="$PWD/.toolchain/cargo"
  export PATH="$CARGO_HOME/bin:$PATH"
  cargo xtask ci
  ```
  需要 `rustfmt`/`clippy`：`rustup component add rustfmt clippy`。
- 下面的 apt 清單只有在要把 `spectrum-gtk` 重新納回建置時才需要。

## 只有 `spectrum-gtk` 需要的系統套件

```bash
sudo apt install -y \
  build-essential \
  pkg-config \
  libgtk-4-dev \
  libglycin-2-dev \
  libglycin-gtk4-2-dev \
  libgraphene-1.0-dev \
  libseccomp-dev \
  liblcms2-dev \
  fontconfig \
  glycin-loaders \
  bubblewrap
```

| 套件 | 用途 | 本機版本 |
|---|---|---|
| `build-essential` | `cc`/`ar`，`system-deps` 需要 | 12.12 |
| `pkg-config` | gtk4-rs 與 glycin 都用 `system_deps::Config::new().probe()` | 已裝 |
| `libgtk-4-dev` | `gtk4.pc`（gtk4/gdk4/gsk4 crate 的 probe 目標） | 4.22.4 |
| `libglycin-2-dev` | `glycin-2.pc` + `glycin.h` | 2.1.5 |
| `libglycin-gtk4-2-dev` | `glycin-gtk4-2.pc` + dev `.so` symlink，`libglycin-gtk4-rebind-sys` 需要 | 2.1.5 |
| `libgraphene-1.0-dev` | `graphene-1.0.pc`（`graphene-rs` crate） | 1.10.8 |
| `libseccomp-dev` | `glycin-2.pc` 的 `Requires` | 2.6.0 |
| `liblcms2-dev` | `glycin-2.pc` 的 `Requires` | 2.17 |
| `fontconfig` | `glycin-2.pc` 的 `Requires` | 2.17.1 |
| `glycin-loaders` | 解碼器執行檔（`/usr/libexec/glycin-loaders`），sandbox 內跑 | 2.1.5 |
| `bubblewrap` | glycin 的 sandbox（`bwrap`），沒有它 `Loader::load()` 會失敗 | 0.11.1 |

## e2e / CI 需要

GTK4 沒有 headless GDK backend，`spectrum-gtk` 的 e2e test 需要一個虛擬顯示器：

- `libgtk-4-bin` → `gtk4-broadwayd`（`test.sh` 會自動啟動，`GDK_BACKEND=broadway`）
- `GTK_A11Y=test`、`GSK_RENDERER=cairo`（CPU 渲染器，快照跨 process 可重複）

不需要 Xvfb、weston 或 mutter。

## 關於 `PKG_CONFIG_PATH`

之前 `libglycin-gtk4-2-dev` 沒裝時，只有 runtime `libglycin-gtk4-2.so.0`，沒有 `.pc` 也沒有 dev `.so` symlink，所以要用 `/tmp/pc` 的 shim 並設 `PKG_CONFIG_PATH=/tmp/pc`。

裝了 `libglycin-gtk4-2-dev` 之後就不需要：

```
pkg-config --modversion glycin-gtk4-2   → 2.1.0
cargo clean -p libglycin-gtk4-rebind-sys && cargo build -p spectrum-gtk   → 成功（無環境變數）
```

注意 `cargo clean -p libglycin-gtk4-rebind-sys` 是必要的：build script 的舊快取會殘留 `-l ':libglycin-gtk4-2.so.0'`，導致 `error: library name must not be empty`。

## 版本限制

- `libglycin-rebind` 必須 pin 在 **0.1**：`libglycin-gtk4-rebind 0.1.0` 要求 `libglycin-rebind ^0.1.0`，兩份同時存在會讓 `gly::Frame` 型別不相容。
- graphene 要用 `graphene-rs`（crates.io 上的 `graphene` 是另一個 crate，版本是 0.1.x）。
- glycin 需要 GTK 4.16+（`glycin-gtk4-2.pc` 的 `Requires: gtk4 >= 4.16.0`）。
