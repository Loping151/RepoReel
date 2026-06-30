use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Widget,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{diff::FileDiff, ir::HistoryEvent, layout::Layout};

const DIFF_POPUP_WIDTH: u16 = 40;
const DIFF_POPUP_HEIGHT: u16 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct Pulse {
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub progress: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorInfo {
    pub key: String,
    pub label: String,
    pub commits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotFile {
    pub path: PathBuf,
    pub heat: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentCommitInfo {
    pub date: String,
    pub author: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContributorInfo {
    pub key: String,
    pub label: String,
    pub commits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeDetail {
    pub path: PathBuf,
    pub is_dir: bool,
    pub repo_path: Option<PathBuf>,
    pub heat: u32,
    pub child_file_count: usize,
    pub total_changes: u32,
    pub recent_commit: Option<RecentCommitInfo>,
    pub contributors: Vec<ContributorInfo>,
    pub diff: Option<FileDiff>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneState {
    pub layout: Layout,
    pub repo_title: String,
    pub span_text: String,
    pub focus: Option<String>,
    pub visible_paths: HashSet<PathBuf>,
    pub active_paths: HashSet<PathBuf>,
    pub hot_files: Vec<HotFile>,
    pub authors: Vec<AuthorInfo>,
    pub pulses: Vec<Pulse>,
    pub now: Option<DateTime<Utc>>,
    pub cursor: usize,
    pub total: usize,
    pub speed: String,
    pub paused: bool,
    pub show_help: bool,
    pub show_authors: bool,
    pub cursor_path: Option<PathBuf>,
    pub nav_mode: bool,
    pub diff_open: bool,
    pub diff_scroll: usize,
    pub author_scroll: usize,
    pub node_details: BTreeMap<PathBuf, NodeDetail>,
}

pub fn render_scene(frame: &mut ratatui::Frame, area: Rect, scene: &SceneState) {
    frame.render_widget(SceneWidget { scene }, area);
}

pub fn author_infos_from_events(events: &[HistoryEvent]) -> Vec<AuthorInfo> {
    let mut authors = BTreeMap::<String, AuthorInfo>::new();
    for event in events {
        let key = event.author.identity_key().to_string();
        authors
            .entry(key.clone())
            .and_modify(|author| author.commits = author.commits.saturating_add(1))
            .or_insert_with(|| AuthorInfo {
                key,
                label: author_label(&event.author.name, event.author.identity_key()),
                commits: 1,
            });
    }
    authors.into_values().collect()
}

pub fn repo_title_from_events(events: &[HistoryEvent]) -> String {
    let mut repos = BTreeMap::<String, String>::new();
    for event in events {
        let repo = event.repo.as_str().trim();
        if repo.is_empty() {
            continue;
        }
        repos
            .entry(repo.to_string())
            .or_insert_with(|| repo_display_name(repo));
    }

    match repos.len() {
        0 => "unknown".to_string(),
        1 => repos
            .into_values()
            .next()
            .unwrap_or_else(|| "unknown".to_string()),
        2 | 3 => repos.into_values().collect::<Vec<_>>().join(" + "),
        count => format!("{count} repos"),
    }
}

fn repo_display_name(repo: &str) -> String {
    Path::new(repo)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(repo)
        .to_string()
}

pub fn span_text_from_events(events: &[HistoryEvent]) -> String {
    let Some((first, rest)) = events.split_first() else {
        return "empty · 0 commits".to_string();
    };

    let (start, end) = rest.iter().fold(
        (first.author_time, first.author_time),
        |(start, end), event| (start.min(event.author_time), end.max(event.author_time)),
    );
    let commits = if events.len() == 1 {
        "1 commit".to_string()
    } else {
        format!("{} commits", events.len())
    };

    format!(
        "{} to {} · {}",
        start.format("%Y-%m"),
        end.format("%Y-%m"),
        commits
    )
}

pub fn build_pulses(
    events: &[HistoryEvent],
    layout: &Layout,
    authors: &[AuthorInfo],
    recent_event_count: usize,
) -> Vec<Pulse> {
    if recent_event_count == 0 || authors.is_empty() {
        return Vec::new();
    }

    let author_index = authors
        .iter()
        .enumerate()
        .map(|(index, author)| (author.key.as_str(), index))
        .collect::<HashMap<_, _>>();

    events
        .iter()
        .rev()
        .take(recent_event_count)
        .enumerate()
        .flat_map(|(age, event)| {
            let from = author_index
                .get(event.author.identity_key())
                .map(|index| author_anchor_position(*index, authors.len()))
                .unwrap_or((0.5, 0.03));
            let progress = (0.16 + age as f64 * 0.18).clamp(0.0, 0.96);
            event.changes.iter().filter_map(move |change| {
                let node = pulse_target_node(layout, &change.path)?;
                Some(Pulse {
                    from,
                    to: (node.x, node.y),
                    progress,
                })
            })
        })
        .collect()
}

pub fn hot_files_from_events(
    events: &[HistoryEvent],
    focus_author: Option<&str>,
    limit: usize,
) -> Vec<HotFile> {
    if limit == 0 {
        return Vec::new();
    }

    let mut heat_by_path = BTreeMap::<PathBuf, u32>::new();
    for event in events {
        if focus_author.is_some_and(|author| event.author.identity_key() != author) {
            continue;
        }
        for change in &event.changes {
            *heat_by_path.entry(change.path.clone()).or_default() += 1;
        }
    }

    let mut files = heat_by_path
        .into_iter()
        .map(|(path, heat)| HotFile { path, heat })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        right
            .heat
            .cmp(&left.heat)
            .then_with(|| left.path.cmp(&right.path))
    });
    files.truncate(limit);
    files
}

pub fn node_details_from_events(
    events: &[HistoryEvent],
    layout: &Layout,
) -> BTreeMap<PathBuf, NodeDetail> {
    let mut details = layout
        .nodes
        .iter()
        .map(|node| {
            (
                node.path.clone(),
                NodeDetail {
                    path: node.path.clone(),
                    is_dir: node.is_dir,
                    heat: node.heat,
                    child_file_count: if node.is_dir {
                        layout
                            .nodes
                            .iter()
                            .filter(|candidate| {
                                !candidate.is_dir
                                    && candidate.path != node.path
                                    && candidate.path.starts_with(&node.path)
                            })
                            .count()
                    } else {
                        0
                    },
                    repo_path: None,
                    total_changes: 0,
                    recent_commit: None,
                    contributors: Vec::new(),
                    diff: None,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut contributor_counts = BTreeMap::<PathBuf, BTreeMap<String, ContributorInfo>>::new();
    let mut recent_times = BTreeMap::<PathBuf, DateTime<Utc>>::new();

    for event in events {
        let mut touched_paths = HashSet::<PathBuf>::new();
        for change in &event.changes {
            for path in path_with_ancestors(&change.path) {
                if let Some(detail) = details.get_mut(&path) {
                    detail.total_changes = detail.total_changes.saturating_add(1);
                    touched_paths.insert(path);
                }
            }
        }

        for path in touched_paths {
            let author_key = event.author.identity_key().to_string();
            contributor_counts
                .entry(path.clone())
                .or_default()
                .entry(author_key.clone())
                .and_modify(|contributor| {
                    contributor.commits = contributor.commits.saturating_add(1);
                })
                .or_insert_with(|| ContributorInfo {
                    key: author_key,
                    label: author_label(&event.author.name, event.author.identity_key()),
                    commits: 1,
                });

            if let Some(detail) = details.get_mut(&path) {
                let should_replace = recent_times
                    .get(&path)
                    .is_none_or(|recent_time| *recent_time <= event.author_time);
                if should_replace {
                    recent_times.insert(path.clone(), event.author_time);
                    detail.repo_path = Some(PathBuf::from(event.repo.as_str()));
                    detail.recent_commit = Some(RecentCommitInfo {
                        date: event.author_time.format("%Y-%m-%d").to_string(),
                        author: author_label(&event.author.name, event.author.identity_key()),
                        message: event
                            .message
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    });
                }
            }
        }
    }

    for (path, contributors) in contributor_counts {
        if let Some(detail) = details.get_mut(&path) {
            let mut contributors = contributors.into_values().collect::<Vec<_>>();
            contributors.sort_by(|left, right| {
                right
                    .commits
                    .cmp(&left.commits)
                    .then_with(|| left.label.cmp(&right.label))
            });
            detail.contributors = contributors;
        }
    }

    details
}

pub fn draw_scene_to_buffer(scene: &SceneState, area: Rect, buf: &mut Buffer) {
    clear_area(buf, area);

    let top_height = if area.height > 0 { 1 } else { 0 };
    let bottom_height = if area.height >= 10 {
        4
    } else if area.height >= 7 {
        3
    } else {
        0
    };
    let tree_height = area.height.saturating_sub(top_height + bottom_height);

    if top_height > 0 {
        let top_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: top_height,
        };
        draw_top_title(scene, top_area, buf);
    }

    let side_width = side_width(area.width, tree_height);
    let tree_width = area.width.saturating_sub(side_width.unwrap_or(0));
    let tree_area = Rect {
        x: area.x,
        y: area.y.saturating_add(top_height),
        width: tree_width,
        height: tree_height,
    };
    draw_tree(scene, tree_area, buf);

    if let Some(side_width) = side_width {
        let side_area = Rect {
            x: area.x.saturating_add(area.width.saturating_sub(side_width)),
            y: tree_area.y,
            width: side_width,
            height: tree_height,
        };
        draw_side_overlay(scene, side_area, buf);
    }

    if bottom_height > 0 {
        let bottom_area = Rect {
            x: area.x,
            y: area
                .y
                .saturating_add(top_height)
                .saturating_add(tree_height),
            width: area.width,
            height: bottom_height,
        };
        draw_bottom_overlay(scene, bottom_area, buf);
    }

    if scene.show_help {
        draw_help_overlay(area, buf);
    }
}

fn draw_top_title(scene: &SceneState, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // The title line doubles as the "what am I looking at" hint: repo name plus
    // a one-phrase explanation, so a first-time viewer immediately gets the metaphor.
    let title = format!(
        "{}  (file/dir tree · not call graph · color = edit frequency)",
        scene.repo_title
    );

    let repo_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let span_style = Style::default().fg(Color::DarkGray);
    let span_width = UnicodeWidthStr::width(scene.span_text.as_str());
    let title_width = UnicodeWidthStr::width(title.as_str());
    let area_width = usize::from(area.width);

    let left_width = if span_width + 2 < area_width {
        area_width - span_width - 2
    } else {
        area_width
    };
    put_text(
        buf,
        area.x,
        area.y,
        &title,
        u16::try_from(left_width).unwrap_or(area.width),
        repo_style,
    );

    if span_width < area_width && title_width + span_width + 2 <= area_width {
        let span_x = area.x.saturating_add(
            area.width
                .saturating_sub(u16::try_from(span_width).unwrap_or(0)),
        );
        put_text(
            buf,
            span_x,
            area.y,
            &scene.span_text,
            u16::try_from(span_width).unwrap_or(area.width),
            span_style,
        );
    }
}

struct SceneWidget<'a> {
    scene: &'a SceneState,
}

impl Widget for SceneWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        draw_scene_to_buffer(self.scene, area, buf);
    }
}

fn draw_tree(scene: &SceneState, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if scene.layout.nodes.is_empty() {
        put_text(
            buf,
            area.x,
            area.y,
            "(empty history)",
            area.width,
            Style::default().fg(Color::DarkGray),
        );
        return;
    }

    // Before playback starts (cursor 0, nothing revealed yet), tell the user what to do
    // instead of showing a confusing blank canvas.
    if scene.visible_paths.is_empty() && scene.paused {
        let hint = "press space to play · ←→ to step · ? for help";
        let hint_width = u16::try_from(UnicodeWidthStr::width(hint)).unwrap_or(area.width);
        let x = area
            .x
            .saturating_add(area.width.saturating_sub(hint_width) / 2);
        let y = area.y.saturating_add(area.height / 2);
        put_text(
            buf,
            x,
            y,
            hint,
            area.width,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        return;
    }

    let graph = render_graph(scene);
    let heat_scale = HeatScale::from_graph(scene, &graph);
    let y_projection = VisibleYProjection::from_graph(scene, &graph);
    draw_links(scene, area, buf, &graph, &heat_scale, y_projection);
    draw_nodes(scene, area, buf, &graph, &heat_scale, y_projection);
    draw_pulses(scene, area, buf, y_projection);
    draw_nav_cursor(scene, area, buf, y_projection);
    draw_diff_popup(scene, area, buf, y_projection);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderGraph {
    visible_dirs: HashSet<usize>,
    visible_files: HashSet<usize>,
    expanded_dirs: HashSet<usize>,
    active_dirs: HashSet<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct VisibleYProjection {
    min: f64,
    max: f64,
}

impl VisibleYProjection {
    fn from_graph(scene: &SceneState, graph: &RenderGraph) -> Self {
        let (min, max) = graph
            .visible_dirs
            .iter()
            .chain(graph.visible_files.iter())
            .filter_map(|index| scene.layout.nodes.get(*index).map(|node| node.y))
            .fold(None::<(f64, f64)>, |range, y| {
                Some(match range {
                    Some((min, max)) => (min.min(y), max.max(y)),
                    None => (y, y),
                })
            })
            .unwrap_or((0.0, 1.0));
        Self { min, max }
    }

    fn normalize(self, y: f64) -> f64 {
        let span = self.max - self.min;
        if span.abs() <= f64::EPSILON {
            0.5
        } else {
            ((y - self.min) / span).clamp(0.0, 1.0)
        }
    }
}

fn render_graph(scene: &SceneState) -> RenderGraph {
    const EXPANDED_DIR_LIMIT: usize = 8;

    if scene.nav_mode {
        let visible_dirs = scene
            .layout
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| node.is_dir.then_some(index))
            .collect::<HashSet<_>>();
        let visible_files = scene
            .layout
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| (!node.is_dir).then_some(index))
            .collect::<HashSet<_>>();
        return RenderGraph {
            expanded_dirs: visible_dirs.clone(),
            visible_dirs,
            visible_files,
            active_dirs: HashSet::new(),
        };
    }

    let mut visible_dirs = HashSet::new();
    let mut active_dirs = HashSet::new();
    for path in scene.visible_paths.iter().chain(scene.active_paths.iter()) {
        let Some(index) = scene.layout.by_path.get(path).copied() else {
            continue;
        };

        if scene.layout.nodes[index].is_dir {
            visible_dirs.insert(index);
        }
        let mut parent = parent_path(path);
        while let Some(path) = parent {
            if let Some(parent_index) = scene.layout.by_path.get(&path).copied()
                && scene.layout.nodes[parent_index].is_dir
            {
                visible_dirs.insert(parent_index);
            }
            parent = parent_path(&path);
        }
    }

    for path in &scene.active_paths {
        let mut parent = Some(path.as_path());
        while let Some(path) = parent {
            if let Some(index) = scene.layout.by_path.get(path).copied()
                && scene.layout.nodes[index].is_dir
            {
                active_dirs.insert(index);
            }
            parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty());
        }
    }

    let mut hot_dirs = visible_dirs.iter().copied().collect::<Vec<_>>();
    hot_dirs.sort_by(|left, right| {
        let left_node = &scene.layout.nodes[*left];
        let right_node = &scene.layout.nodes[*right];
        right_node
            .heat
            .cmp(&left_node.heat)
            .then_with(|| left_node.path.cmp(&right_node.path))
    });
    hot_dirs.truncate(EXPANDED_DIR_LIMIT);
    let expanded_dirs = hot_dirs.into_iter().collect::<HashSet<_>>();

    let mut visible_files = HashSet::new();
    for path in scene.visible_paths.iter().chain(scene.active_paths.iter()) {
        let Some(index) = scene.layout.by_path.get(path).copied() else {
            continue;
        };
        if scene.layout.nodes[index].is_dir {
            continue;
        }
        if let Some(parent) = parent_path(path)
            && let Some(parent_index) = scene.layout.by_path.get(&parent).copied()
            && expanded_dirs.contains(&parent_index)
        {
            visible_files.insert(index);
        }
    }

    RenderGraph {
        visible_dirs,
        visible_files,
        expanded_dirs,
        active_dirs,
    }
}

fn draw_links(
    scene: &SceneState,
    area: Rect,
    buf: &mut Buffer,
    graph: &RenderGraph,
    heat_scale: &HeatScale,
    y_projection: VisibleYProjection,
) {
    draw_virtual_root(scene, area, buf, graph, heat_scale, y_projection);
    for (parent_index, parent) in scene.layout.nodes.iter().enumerate() {
        if !graph.visible_dirs.contains(&parent_index) {
            continue;
        }
        let Some(from) = map_point(area, parent.x, y_projection.normalize(parent.y)) else {
            continue;
        };
        for child_index in &parent.children {
            let child = &scene.layout.nodes[*child_index];
            let child_visible = if child.is_dir {
                graph.visible_dirs.contains(child_index)
            } else {
                graph.visible_files.contains(child_index)
            };
            if !child_visible {
                continue;
            }
            let Some(to) = map_point(area, child.x, y_projection.normalize(child.y)) else {
                continue;
            };
            draw_line(
                buf,
                from,
                to,
                Style::default().fg(dim_heat_color(heat_scale.band(parent.heat))),
            );
        }
    }
}

fn draw_virtual_root(
    scene: &SceneState,
    area: Rect,
    buf: &mut Buffer,
    graph: &RenderGraph,
    heat_scale: &HeatScale,
    y_projection: VisibleYProjection,
) {
    let roots = scene
        .layout
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, node)| {
            node.depth == 0 && node.is_dir && graph.visible_dirs.contains(index)
        })
        .collect::<Vec<_>>();
    if roots.len() <= 1 {
        return;
    }

    let Some(center) = map_point(area, 0.5, 0.54) else {
        return;
    };
    for (_, root) in roots {
        if let Some(to) = map_point(area, root.x, y_projection.normalize(root.y)) {
            draw_line(
                buf,
                center,
                to,
                Style::default().fg(dim_heat_color(heat_scale.band(root.heat))),
            );
        }
    }
    put_symbol(
        buf,
        center.0,
        center.1,
        "█",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
}

fn draw_nodes(
    scene: &SceneState,
    area: Rect,
    buf: &mut Buffer,
    graph: &RenderGraph,
    heat_scale: &HeatScale,
    y_projection: VisibleYProjection,
) {
    let mut indices = graph
        .visible_dirs
        .iter()
        .chain(graph.visible_files.iter())
        .copied()
        .collect::<Vec<_>>();
    indices.sort_by(|left, right| {
        scene.layout.nodes[*left]
            .y
            .total_cmp(&scene.layout.nodes[*right].y)
            .then_with(|| {
                scene.layout.nodes[*left]
                    .row
                    .cmp(&scene.layout.nodes[*right].row)
            })
    });

    for index in indices {
        let node = &scene.layout.nodes[index];
        let Some((x, y)) = map_point(area, node.x, y_projection.normalize(node.y)) else {
            continue;
        };
        let heat_band = heat_scale.band(node.heat);
        if node.is_dir {
            let is_active = graph.active_dirs.contains(&index);
            draw_dir_node(buf, area, x, y, heat_band, is_active);
        } else {
            let is_active = scene.active_paths.contains(&node.path);
            draw_file_node(buf, x, y, heat_band, is_active);
        }
    }
}

fn draw_pulses(scene: &SceneState, area: Rect, buf: &mut Buffer, y_projection: VisibleYProjection) {
    for pulse in &scene.pulses {
        let progress = pulse.progress.clamp(0.0, 1.0);
        let Some(from) = map_point(area, pulse.from.0, pulse.from.1) else {
            continue;
        };
        // Interpolate after mapping to cells so pulses follow the exact rendered node path.
        let Some(to) = map_point(area, pulse.to.0, y_projection.normalize(pulse.to.1)) else {
            continue;
        };

        let fx = f64::from(from.0);
        let fy = f64::from(from.1);
        let tx = f64::from(to.0);
        let ty = f64::from(to.1);
        let head_x = fx + (tx - fx) * progress;
        let head_y = fy + (ty - fy) * progress;

        // Trail: a few fading dots trailing behind the head, opposite to travel direction.
        // Visible when paused so the user can tell which way the commit is flying.
        let dx = tx - fx;
        let dy = ty - fy;
        let trail_len = dx.hypot(dy).min(6.0);
        let trail = ["░", "▒", "·"];
        for (i, sym) in trail.iter().enumerate() {
            let back = (1.0 + i as f64) * 1.5;
            let t = (progress - back / trail_len.max(1.0)).max(0.0);
            if t > 1.0 {
                continue;
            }
            let txp = fx + dx * t;
            let typ = fy + dy * t;
            let cx = txp.round() as i32;
            let cy = typ.round() as i32;
            if cx >= i32::from(area.x)
                && cx < i32::from(area.x) + i32::from(area.width)
                && cy >= i32::from(area.y)
                && cy < i32::from(area.y) + i32::from(area.height)
            {
                put_symbol(
                    buf,
                    cx as u16,
                    cy as u16,
                    sym,
                    Style::default().fg(Color::Yellow),
                );
            }
        }

        let hx = head_x.round() as u16;
        let hy = head_y.round() as u16;
        put_symbol(
            buf,
            hx,
            hy,
            "▶",
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        );
        if hx.saturating_add(1) < area.x.saturating_add(area.width) {
            put_symbol(
                buf,
                hx.saturating_add(1),
                hy,
                "●",
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }
}

fn draw_nav_cursor(
    scene: &SceneState,
    area: Rect,
    buf: &mut Buffer,
    y_projection: VisibleYProjection,
) {
    if !scene.nav_mode {
        return;
    }
    let Some(path) = scene.cursor_path.as_ref() else {
        return;
    };
    let Some(index) = scene.layout.by_path.get(path).copied() else {
        return;
    };
    let Some(node) = scene.layout.nodes.get(index) else {
        return;
    };
    let Some((x, y)) = map_point(area, node.x, y_projection.normalize(node.y)) else {
        return;
    };

    let style = Style::default()
        .fg(Color::LightCyan)
        .add_modifier(Modifier::BOLD);
    let left = i32::from(x) - 1;
    let right = i32::from(x) + 1;
    let top = i32::from(y) - 1;
    let bottom = i32::from(y) + 1;
    for (cell_x, cell_y, symbol) in [
        (left, top, "┌"),
        (x.into(), top, "─"),
        (right, top, "┐"),
        (left, y.into(), "│"),
        (right, y.into(), "│"),
        (left, bottom, "└"),
        (x.into(), bottom, "─"),
        (right, bottom, "┘"),
    ] {
        put_symbol_in_area(buf, area, cell_x, cell_y, symbol, style);
    }
}

fn draw_diff_popup(
    scene: &SceneState,
    area: Rect,
    buf: &mut Buffer,
    y_projection: VisibleYProjection,
) {
    if !scene.nav_mode || !scene.diff_open || area.width < 8 || area.height < 5 {
        return;
    }

    let Some(path) = scene.cursor_path.as_ref() else {
        return;
    };
    let Some(detail) = scene.node_details.get(path) else {
        return;
    };
    if detail.is_dir {
        return;
    }
    let Some(diff) = detail.diff.as_ref() else {
        return;
    };
    let Some(index) = scene.layout.by_path.get(path).copied() else {
        return;
    };
    let Some(node) = scene.layout.nodes.get(index) else {
        return;
    };
    let Some(anchor) = map_point(area, node.x, y_projection.normalize(node.y)) else {
        return;
    };

    let width = DIFF_POPUP_WIDTH.min(area.width);
    let height = DIFF_POPUP_HEIGHT.min(area.height);
    if width < 8 || height < 5 {
        return;
    }

    let popup = anchored_popup_rect(area, anchor, width, height);
    clear_area_with_style(buf, popup, Style::default().bg(Color::DarkGray));
    draw_box(
        buf,
        popup,
        Style::default().fg(Color::Gray).bg(Color::DarkGray),
    );

    let inner_x = popup.x.saturating_add(1);
    let inner_y = popup.y.saturating_add(1);
    let inner_width = popup.width.saturating_sub(2);
    let inner_height = popup.height.saturating_sub(2);
    if inner_width == 0 || inner_height == 0 {
        return;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_else(|| path.to_str().unwrap_or("file"));
    let title = format!(
        "{} {} {}",
        file_name,
        short_text(&diff.commit_oid, 8),
        diff.date.format("%Y-%m-%d")
    );
    put_text(
        buf,
        inner_x,
        inner_y,
        &title,
        inner_width,
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    let diff_area_height = usize::from(inner_height.saturating_sub(1));
    if diff_area_height == 0 {
        return;
    }
    let lines = diff_lines(diff, usize::MAX);
    for (offset, (text, style)) in lines
        .iter()
        .skip(scene.diff_scroll)
        .take(diff_area_height)
        .enumerate()
    {
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        put_text(
            buf,
            inner_x,
            inner_y.saturating_add(1).saturating_add(offset),
            text,
            inner_width,
            (*style).bg(Color::DarkGray),
        );
    }
}

fn anchored_popup_rect(area: Rect, anchor: (u16, u16), width: u16, height: u16) -> Rect {
    let right_edge = area.x.saturating_add(area.width);
    let bottom_edge = area.y.saturating_add(area.height);

    let right_x = anchor.0.saturating_add(2);
    let x = if right_x.saturating_add(width) <= right_edge {
        right_x
    } else if anchor.0 > area.x.saturating_add(width) {
        anchor.0.saturating_sub(width).saturating_sub(1)
    } else {
        area.x
    };

    let below_y = anchor.1.saturating_add(1);
    let y = if below_y.saturating_add(height) <= bottom_edge {
        below_y
    } else if anchor.1 > area.y.saturating_add(height) {
        anchor.1.saturating_sub(height).saturating_sub(1)
    } else {
        area.y
    };

    Rect {
        x,
        y,
        width,
        height,
    }
}

fn draw_box(buf: &mut Buffer, area: Rect, style: Style) {
    let right = area.x.saturating_add(area.width.saturating_sub(1));
    let bottom = area.y.saturating_add(area.height.saturating_sub(1));
    for x in area.x.saturating_add(1)..right {
        put_symbol(buf, x, area.y, "─", style);
        put_symbol(buf, x, bottom, "─", style);
    }
    for y in area.y.saturating_add(1)..bottom {
        put_symbol(buf, area.x, y, "│", style);
        put_symbol(buf, right, y, "│", style);
    }
    put_symbol(buf, area.x, area.y, "┌", style);
    put_symbol(buf, right, area.y, "┐", style);
    put_symbol(buf, area.x, bottom, "└", style);
    put_symbol(buf, right, bottom, "┘", style);
}

fn draw_side_overlay(scene: &SceneState, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if area.width < 5 {
        return;
    }

    clear_area(buf, area);
    let divider_style = Style::default().fg(Color::DarkGray);
    for y in area.y..area.y.saturating_add(area.height) {
        put_symbol(buf, area.x, y, "│", divider_style);
    }

    let x = area.x.saturating_add(1);
    let width = area.width.saturating_sub(1);
    let end_y = area.y.saturating_add(area.height);
    let mut y = area.y;

    y = draw_legend_section(x, y, end_y, width, buf);
    y = y.saturating_add(1);
    y = draw_node_detail_section(scene, x, y, end_y, width, buf);
    if y < end_y {
        y = y.saturating_add(1);
    }

    let tail_reserve = if scene.show_authors && end_y.saturating_sub(y) > 16 {
        7
    } else if scene.show_authors {
        0
    } else {
        (area.height / 3).max(4)
    };
    let author_end_y = end_y.saturating_sub(tail_reserve).max(y);
    y = draw_authors_section(scene, x, y, author_end_y, width, buf);
    if y < end_y {
        y = y.saturating_add(1);
    }

    let stats_start = end_y.saturating_sub(4);
    if y < stats_start {
        y = draw_hot_files_section(scene, x, y, stats_start, width, buf);
        if y < stats_start {
            y = y.saturating_add(1);
        }
    }
    draw_stats_section(scene, x, y.max(stats_start), end_y, width, buf);
}

fn draw_legend_section(x: u16, y: u16, end_y: u16, width: u16, buf: &mut Buffer) -> u16 {
    if y >= end_y {
        return y;
    }

    put_text(
        buf,
        x,
        y,
        "legend",
        width,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let mut row = y.saturating_add(1);
    for (symbol, label, color) in [
        ("■", "top relative heat", Color::LightRed),
        ("■", "mid relative heat", Color::LightGreen),
        ("■", "low relative heat", Color::Blue),
        ("●", "pulse commit", Color::LightYellow),
    ] {
        if row >= end_y {
            break;
        }
        put_symbol(
            buf,
            x,
            row,
            symbol,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        );
        put_text(
            buf,
            x.saturating_add(2),
            row,
            label,
            width.saturating_sub(2),
            Style::default().fg(Color::Gray),
        );
        row = row.saturating_add(1);
    }

    row
}

fn draw_stats_section(
    scene: &SceneState,
    x: u16,
    y: u16,
    end_y: u16,
    width: u16,
    buf: &mut Buffer,
) -> u16 {
    if y >= end_y {
        return y;
    }

    put_text(
        buf,
        x,
        y,
        "stats",
        width,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let stats = [
        format!("paths: {}", scene.layout.by_path.len()),
        format!("visible: {}", scene.visible_paths.len()),
        format!("authors: {}", scene.authors.len()),
    ];
    let mut row = y.saturating_add(1);
    for stat in stats {
        if row >= end_y {
            break;
        }
        put_text(buf, x, row, &stat, width, Style::default().fg(Color::Gray));
        row = row.saturating_add(1);
    }

    row
}

fn draw_node_detail_section(
    scene: &SceneState,
    x: u16,
    y: u16,
    end_y: u16,
    width: u16,
    buf: &mut Buffer,
) -> u16 {
    if y >= end_y || !scene.nav_mode {
        return y;
    }

    let Some(path) = scene.cursor_path.as_ref() else {
        return y;
    };
    let Some(detail) = scene.node_details.get(path) else {
        return y;
    };

    put_text(
        buf,
        x,
        y,
        if detail.is_dir {
            "directory detail"
        } else {
            "file detail"
        },
        width,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let mut row = y.saturating_add(1);
    let lines = node_detail_lines(detail);
    for (text, style) in lines {
        if row >= end_y {
            break;
        }
        put_text(buf, x, row, &fit_text(&text, width), width, style);
        row = row.saturating_add(1);
    }

    row
}

fn node_detail_lines(detail: &NodeDetail) -> Vec<(String, Style)> {
    let mut lines = vec![
        (
            format!("path {}", detail.path.display()),
            Style::default().fg(Color::Gray),
        ),
        (
            if detail.is_dir {
                format!(
                    "files {}  changes {}",
                    detail.child_file_count, detail.total_changes
                )
            } else {
                format!("heat {}  changes {}", detail.heat, detail.total_changes)
            },
            Style::default().fg(Color::Gray),
        ),
    ];

    if let Some(recent) = &detail.recent_commit {
        lines.push((
            format!("last {} {}", recent.date, recent.author),
            Style::default().fg(Color::Gray),
        ));
        lines.push((
            format!("msg {}", recent.message),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        lines.push((
            "last none".to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    }

    if detail.contributors.is_empty() {
        lines.push((
            "contributors none".to_string(),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        lines.push((
            "contributors".to_string(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
        for contributor in detail.contributors.iter().take(4) {
            lines.push((
                format!("  {} {}", contributor.label, contributor.commits),
                Style::default().fg(author_color(&contributor.key)),
            ));
        }
    }

    lines
}

fn diff_lines(diff: &FileDiff, limit: usize) -> Vec<(String, Style)> {
    let removed = diff
        .removed
        .iter()
        .map(|line| (format!("-{line}"), Style::default().fg(Color::Red)));
    let added = diff
        .added
        .iter()
        .map(|line| (format!("+{line}"), Style::default().fg(Color::Green)));

    removed.chain(added).take(limit).collect()
}

fn short_text(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn draw_authors_section(
    scene: &SceneState,
    x: u16,
    y: u16,
    end_y: u16,
    width: u16,
    buf: &mut Buffer,
) -> u16 {
    if y >= end_y {
        return y;
    }

    put_text(
        buf,
        x,
        y,
        if scene.show_authors {
            "authors (a)"
        } else {
            "authors"
        },
        width,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let remaining_rows = end_y.saturating_sub(y.saturating_add(1));
    let visible_rows = if scene.show_authors {
        remaining_rows
    } else {
        remaining_rows.saturating_sub(3).min(4)
    };
    let max_rows = visible_rows;
    let start = if scene.show_authors {
        scene
            .author_scroll
            .min(scene.authors.len().saturating_sub(usize::from(max_rows)))
    } else {
        0
    };
    let mut row = y.saturating_add(1);
    for author in scene.authors.iter().skip(start).take(usize::from(max_rows)) {
        if row >= end_y {
            break;
        }
        put_symbol(
            buf,
            x,
            row,
            "■",
            Style::default()
                .fg(author_color(&author.key))
                .add_modifier(Modifier::BOLD),
        );
        let commits = author.commits.to_string();
        let commits_width = u16::try_from(UnicodeWidthStr::width(commits.as_str())).unwrap_or(0);
        let name_width = width.saturating_sub(3).saturating_sub(commits_width);
        let label = fit_text(&author.label, name_width);
        put_text(
            buf,
            x.saturating_add(2),
            row,
            &label,
            name_width,
            Style::default().fg(Color::Gray),
        );
        if commits_width < width {
            put_text(
                buf,
                x.saturating_add(width).saturating_sub(commits_width),
                row,
                &commits,
                commits_width,
                Style::default().fg(Color::DarkGray),
            );
        }
        row = row.saturating_add(1);
    }

    row
}

fn draw_hot_files_section(
    scene: &SceneState,
    x: u16,
    y: u16,
    end_y: u16,
    width: u16,
    buf: &mut Buffer,
) -> u16 {
    if y >= end_y {
        return y;
    }

    put_text(
        buf,
        x,
        y,
        "hot files",
        width,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let list_top = y.saturating_add(1);
    let max_rows = end_y.saturating_sub(list_top);
    let heat_scale = HeatScale::from_heats(scene.hot_files.iter().map(|file| file.heat));
    for (row, file) in scene
        .hot_files
        .iter()
        .take(usize::from(max_rows))
        .enumerate()
    {
        let Ok(row) = u16::try_from(row) else {
            break;
        };
        let y = list_top.saturating_add(row);
        if y >= end_y {
            break;
        }
        put_symbol(
            buf,
            x,
            y,
            "■",
            Style::default()
                .fg(heat_color(heat_scale.band(file.heat)))
                .add_modifier(Modifier::BOLD),
        );
        let label_x = x.saturating_add(2);
        let heat_text = format!(" {}", file.heat);
        let heat_width = u16::try_from(UnicodeWidthStr::width(heat_text.as_str())).unwrap_or(0);
        let name_width = width.saturating_sub(2).saturating_sub(heat_width);
        let name = display_path(&file.path, name_width);
        put_text(
            buf,
            label_x,
            y,
            &name,
            name_width,
            Style::default().fg(Color::Gray),
        );
        if heat_width < width {
            put_text(
                buf,
                x.saturating_add(width).saturating_sub(heat_width),
                y,
                &heat_text,
                heat_width,
                Style::default().fg(heat_color(heat_scale.band(file.heat))),
            );
        }
    }

    list_top.saturating_add(max_rows)
}

fn draw_bottom_overlay(scene: &SceneState, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let divider = Style::default().fg(Color::DarkGray);
    for x in area.x..area.x.saturating_add(area.width) {
        put_symbol(buf, x, area.y, "-", divider);
    }

    if area.height < 2 {
        return;
    }

    // Line 2: playback state + inline progress, kept short so the keys line stays readable.
    let focus = scene.focus.as_deref().unwrap_or("none");
    let progress = format!("{}/{}", scene.cursor, scene.total);
    let state = if scene.nav_mode {
        let path = scene
            .cursor_path
            .as_ref()
            .map(|path| display_path(path, 28))
            .unwrap_or_else(|| "none".to_string());
        format!("◆ navigating  {}  node {}", progress, path)
    } else if scene.cursor >= scene.total {
        format!("● finished  {}  focus {}", progress, focus)
    } else {
        format!(
            "{}  {}  {}  focus {}",
            if scene.paused {
                "‖ paused"
            } else {
                "▶ playing"
            },
            scene.speed,
            progress,
            focus
        )
    };
    let state_style = Style::default()
        .fg(if scene.nav_mode {
            Color::LightCyan
        } else if scene.paused {
            Color::Yellow
        } else {
            Color::Green
        })
        .add_modifier(Modifier::BOLD);
    put_text(
        buf,
        area.x,
        area.y.saturating_add(1),
        &state,
        area.width,
        state_style,
    );

    if area.height < 3 {
        return;
    }

    // Line 3: permanent keys hint in a bright color so users always see how to operate.
    let keys = if scene.nav_mode {
        "nav: ↑↓←→ move cursor · enter/space details · tab/esc playback · a authors · q quit"
    } else if scene.show_authors {
        "authors: ↑↓ scroll · a close · tab tree nav · space play/pause · ←→ step · ? help · esc quit"
    } else {
        "space play/pause · tab tree nav · ←→ step · ↑↓ speed · </> jump · f focus · a authors · ? help · esc quit"
    };
    let keys_style = Style::default().fg(Color::Cyan);
    put_text(
        buf,
        area.x,
        area.y.saturating_add(2),
        keys,
        area.width,
        keys_style,
    );

    if area.height < 4 {
        return;
    }

    draw_progress_bar(scene, area, buf);
}

fn draw_help_overlay(area: Rect, buf: &mut Buffer) {
    if area.width < 40 || area.height < 7 {
        return;
    }

    let width = area.width.min(76);
    let height = 8;
    let x = area
        .x
        .saturating_add((area.width.saturating_sub(width)) / 2);
    let y = area
        .y
        .saturating_add((area.height.saturating_sub(height)) / 2);
    let overlay = Rect {
        x,
        y,
        width,
        height,
    };
    clear_area(buf, overlay);

    let style = Style::default().fg(Color::Gray);
    put_text(
        buf,
        x,
        y,
        "help",
        width,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );
    for (offset, text) in [
        "space play/pause | ←/→ step one commit | </> jump small",
        "Shift+←/→ or PageUp/PageDown jump larger | ↑/↓ speed",
        "tab tree navigation | nav arrows move | enter/space details",
        "f focus next author | a authors | r reset | ? close help",
        "esc/q quit | Ctrl+C quit",
    ]
    .into_iter()
    .enumerate()
    {
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        put_text(
            buf,
            x,
            y.saturating_add(offset).saturating_add(2),
            text,
            width,
            style,
        );
    }
}

fn clear_area(buf: &mut Buffer, area: Rect) {
    let style = Style::default();
    clear_area_with_style(buf, area, style);
}

fn clear_area_with_style(buf: &mut Buffer, area: Rect, style: Style) {
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            put_symbol(buf, x, y, " ", style);
        }
    }
}

fn parent_path(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    (!parent.as_os_str().is_empty()).then(|| parent.to_path_buf())
}

fn path_with_ancestors(path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        paths.push(current.clone());
    }
    paths
}

fn draw_dir_node(buf: &mut Buffer, area: Rect, x: u16, y: u16, heat_band: u8, active: bool) {
    draw_node_glow(buf, area, x, y, heat_band);

    let color = if active {
        Color::LightYellow
    } else {
        heat_color(heat_band)
    };
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    put_symbol(buf, x, y, "█", style);

    if heat_band >= 2 {
        put_symbol_in_area(buf, area, i32::from(x) - 1, i32::from(y), "■", style);
        put_symbol_in_area(buf, area, i32::from(x) + 1, i32::from(y), "■", style);
    }
    if heat_band >= 3 {
        put_symbol_in_area(buf, area, i32::from(x), i32::from(y) - 1, "■", style);
        put_symbol_in_area(buf, area, i32::from(x), i32::from(y) + 1, "■", style);
    }
    if heat_band >= 4 {
        for dy in -1_i32..=1 {
            for dx in -1_i32..=1 {
                put_symbol_in_area(buf, area, i32::from(x) + dx, i32::from(y) + dy, "█", style);
            }
        }
    }
}

fn draw_file_node(buf: &mut Buffer, x: u16, y: u16, heat_band: u8, active: bool) {
    let color = if active {
        Color::LightYellow
    } else {
        heat_color(heat_band)
    };
    put_symbol(
        buf,
        x,
        y,
        if active { "█" } else { "■" },
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    );
}

fn draw_node_glow(buf: &mut Buffer, area: Rect, x: u16, y: u16, heat_band: u8) {
    if heat_band == 0 || area.width < 3 || area.height < 3 {
        return;
    }

    let style = Style::default().fg(dim_heat_color(heat_band));
    for (dx, dy) in [
        (-1_i32, -1_i32),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ] {
        let cell_x = i32::from(x) + dx;
        let cell_y = i32::from(y) + dy;
        if cell_x < i32::from(area.x)
            || cell_y < i32::from(area.y)
            || cell_x >= i32::from(area.x.saturating_add(area.width))
            || cell_y >= i32::from(area.y.saturating_add(area.height))
        {
            continue;
        }
        put_symbol_if_empty(buf, cell_x as u16, cell_y as u16, "■", style);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HeatScale {
    bands: HashMap<u32, u8>,
}

impl HeatScale {
    fn from_graph(scene: &SceneState, graph: &RenderGraph) -> Self {
        let heats = graph
            .visible_dirs
            .iter()
            .chain(graph.visible_files.iter())
            .map(|index| {
                scene
                    .layout
                    .nodes
                    .get(*index)
                    .map(|node| node.heat)
                    .unwrap_or(0)
            });
        Self::from_heats(heats)
    }

    fn from_heats(heats: impl Iterator<Item = u32>) -> Self {
        let mut sorted = heats.filter(|heat| *heat > 0).collect::<Vec<_>>();
        sorted.sort_unstable();
        if sorted.is_empty() {
            return Self {
                bands: HashMap::new(),
            };
        }

        let mut bands = HashMap::new();
        let last = sorted.len().saturating_sub(1);
        for heat in sorted.iter().copied() {
            let rank = sorted.partition_point(|candidate| *candidate < heat);
            let percentile = if last == 0 {
                0.0
            } else {
                rank as f64 / last as f64
            };
            bands.insert(heat, heat_band_from_percentile(percentile));
        }

        let visible_band_count = bands.values().copied().collect::<HashSet<_>>().len();
        let mut distinct_heats = sorted;
        distinct_heats.dedup();
        if distinct_heats.len() >= 5 && visible_band_count < 5 {
            bands = heat_bands_from_distinct_values(&distinct_heats);
        }

        Self { bands }
    }

    fn band(&self, heat: u32) -> u8 {
        self.bands.get(&heat).copied().unwrap_or(0)
    }
}

fn heat_bands_from_distinct_values(sorted_heats: &[u32]) -> HashMap<u32, u8> {
    let last = sorted_heats.len().saturating_sub(1);
    sorted_heats
        .iter()
        .enumerate()
        .map(|(rank, heat)| {
            let percentile = if last == 0 {
                0.0
            } else {
                rank as f64 / last as f64
            };
            (*heat, heat_band_from_percentile(percentile))
        })
        .collect()
}

fn heat_band_from_percentile(percentile: f64) -> u8 {
    if percentile >= 0.90 {
        4
    } else if percentile >= 0.70 {
        3
    } else if percentile >= 0.40 {
        2
    } else if percentile >= 0.10 {
        1
    } else {
        0
    }
}

fn heat_color(heat_band: u8) -> Color {
    match heat_band {
        0 => Color::Blue,
        1 => Color::Cyan,
        2 => Color::LightGreen,
        3 => Color::Yellow,
        _ => Color::LightRed,
    }
}

fn dim_heat_color(heat_band: u8) -> Color {
    match heat_band {
        0 => Color::Rgb(24, 72, 130),
        1 => Color::Rgb(0, 100, 120),
        2 => Color::Rgb(28, 120, 74),
        3 => Color::Rgb(135, 116, 20),
        _ => Color::Rgb(150, 42, 42),
    }
}

fn map_point(area: Rect, x: f64, y: f64) -> Option<(u16, u16)> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let x = x.clamp(0.0, 1.0);
    let y = y.clamp(0.0, 1.0);
    let cell_x = area
        .x
        .saturating_add((x * f64::from(area.width.saturating_sub(1))).round() as u16);
    let cell_y = area
        .y
        .saturating_add((y * f64::from(area.height.saturating_sub(1))).round() as u16);
    Some((cell_x, cell_y))
}

fn draw_line(buf: &mut Buffer, from: (u16, u16), to: (u16, u16), style: Style) {
    let mut x0 = i32::from(from.0);
    let mut y0 = i32::from(from.1);
    let x1 = i32::from(to.0);
    let y1 = i32::from(to.1);
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if (x0, y0) != (i32::from(from.0), i32::from(from.1))
            && (x0, y0) != (i32::from(to.0), i32::from(to.1))
            && x0 >= 0
            && y0 >= 0
        {
            put_symbol(buf, x0 as u16, y0 as u16, "█", style);
        }

        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

pub fn author_anchor_position(index: usize, total: usize) -> (f64, f64) {
    if total == 0 {
        return (0.5, 0.03);
    }
    let step = 1.0 / (total + 1) as f64;
    ((step * (index + 1) as f64).clamp(0.04, 0.96), 0.03)
}

pub fn author_color(identity: &str) -> Color {
    const PALETTE: [Color; 8] = [
        Color::LightCyan,
        Color::LightMagenta,
        Color::LightGreen,
        Color::LightRed,
        Color::Cyan,
        Color::Magenta,
        Color::Green,
        Color::Yellow,
    ];

    PALETTE[stable_hash(identity.as_bytes()) % PALETTE.len()]
}

fn stable_hash(bytes: &[u8]) -> usize {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash as usize
}

fn author_label(name: &str, fallback: &str) -> String {
    let name = name.trim();
    if !name.is_empty() {
        return name.to_string();
    }

    let prefix = fallback.split('@').next().unwrap_or(fallback).trim();
    if prefix.is_empty() {
        fallback.to_string()
    } else {
        prefix.to_string()
    }
}

fn put_text(buf: &mut Buffer, x: u16, y: u16, text: &str, width: u16, style: Style) {
    if width == 0 || UnicodeWidthStr::width(text) == 0 {
        return;
    }

    let mut offset: u16 = 0;
    for ch in text.chars() {
        let char_width = ch.width().unwrap_or(0);
        if char_width == 0 {
            continue;
        }

        let Ok(char_width) = u16::try_from(char_width) else {
            break;
        };
        if offset.saturating_add(char_width) > width {
            break;
        }

        put_symbol(buf, x.saturating_add(offset), y, ch.to_string(), style);

        for continuation in 1..char_width {
            put_symbol(
                buf,
                x.saturating_add(offset).saturating_add(continuation),
                y,
                " ",
                style,
            );
        }

        offset = offset.saturating_add(char_width);
    }
}

fn put_symbol(buf: &mut Buffer, x: u16, y: u16, symbol: impl AsRef<str>, style: Style) {
    buf[(x, y)].set_symbol(symbol.as_ref()).set_style(style);
}

fn put_symbol_if_empty(buf: &mut Buffer, x: u16, y: u16, symbol: impl AsRef<str>, style: Style) {
    if buf[(x, y)].symbol().trim().is_empty() {
        put_symbol(buf, x, y, symbol, style);
    }
}

fn put_symbol_in_area(
    buf: &mut Buffer,
    area: Rect,
    x: i32,
    y: i32,
    symbol: impl AsRef<str>,
    style: Style,
) {
    if x < i32::from(area.x)
        || y < i32::from(area.y)
        || x >= i32::from(area.x.saturating_add(area.width))
        || y >= i32::from(area.y.saturating_add(area.height))
    {
        return;
    }
    put_symbol(buf, x as u16, y as u16, symbol, style);
}

fn side_width(width: u16, tree_height: u16) -> Option<u16> {
    if width < 64 || tree_height < 10 {
        return None;
    }
    // Scale with terminal width so wide screens get a usable panel (legend/authors/
    // hot files/diff) instead of a cramped fixed 28-32 columns.
    let desired = (width / 4).clamp(30, width.saturating_sub(60).max(40));
    Some(desired)
}

fn pulse_target_node<'a>(layout: &'a Layout, path: &Path) -> Option<&'a crate::layout::LayoutNode> {
    let mut current = parent_path(path);
    while let Some(path) = current {
        if let Some(index) = layout.by_path.get(&path).copied()
            && let Some(node) = layout.nodes.get(index)
            && node.is_dir
        {
            return Some(node);
        }
        current = parent_path(&path);
    }

    layout
        .by_path
        .get(path)
        .and_then(|index| layout.nodes.get(*index))
}

fn display_path(path: &Path, width: u16) -> String {
    let text = path.display().to_string();
    fit_text(&text, width)
}

fn fit_text(text: &str, width: u16) -> String {
    let width = usize::from(width);
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width <= 2 {
        return ".".repeat(width);
    }

    let mut out = String::new();
    let keep = width.saturating_sub(2);
    for ch in text
        .chars()
        .rev()
        .take(keep)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if UnicodeWidthStr::width(out.as_str()) + ch.width().unwrap_or(0) > keep {
            break;
        }
        out.push(ch);
    }
    format!("..{out}")
}

fn draw_progress_bar(scene: &SceneState, area: Rect, buf: &mut Buffer) {
    let y = area.y.saturating_add(2);
    let ratio = if scene.total == 0 {
        0.0
    } else {
        scene.cursor as f64 / scene.total as f64
    }
    .clamp(0.0, 1.0);
    let filled = (ratio * f64::from(area.width)).round() as u16;
    for offset in 0..area.width {
        let symbol = if offset < filled { "█" } else { "░" };
        let color = if offset < filled {
            Color::LightCyan
        } else {
            Color::DarkGray
        };
        put_symbol(
            buf,
            area.x.saturating_add(offset),
            y,
            symbol,
            Style::default().fg(color),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf};

    use chrono::{TimeZone, Utc};
    use ratatui::style::Color;

    use super::*;
    use crate::diff::FileDiff;
    use crate::layout::LayoutNode;

    fn scene_with_paths(paths: &[(&str, bool)]) -> SceneState {
        let mut nodes = Vec::new();
        let mut by_path = HashMap::new();

        let last = paths.len().saturating_sub(1).max(1);
        for (row, (path, is_dir)) in paths.iter().enumerate() {
            let path = PathBuf::from(path);
            by_path.insert(path.clone(), row);
            let depth = path.components().count().saturating_sub(1);
            nodes.push(LayoutNode {
                path,
                depth: depth.try_into().unwrap(),
                row: row.try_into().unwrap(),
                x: row as f64 / last as f64,
                y: row as f64 / last as f64,
                heat: 1,
                is_dir: *is_dir,
                children: Vec::new(),
            });
        }
        for index in 0..nodes.len() {
            if let Some(parent) = parent_path(&nodes[index].path)
                && let Some(parent_index) = by_path.get(&parent).copied()
            {
                nodes[parent_index].children.push(index);
            }
        }

        let total_rows = nodes.len().try_into().unwrap();
        let visible_paths = nodes.iter().map(|node| node.path.clone()).collect();

        SceneState {
            layout: Layout {
                nodes,
                total_rows,
                by_path,
                max_visible_rows: total_rows,
            },
            repo_title: "fixture".to_string(),
            span_text: "2024-01 to 2024-01 · 1 commit".to_string(),
            focus: None,
            visible_paths,
            active_paths: HashSet::new(),
            hot_files: Vec::new(),
            authors: Vec::new(),
            pulses: Vec::new(),
            now: None,
            cursor: 0,
            total: 0,
            speed: "1.0x".to_string(),
            paused: false,
            show_help: false,
            show_authors: false,
            cursor_path: None,
            nav_mode: false,
            diff_open: false,
            diff_scroll: 0,
            author_scroll: 0,
            node_details: BTreeMap::new(),
        }
    }

    #[test]
    fn draw_scene_uses_block_symbols_instead_of_path_initials() {
        let scene = scene_with_paths(&[
            ("src", true),
            ("src/模块.rs", false),
            ("src/tail.rs", false),
        ]);
        let area = Rect::new(0, 0, 30, 6);
        let mut buf = Buffer::empty(area);

        draw_scene_to_buffer(&scene, area, &mut buf);

        let symbols = all_symbols(&buf, Rect::new(0, 1, 30, 5));
        assert!(symbols.contains('█') || symbols.contains('■'));
        assert!(!symbols.contains('模'));
        assert!(!symbols.contains('t'));
        assert!(!symbols.contains('╲'));
        assert!(!symbols.contains('─'));
        assert!(!symbols.contains('│'));
    }

    #[test]
    fn draw_scene_highlights_active_paths_without_labels() {
        let mut scene = scene_with_paths(&[
            ("src", true),
            ("src/active.rs", false),
            ("src/inactive.rs", false),
        ]);
        scene.active_paths.insert(PathBuf::from("src/active.rs"));
        let area = Rect::new(0, 0, 30, 6);
        let mut buf = Buffer::empty(area);

        draw_scene_to_buffer(&scene, area, &mut buf);

        assert!(has_cell_with_fg(&buf, area, Color::LightYellow));
        assert!(!all_symbols(&buf, area).contains("active.rs"));
    }

    #[test]
    fn diff_lines_render_added_and_removed_styles() {
        let diff = FileDiff {
            commit_oid: "abcdef123456".to_string(),
            author: "Ada".to_string(),
            date: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
            added: vec!["new line".to_string()],
            removed: vec!["old line".to_string()],
        };

        let lines = diff_lines(&diff, 10);

        assert!(
            lines
                .iter()
                .any(|(text, style)| text == "-old line" && style.fg == Some(Color::Red))
        );
        assert!(
            lines
                .iter()
                .any(|(text, style)| text == "+new line" && style.fg == Some(Color::Green))
        );
    }

    #[test]
    fn draw_diff_popup_renders_near_selected_file_when_open() {
        let mut scene =
            scene_with_paths(&[("src", true), ("src/main.rs", false), ("src/lib.rs", false)]);
        let path = PathBuf::from("src/main.rs");
        scene.nav_mode = true;
        scene.diff_open = true;
        scene.cursor_path = Some(path.clone());
        scene.node_details.insert(
            path.clone(),
            NodeDetail {
                path,
                is_dir: false,
                repo_path: Some(PathBuf::from("/repo")),
                heat: 2,
                child_file_count: 0,
                total_changes: 2,
                recent_commit: None,
                contributors: Vec::new(),
                diff: Some(FileDiff {
                    commit_oid: "abcdef123456".to_string(),
                    author: "Ada".to_string(),
                    date: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
                    added: vec!["new line".to_string()],
                    removed: vec!["old line".to_string()],
                }),
            },
        );
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);

        draw_scene_to_buffer(&scene, area, &mut buf);

        assert!(all_symbols(&buf, area).contains("┌"));
        assert!(all_symbols(&buf, area).contains("-old line"));
        assert!(all_symbols(&buf, area).contains("+new line"));
    }

    fn all_symbols(buf: &Buffer, area: Rect) -> String {
        (area.y..area.y.saturating_add(area.height))
            .flat_map(|y| {
                (area.x..area.x.saturating_add(area.width)).map(move |x| buf[(x, y)].symbol())
            })
            .collect()
    }

    fn has_cell_with_fg(buf: &Buffer, area: Rect, color: Color) -> bool {
        (area.y..area.y.saturating_add(area.height)).any(|y| {
            (area.x..area.x.saturating_add(area.width))
                .any(|x| buf[(x, y)].style().fg == Some(color))
        })
    }
}
