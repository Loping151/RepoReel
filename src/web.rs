use crate::ir::TimelineTrack;

pub fn render_replay_html(track: &TimelineTrack, audio_base64: Option<&str>) -> String {
    let timeline_json = serde_json::to_string(track)
        .expect("serializing TimelineTrack to embedded JSON should not fail")
        .replace("</", "<\\/");
    let audio_markup_len = audio_base64.map_or(0, str::len);
    let mut html = String::with_capacity(
        timeline_json.len()
            + audio_markup_len
            + HTML_PREFIX.len()
            + HTML_AFTER_MUSIC_BUTTON.len()
            + HTML_MUSIC_BUTTON.len()
            + HTML_AFTER_JSON.len()
            + HTML_SUFFIX.len(),
    );
    html.push_str(HTML_PREFIX);
    if audio_base64.is_some() {
        html.push_str(HTML_MUSIC_BUTTON);
    }
    html.push_str(HTML_AFTER_MUSIC_BUTTON);
    html.push_str(&timeline_json);
    html.push_str(HTML_AFTER_JSON);
    if let Some(audio_base64) = audio_base64 {
        html.push_str(r#"<audio id="bgmusic" preload="auto" src="data:audio/mpeg;base64,"#);
        html.push_str(audio_base64);
        html.push_str(
            r#""></audio>
"#,
        );
    }
    html.push_str(HTML_SUFFIX);
    html
}

const HTML_PREFIX: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>reporeel web replay</title>
<style>
:root {
  color-scheme: dark;
  --bg: #111316;
  --panel: #1a1f25;
  --panel-2: #222934;
  --text: #eef2f7;
  --muted: #a7b0bd;
  --line: #46515f;
  --line-active: #f2b84b;
  --node: #64748b;
  --node-dir: #38bdf8;
  --node-file: #94a3b8;
  --active: #f97316;
  --pulse: #22c55e;
}

* { box-sizing: border-box; }

html,
body {
  width: 100%;
  height: 100%;
  margin: 0;
  overflow: hidden;
  background: var(--bg);
  color: var(--text);
  font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

body {
  display: grid;
  grid-template-rows: auto 1fr auto;
}

header,
footer {
  padding: 12px 16px;
  background: var(--panel);
  border-color: #2f3742;
}

header {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 16px;
  align-items: center;
  border-bottom: 1px solid #2f3742;
}

h1 {
  margin: 0;
  font-size: 16px;
  font-weight: 700;
  letter-spacing: 0;
}

.meta,
.commit {
  color: var(--muted);
  font-size: 12px;
  line-height: 1.4;
}

.commit {
  margin-top: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stats {
  display: flex;
  gap: 14px;
  align-items: center;
  color: var(--muted);
  font-size: 12px;
}

main {
  min-height: 0;
  position: relative;
}

canvas {
  display: block;
  width: 100%;
  height: 100%;
  background: #0f1216;
}

footer {
  display: grid;
  grid-template-columns: auto minmax(120px, 1fr) auto auto;
  gap: 12px;
  align-items: center;
  border-top: 1px solid #2f3742;
}

button,
select {
  min-height: 34px;
  border: 1px solid #3a4452;
  border-radius: 6px;
  background: var(--panel-2);
  color: var(--text);
  font: inherit;
}

button {
  min-width: 92px;
  padding: 0 12px;
  cursor: pointer;
}

button:hover,
select:hover {
  border-color: #5b697a;
}

input[type="range"] {
  width: 100%;
  accent-color: var(--line-active);
}

.time {
  min-width: 108px;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
  font-size: 12px;
  text-align: right;
}

@media (max-width: 720px) {
  header {
    grid-template-columns: 1fr;
    gap: 8px;
  }

  .stats {
    flex-wrap: wrap;
  }

  footer {
    grid-template-columns: auto 1fr;
  }

  .time {
    text-align: left;
  }
}
</style>
</head>
<body>
<header>
  <div>
    <h1 id="title">reporeel</h1>
    <div class="meta" id="meta"></div>
    <div class="commit" id="commit"></div>
  </div>
  <div class="stats" id="stats"></div>
</header>
<main>
  <canvas id="stage"></canvas>
</main>
<footer>
  <button id="play" type="button">Play</button>
  <input id="scrub" type="range" min="0" value="0" step="1" aria-label="timeline">
  <select id="speed" aria-label="playback speed">
    <option value="0.5">0.5x</option>
    <option value="1" selected>1x</option>
    <option value="2">2x</option>
    <option value="4">4x</option>
  </select>
"##;

const HTML_MUSIC_BUTTON: &str = r##"  <button id="music" type="button">Music on</button>
"##;

const HTML_AFTER_MUSIC_BUTTON: &str = r##"  <div class="time" id="time">0 / 0</div>
</footer>
<script>
const TIMELINE = "##;

const HTML_AFTER_JSON: &str = r##";

</script>
"##;

const HTML_SUFFIX: &str = r##"<script>
const events = (TIMELINE.git_events || []).slice().sort((a, b) => {
  return Date.parse(a.author_time) - Date.parse(b.author_time);
});
const canvas = document.getElementById("stage");
const ctx = canvas.getContext("2d");
const playButton = document.getElementById("play");
const musicButton = document.getElementById("music");
const bgMusic = document.getElementById("bgmusic");
const scrub = document.getElementById("scrub");
const speed = document.getElementById("speed");
const title = document.getElementById("title");
const meta = document.getElementById("meta");
const commit = document.getElementById("commit");
const stats = document.getElementById("stats");
const time = document.getElementById("time");
const ACTIVE_EVENT_COUNT = 4;
const STEP_MS = 180;
let cursor = 0;
let playing = false;
let lastFrame = 0;
let progress = 0;
let musicEnabled = Boolean(bgMusic);

const tree = buildTree(events);
scrub.max = String(events.length);
title.textContent = repoTitle(events);
meta.textContent = spanText(events);
stats.textContent = `${events.length} commits  |  ${tree.files} files  |  ${tree.authors} authors`;

function repoTitle(items) {
  const repos = new Set(items.map((event) => event.repo).filter(Boolean));
  if (repos.size === 0) return "reporeel";
  if (repos.size === 1) return Array.from(repos)[0];
  return `${repos.size} repositories`;
}

function spanText(items) {
  if (items.length === 0) return "No commits in this timeline";
  const first = formatDate(items[0].author_time);
  const last = formatDate(items[items.length - 1].author_time);
  return first === last ? first : `${first} to ${last}`;
}

function formatDate(value) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "2-digit" });
}

