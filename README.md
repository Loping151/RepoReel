<p align="center">
  <img src="assets/logo-small.png" width="120" alt="RepoReel logo">
</p>

# RepoReel

> Turn git history into a terminal-native time movie. Interactive playback, multi-repo timelines, deterministic GIF export, and self-contained HTML replay with optional music.

<p align="center">
  <img src="assets/hero.gif" width="720" alt="RepoReel hero GIF">
</p>

[English](#english) · [中文](#中文)

---

## English

RepoReel renders repository history as a "time movie": a directory tree grows over time, hot files are colored by relative activity, commits pulse from author anchors toward changed paths, and the same history can be replayed in the terminal, exported as a CI-stable GIF, or packaged as a shareable HTML file.

### Why

RepoReel is for teams that want repository evolution to be inspectable, reproducible, and easy to publish.

- Compared with Gource, RepoReel is terminal-native: no OpenGL, SDL, desktop session, or screen recording pipeline.
- It works over SSH and in headless CI, so the same tool can be used locally and in automation.
- `export` is deterministic for the same input history and render options, making generated hero GIFs reviewable and reproducible.
- The interactive player is not just passive playback: navigation mode lets you select file nodes and inspect the latest nearby diff.
- Every history command accepts repeated `--repo`, merging multiple repositories into one timeline and normalizing authors across repos.

### Install

Install directly from GitHub:

```bash
cargo install --git https://github.com/Loping151/reporeel
```

From source:

```bash
git clone https://github.com/Loping151/reporeel
cd reporeel
cargo install --path .
```

RepoReel uses Rust edition 2024. Use a current stable Rust toolchain.

### Quickstart

```bash
# Parse history and print a human-readable summary.
reporeel events --repo . --from v0.1.0 --to HEAD

# Play the same range interactively in the terminal.
reporeel play --repo . --from v0.1.0 --to HEAD

# Export a deterministic GIF suitable for README or release assets.
reporeel export --repo . --from v0.1.0 --to HEAD --out assets/hero.gif
```

Use `--repo` more than once to merge repositories:

```bash
reporeel play --repo ../api --repo ../web --repo ../docs --from main~200 --to main
```

### Commands

#### `reporeel events`

Parse git history and print a summary: top contributors, recent commits, active files, commit count, author count, file count, and time span. Add `--json` for the full machine-readable timeline.

```bash
reporeel events --repo <path> [--repo <path> ...] [--from <ref>] [--to <ref>] [--json]
```

Examples:

```bash
reporeel events --repo .
reporeel events --repo ../api --repo ../web --from v1.0.0 --to HEAD --json
```

#### `reporeel play`

Play the merged history in the terminal. The renderer adapts to terminal size and keeps the visualization terminal-native, so it works in SSH sessions without a graphics stack.

```bash
reporeel play --repo <path> [--repo <path> ...] [--from <ref>] [--to <ref>]
```

The scene is a directory tree, not a call graph. File and directory nodes persist as history advances; hot files use relative quantile coloring; recent commits draw pulse trails from author anchors to changed paths.

Navigation mode is entered with `Tab`. Move the cursor with arrow keys, press `Enter` on a file node to open its most recent commit diff near the node, then scroll the diff with `Up`/`Down`. Added lines are green and removed lines are red. `Esc` closes the diff or leaves navigation mode.

#### `reporeel export`

Render history to a deterministic GIF.

```bash
reporeel export \
  --repo <path> [--repo <path> ...] \
  [--from <ref>] [--to <ref>] \
  --out <file.gif> \
  [--fps <n>] [--width <cells>] [--height <cells>] [--max-frames <n>]
```

Defaults are `--fps 15`, `--width 120`, `--height 40`, and `--max-frames 90`. Each terminal cell is rendered as 8x16 pixels, so the default output is 960x640 pixels.

`export` is designed for CI. It reads the parsed git timeline and render options, with no wall-clock time or randomness. The same input produces byte-identical GIFs. Large histories are sampled automatically; for example, a 9k-commit repository can be reduced to a small hero GIF around 680KB instead of a frame-per-commit recording.

#### `reporeel demo`

Render a built-in synthetic project to a GIF for demos, docs, or marketing material.

```bash
reporeel demo --out demo.gif
```

#### `reporeel web`

Export a self-contained HTML replay. The output embeds the timeline data and player code, renders through Canvas and JavaScript, and can be opened directly in a browser or shared as a single file.

```bash
reporeel web \
  --repo <path> [--repo <path> ...] \
  [--from <ref>] [--to <ref>] \
  --out replay.html \
  [--music <audio-file>]
```

With `--music`, RepoReel base64-embeds the audio file into the HTML as background music. Keep audio files small when the output is meant to be shared.

#### `reporeel help`

Use built-in help for the command list or subcommand-specific flags:

```bash
reporeel help
reporeel help play
reporeel help export
```

### CI

Example GitHub Actions workflow that regenerates a README hero GIF on release tags:

```yaml
name: reporeel-hero

on:
  push:
    tags:
      - "v*"

jobs:
  hero:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install --git https://github.com/Loping151/reporeel
      - run: reporeel export --repo . --out assets/hero.gif --fps 15 --width 120 --height 40 --max-frames 90
      - uses: stefanzweifel/git-auto-commit-action@v5
        with:
          commit_message: "update RepoReel hero GIF"
          file_pattern: assets/hero.gif
```

For a multi-repo release visualization, check out sibling repositories and pass each path with `--repo`.

### Keybindings

| Key | Action |
| --- | --- |
| `Space` | Play or pause |
| `Left` / `Right` | Step one event backward or forward |
| `Up` / `Down` | Change speed |
| `<` / `>` | Small jump backward or forward |
| `PageUp` / `PageDown` | Large jump backward or forward |
| `f` | Focus next author |
| `a` | Expand or collapse the authors list |
| `Tab` | Enter or leave navigation mode |
| Navigation `Up` / `Down` / `Left` / `Right` | Move the node cursor |
| Navigation `Enter` | Select a node and open details or diff |
| Diff `Up` / `Down` | Scroll the open diff |
| Navigation `Esc` | Close diff, then leave navigation mode |
| `?` | Toggle help |
| `r` | Reset playback |
| `q` / `Esc` | Quit |
| `Ctrl+C` | Quit |

### Troubleshooting

- No commits are shown: make sure the checkout has history. In CI, use `fetch-depth: 0`.
- A ref range is empty: check that both refs exist in each repository passed with `--repo`.
- The terminal view is too dense: enlarge the terminal or export with larger `--width` and `--height`.
- The GIF is too large: reduce `--max-frames`, reduce `--fps`, or narrow the range with `--from` and `--to`.
- HTML with music is large: `--music` embeds audio as base64, which increases the HTML size. Use a short compressed audio file.
- Diff is not shown for a selected node: diff view is available for file nodes with a recent commit in the selected repository, not directory nodes.

---

## 中文

RepoReel 把仓库历史渲染成"时间电影":目录树随时间生长,热点文件按相对活跃度染色,commit 脉冲从作者锚点飞向改动路径。同一份历史可以在终端交互播放,导出为 CI 稳定的 GIF,也可以打包成可分享的自包含 HTML。

### 为什么

RepoReel 面向需要查看、复现、发布仓库演化过程的团队。

- 相比 Gource, RepoReel 是终端原生工具:不需要 OpenGL、SDL、桌面会话或录屏导出链路。
- 它能在 SSH 和 headless CI 中运行,本地查看和自动化发布用同一个工具。
- `export` 对同一份输入历史和渲染参数是确定性的,生成的 hero GIF 可复查、可复现。
- 交互播放不是单纯回放:导航模式可以选中文件节点,在节点附近查看最近 commit 的 diff。
- 所有历史命令都支持重复传入 `--repo`,把多个仓库合并成同一条时间线,并跨 repo 归一化作者。

### 安装

直接从 GitHub 安装:

```bash
cargo install --git https://github.com/Loping151/reporeel
```

从源码安装:

```bash
git clone https://github.com/Loping151/reporeel
cd reporeel
cargo install --path .
```

RepoReel 使用 Rust edition 2024。请使用当前 stable Rust 工具链。

### 快速开始

```bash
# 解析历史并输出人类可读概要。
reporeel events --repo . --from v0.1.0 --to HEAD

# 在终端交互播放同一段历史。
reporeel play --repo . --from v0.1.0 --to HEAD

# 导出适合 README 或发布素材的确定性 GIF。
reporeel export --repo . --from v0.1.0 --to HEAD --out assets/hero.gif
```

重复使用 `--repo` 可以合并多个仓库:

```bash
reporeel play --repo ../api --repo ../web --repo ../docs --from main~200 --to main
```

### 子命令

#### `reporeel events`

解析 git 历史并输出概要: top contributors、recent commits、active files、commit 数、作者数、文件数和时间跨度。添加 `--json` 可输出完整机器可读 timeline。

```bash
reporeel events --repo <path> [--repo <path> ...] [--from <ref>] [--to <ref>] [--json]
```

示例:

```bash
reporeel events --repo .
reporeel events --repo ../api --repo ../web --from v1.0.0 --to HEAD --json
```

#### `reporeel play`

在终端播放合并后的历史。渲染器会自适应终端尺寸,并保持终端原生,因此可在无图形栈的 SSH 会话中运行。

```bash
reporeel play --repo <path> [--repo <path> ...] [--from <ref>] [--to <ref>]
```

画面展示的是目录树,不是调用链。文件和目录节点会随历史推进持续存在;热点文件使用相对分位染色;最近 commit 会从作者锚点向改动路径绘制带方向尾迹的脉冲。

按 `Tab` 进入导航模式。用方向键移动光标,在文件节点上按 `Enter` 可在节点附近打开最近 commit 的 diff,再用 `Up`/`Down` 翻页。新增行为绿色,删除行为红色。`Esc` 关闭 diff 或退出导航模式。

#### `reporeel export`

把历史渲染成确定性 GIF。

```bash
reporeel export \
  --repo <path> [--repo <path> ...] \
  [--from <ref>] [--to <ref>] \
  --out <file.gif> \
  [--fps <n>] [--width <cells>] [--height <cells>] [--max-frames <n>]
```

默认参数为 `--fps 15`、`--width 120`、`--height 40`、`--max-frames 90`。每个终端 cell 渲染为 8x16 像素,因此默认输出为 960x640 像素。

`export` 面向 CI 设计。它只读取解析后的 git timeline 和渲染参数,不使用墙钟时间或随机数。同一份输入会产生字节级一致的 GIF。大历史会自动采样;例如 9k commit 的仓库可以压缩成约 680KB 的小型 hero GIF,而不是逐 commit 录制的大文件。

#### `reporeel demo`

把内置合成项目渲染成 GIF,用于 demo、文档或营销素材。

```bash
reporeel demo --out demo.gif
```

#### `reporeel web`

导出自包含 HTML replay。输出文件内嵌 timeline 数据和播放器代码,通过 Canvas 和 JavaScript 渲染,可直接用浏览器打开,也可作为单文件分享。

```bash
reporeel web \
  --repo <path> [--repo <path> ...] \
  [--from <ref>] [--to <ref>] \
  --out replay.html \
  [--music <audio-file>]
```

使用 `--music` 时,RepoReel 会把音频文件 base64 内嵌到 HTML 中作为背景音乐。用于分享时建议使用较小的音频文件。

#### `reporeel help`

查看命令列表或某个子命令的参数:

```bash
reporeel help
reporeel help play
reporeel help export
```

### CI

下面的 GitHub Actions 示例会在发布 tag 时重新生成 README hero GIF:

```yaml
name: reporeel-hero

on:
  push:
    tags:
      - "v*"

jobs:
  hero:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install --git https://github.com/Loping151/reporeel
      - run: reporeel export --repo . --out assets/hero.gif --fps 15 --width 120 --height 40 --max-frames 90
      - uses: stefanzweifel/git-auto-commit-action@v5
        with:
          commit_message: "update RepoReel hero GIF"
          file_pattern: assets/hero.gif
```

如果要生成多 repo 发布可视化,在 workflow 中 checkout 多个相邻仓库,再用多个 `--repo` 传入路径。

### 键位

| 按键 | 行为 |
| --- | --- |
| `Space` | 播放或暂停 |
| `Left` / `Right` | 向前或向后单步一个事件 |
| `Up` / `Down` | 调整播放速度 |
| `<` / `>` | 小跳前进或后退 |
| `PageUp` / `PageDown` | 大跳前进或后退 |
| `f` | 聚焦下一个作者 |
| `a` | 展开或收起 authors 列表 |
| `Tab` | 进入或退出导航模式 |
| 导航模式 `Up` / `Down` / `Left` / `Right` | 移动节点光标 |
| 导航模式 `Enter` | 选择节点并打开详情或 diff |
| Diff `Up` / `Down` | 翻动已打开的 diff |
| 导航模式 `Esc` | 关闭 diff,再退出导航模式 |
| `?` | 显示或隐藏帮助 |
| `r` | 重置播放 |
| `q` / `Esc` | 退出 |
| `Ctrl+C` | 退出 |

### 常见问题

- 没有显示 commit:确认 checkout 包含历史。CI 中请使用 `fetch-depth: 0`。
- ref 范围为空:确认每个通过 `--repo` 传入的仓库里都存在对应 refs。
- 终端画面太密:放大终端,或导出时提高 `--width` 和 `--height`。
- GIF 太大:降低 `--max-frames`、降低 `--fps`,或用 `--from` 和 `--to` 缩小范围。
- 带音乐的 HTML 太大:`--music` 会把音频 base64 内嵌进 HTML,文件会变大。请使用较短的压缩音频。
- 选中节点后没有 diff:diff 视图适用于选中仓库里有最近 commit 的文件节点,不适用于目录节点。

## License

MIT