function buildTree(items) {
  const nodes = new Map();
  const authors = new Set();

  function ensure(path, isDir) {
    if (!path) return null;
    const existing = nodes.get(path);
    if (existing) {
      existing.isDir = existing.isDir || isDir;
      return existing;
    }
    const parts = path.split("/");
    const parent = parts.length > 1 ? parts.slice(0, -1).join("/") : "";
    const node = {
      path,
      name: parts[parts.length - 1],
      parent,
      depth: parts.length - 1,
      row: 0,
      heat: 0,
      isDir,
      children: [],
      x: 0,
      y: 0
    };
    nodes.set(path, node);
    return node;
  }

  for (const event of items) {
    if (event.author && event.author.email) authors.add(event.author.email);
    for (const change of event.changes || []) {
      const normalized = normalizePath(change.path);
      if (!normalized) continue;
      const parts = normalized.split("/");
      for (let index = 0; index < parts.length; index += 1) {
        const path = parts.slice(0, index + 1).join("/");
        const isDir = index + 1 < parts.length;
        const node = ensure(path, isDir);
        node.heat += 1;
      }
    }
  }

  for (const node of nodes.values()) {
    if (!node.parent) continue;
    const parent = nodes.get(node.parent);
    if (parent) parent.children.push(node);
  }

  const ordered = Array.from(nodes.values()).sort((a, b) => a.path.localeCompare(b.path));
  let row = 0;
  const roots = ordered.filter((node) => !node.parent || !nodes.has(node.parent));
  for (const root of roots) row = assignRows(root, row);

  return {
    nodes: ordered,
    byPath: nodes,
    roots,
    files: ordered.filter((node) => !node.isDir).length,
    authors: authors.size,
    maxHeat: ordered.reduce((max, node) => Math.max(max, node.heat), 1)
  };
}

function assignRows(node, row) {
  node.row = row;
  let next = row + 1;
  node.children.sort((a, b) => a.path.localeCompare(b.path));
  for (const child of node.children) next = assignRows(child, next);
  return next;
}

function normalizePath(value) {
  return String(value || "")
    .replace(/\\/g, "/")
    .split("/")
    .filter((part) => part && part !== ".")
    .join("/");
}

function resizeCanvas() {
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.floor(rect.width * dpr));
  const height = Math.max(1, Math.floor(rect.height * dpr));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
}

function updatePlayback(now) {
  if (!lastFrame) lastFrame = now;
  const elapsed = now - lastFrame;
  lastFrame = now;

  if (playing && events.length > 0) {
    progress += (elapsed / STEP_MS) * Number(speed.value);
    while (progress >= 1 && cursor < events.length) {
      cursor += 1;
      progress -= 1;
    }
    if (cursor >= events.length) {
      cursor = events.length;
      playing = false;
      progress = 0;
      syncAudio();
    }
    scrub.value = String(cursor);
  }

  draw();
  requestAnimationFrame(updatePlayback);
}

function activeState() {
  const visible = new Set();
  const active = new Set();
  const recent = events.slice(Math.max(0, cursor - ACTIVE_EVENT_COUNT), cursor);

  for (const event of events.slice(0, cursor)) {
    for (const change of event.changes || []) addPathAndParents(visible, normalizePath(change.path));
  }
  for (const event of recent) {
    for (const change of event.changes || []) addPathAndParents(active, normalizePath(change.path));
  }
  return { visible, active };
}

function addPathAndParents(set, path) {
  if (!path) return;
  const parts = path.split("/");
  for (let index = 0; index < parts.length; index += 1) {
    set.add(parts.slice(0, index + 1).join("/"));
  }
}

function draw() {
  resizeCanvas();
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = "#0f1216";
  ctx.fillRect(0, 0, width, height);

  const margin = { left: 28, right: 28, top: 24, bottom: 24 };
  const usableWidth = Math.max(1, width - margin.left - margin.right);
  const usableHeight = Math.max(1, height - margin.top - margin.bottom);
  const maxDepth = tree.nodes.reduce((max, node) => Math.max(max, node.depth), 0);
  const rowCount = Math.max(1, tree.nodes.length - 1);
  const state = activeState();

  for (const node of tree.nodes) {
    node.x = margin.left + (maxDepth === 0 ? 0.5 : node.depth / maxDepth) * usableWidth;
    node.y = margin.top + (node.row / rowCount) * usableHeight;
  }

  drawLinks(state);
  drawNodes(state);
  drawPulse(state, performance.now());
  drawEmptyState(width, height);
  updateLabels();
}

function drawLinks(state) {
  ctx.lineWidth = 1;
  for (const node of tree.nodes) {
    if (!node.parent) continue;
    const parent = tree.byPath.get(node.parent);
    if (!parent) continue;
    const visible = state.visible.has(node.path);
    const active = state.active.has(node.path);
    ctx.strokeStyle = active ? "#f2b84b" : visible ? "#46515f" : "#242a33";
    ctx.globalAlpha = active ? 0.95 : visible ? 0.72 : 0.26;
    ctx.beginPath();
    ctx.moveTo(parent.x, parent.y);
    ctx.lineTo(node.x, node.y);
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
}

function drawNodes(state) {
  for (const node of tree.nodes) {
    const visible = state.visible.has(node.path);
    const active = state.active.has(node.path);
    const heat = Math.min(1, node.heat / tree.maxHeat);
    const size = node.isDir ? 10 : 8;
    const alpha = active ? 1 : visible ? 0.78 : 0.18;
    const color = active ? "#f97316" : node.isDir ? "#38bdf8" : blend("#64748b", "#e2e8f0", heat);
    ctx.globalAlpha = alpha;
    ctx.fillStyle = color;
    ctx.fillRect(node.x - size / 2, node.y - size / 2, size, size);
    if (active) {
      ctx.strokeStyle = "#fff7ed";
      ctx.lineWidth = 2;
      ctx.strokeRect(node.x - size / 2 - 2, node.y - size / 2 - 2, size + 4, size + 4);
    }
  }
  ctx.globalAlpha = 1;
}

function drawPulse(state, now) {
  const phase = (now % 900) / 900;
  for (const node of tree.nodes) {
    if (!state.active.has(node.path)) continue;
    const radius = 9 + phase * 18;
    ctx.globalAlpha = 1 - phase;
    ctx.strokeStyle = "#22c55e";
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(node.x, node.y, radius, 0, Math.PI * 2);
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
}

function drawEmptyState(width, height) {
  if (events.length !== 0) return;
  ctx.fillStyle = "#a7b0bd";
  ctx.font = "14px ui-sans-serif, system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.fillText("No commits to replay", width / 2, height / 2);
}

function blend(from, to, amount) {
  const left = hex(from);
  const right = hex(to);
  const mixed = left.map((value, index) => Math.round(value + (right[index] - value) * amount));
  return `rgb(${mixed[0]}, ${mixed[1]}, ${mixed[2]})`;
}

function hex(value) {
  const clean = value.replace("#", "");
  return [0, 2, 4].map((index) => parseInt(clean.slice(index, index + 2), 16));
}

function updateLabels() {
  const current = events[Math.max(0, cursor - 1)];
  playButton.textContent = playing ? "Pause" : "Play";
  if (musicButton) musicButton.textContent = musicEnabled ? "Music on" : "Music off";
  time.textContent = `${cursor} / ${events.length}`;
  if (!current) {
    commit.textContent = "";
    return;
  }
  const author = current.author && current.author.name ? current.author.name : "unknown";
  const oid = String(current.commit_oid || "").slice(0, 7);
  const message = String(current.message || "").split("\n")[0];
  commit.textContent = `${oid}  ${formatDate(current.author_time)}  ${author}  ${message}`;
}

function syncAudio() {
  if (!bgMusic) return;
  const targetTime = Math.max(0, cursor * STEP_MS / 1000);
  bgMusic.playbackRate = Number(speed.value);
  if (Number.isFinite(bgMusic.duration) && targetTime < bgMusic.duration) {
    bgMusic.currentTime = targetTime;
  }
  if (!playing || !musicEnabled) {
    bgMusic.pause();
    return;
  }
  const playPromise = bgMusic.play();
  if (playPromise && typeof playPromise.catch === "function") {
    playPromise.catch(() => {
      musicEnabled = false;
      bgMusic.pause();
      updateLabels();
    });
  }
}

playButton.addEventListener("click", () => {
  if (cursor >= events.length) cursor = 0;
  playing = !playing;
  progress = 0;
  lastFrame = 0;
  scrub.value = String(cursor);
  syncAudio();
  draw();
});

scrub.addEventListener("input", () => {
  cursor = Number(scrub.value);
  playing = false;
  progress = 0;
  syncAudio();
  draw();
});

speed.addEventListener("change", () => {
  progress = 0;
  syncAudio();
});

if (musicButton) {
  musicButton.addEventListener("click", () => {
    musicEnabled = !musicEnabled;
    syncAudio();
    draw();
  });
}

window.addEventListener("resize", draw);
draw();
requestAnimationFrame(updatePlayback);
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::ir::{Author, ChangeKind, FileChange, HistoryEvent, RepoId};

    fn fixture_track(message: &str) -> TimelineTrack {
        TimelineTrack::from_git_events(vec![HistoryEvent {
            repo: RepoId::new("fixture"),
            commit_oid: "abcdef123456".to_string(),
            author: Author::normalized("Ada", "ada@example.com"),
            author_time: Utc.timestamp_opt(10, 0).single().unwrap(),
            commit_time: Utc.timestamp_opt(10, 0).single().unwrap(),
            changes: vec![FileChange {
                path: PathBuf::from("src/main.rs"),
                kind: ChangeKind::Modify,
                lines_added: 4,
                lines_deleted: 1,
            }],
            message: message.to_string(),
            tags: Vec::new(),
        }])
    }

    #[test]
    fn render_replay_html_embeds_canvas_timeline_and_controls() {
        let html = render_replay_html(&fixture_track("add player"), None);

        assert!(html.contains("<canvas id=\"stage\"></canvas>"));
        assert!(html.contains("const TIMELINE = {"));
        assert!(html.contains("playButton.addEventListener(\"click\""));
        assert!(html.contains("requestAnimationFrame(updatePlayback)"));
        assert!(!html.contains("<audio id=\"bgmusic\""));
    }

    #[test]
    fn render_replay_html_escapes_script_closing_sequence_in_json() {
        let html = render_replay_html(&fixture_track("</script><p>breakout</p>"), None);

        assert!(!html.contains("</script><p>breakout</p>"));
        assert!(html.contains("<\\/script><p>breakout<\\/p>"));
    }

    #[test]
    fn render_replay_html_embeds_audio_and_music_controls_when_provided() {
        let html = render_replay_html(&fixture_track("add player"), Some("UklGRg=="));

        assert!(html.contains(
            "<audio id=\"bgmusic\" preload=\"auto\" src=\"data:audio/mpeg;base64,UklGRg==\""
        ));
        assert!(html.contains("<button id=\"music\" type=\"button\">Music on</button>"));
        assert!(html.contains("bgMusic.play()"));
        assert!(html.contains("bgMusic.pause()"));
        assert!(html.contains("musicButton.addEventListener(\"click\""));
    }
}
