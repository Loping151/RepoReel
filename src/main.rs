mod diff;
mod export;
mod font;
mod ir;
mod layout;
mod render;
mod source;
mod timeline;
mod web;

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use diff::recent_file_diff;
use export::{ExportOptions, export_gif, synthetic_demo_events};
use ir::{HistoryEvent, TimelineTrack};
use layout::{Layout, build_layout};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use render::{
    AuthorInfo, SceneState, author_infos_from_events, build_pulses, hot_files_from_events,
    node_details_from_events, render_scene, repo_title_from_events, span_text_from_events,
};
use source::{GitLogSource, HistorySource, RefRange};
use timeline::TimelinePlayer;
use web::render_replay_html;

const FRAME_INTERVAL: Duration = Duration::from_millis(33);
const ACTIVE_EVENT_COUNT: usize = 3;
const PULSE_EVENT_COUNT: usize = 5;
const HOT_FILE_LIMIT: usize = 32;
const JUMP_EVENT_COUNT: i32 = 20;
const SMALL_JUMP_EVENT_COUNT: i32 = 10;
const DIFF_SCROLL_PAGE_LINES: usize = 7;
const SPEEDS: [f64; 4] = [0.5, 1.0, 2.0, 4.0];

#[derive(Debug, Parser)]
#[command(name = "reporeel", version, about = "CI-reproducible repo movie maker")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Play the repo history interactively in the terminal.
    Play {
        #[arg(
            long,
            value_name = "PATH",
            help = "path to a git repository; repeat for multiple repositories"
        )]
        repo: Vec<PathBuf>,
        #[arg(long, help = "git ref to start from (e.g. HEAD~30, v1.0)")]
        from: Option<String>,
        #[arg(long, help = "git ref to end at (default: HEAD)")]
        to: Option<String>,
    },
    /// Render the repo history to a deterministic GIF (CI-friendly).
    Export {
        #[arg(
            long,
            value_name = "PATH",
            help = "path to a git repository; repeat for multiple repositories"
        )]
        repo: Vec<PathBuf>,
        #[arg(long, help = "git ref to start from (e.g. HEAD~30, v1.0)")]
        from: Option<String>,
        #[arg(long, help = "git ref to end at (default: HEAD)")]
        to: Option<String>,
        #[arg(long, default_value = "hero.gif")]
        out: PathBuf,
        #[arg(long, default_value_t = 15)]
        fps: u16,
        #[arg(long, default_value_t = 120)]
        width: u16,
        #[arg(long, default_value_t = 40)]
        height: u16,
        #[arg(long, default_value_t = 90)]
        max_frames: usize,
    },
    /// Render a built-in synthetic project to a GIF.
    Demo {
        #[arg(long, default_value = "demo.gif")]
        out: PathBuf,
    },
    /// Print a summary of the parsed repo history.
    Events {
        #[arg(
            long,
            value_name = "PATH",
            help = "path to a git repository; repeat for multiple repositories"
        )]
        repo: Vec<PathBuf>,
        #[arg(long, help = "git ref to start from (e.g. HEAD~30, v1.0)")]
        from: Option<String>,
        #[arg(long, help = "git ref to end at (default: HEAD)")]
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Export a self-contained HTML replay for browser playback.
    Web {
        #[arg(
            long,
            value_name = "PATH",
            num_args = 1..,
            help = "path to a git repository; accepts one or more paths"
        )]
        repo: Vec<PathBuf>,
        #[arg(long, help = "git ref to start from (e.g. HEAD~30, v1.0)")]
        from: Option<String>,
        #[arg(long, help = "git ref to end at (default: HEAD)")]
        to: Option<String>,
        #[arg(long, default_value = "replay.html")]
        out: PathBuf,
        #[arg(
            long,
            value_name = "PATH",
            help = "background music file to embed in the HTML replay"
        )]
        music: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Play { repo, from, to } => run_play(repo, from, to)?,
        Command::Export {
            repo,
            from,
            to,
            out,
            fps,
            width,
            height,
            max_frames,
        } => run_export(repo, from, to, out, fps, width, height, max_frames)?,
        Command::Demo { out } => run_demo(out)?,
        Command::Events {
            repo,
            from,
            to,
            json,
        } => run_events(repo, from, to, json)?,
        Command::Web {
            repo,
            from,
            to,
            out,
            music,
        } => run_web(repo, from, to, out, music)?,
    }

    Ok(())
}

fn run_web(
    repos: Vec<PathBuf>,
    from: Option<String>,
    to: Option<String>,
    out: PathBuf,
    music: Option<PathBuf>,
) -> Result<()> {
    let repos = default_repos(repos);
    eprintln!("reporeel: loading {}...", repo_list_label(&repos));
    let track = load_track(repos, from, to)?;
    let audio_base64 = match music {
        Some(path) => {
            let bytes = fs::read(&path)?;
            eprintln!(
                "reporeel: embedding background music from {} (HTML will grow by roughly one third of the audio size; keep audio files small for sharing).",
                path.display()
            );
            Some(base64_encode(&bytes))
        }
        None => None,
    };
    let html = render_replay_html(&track, audio_base64.as_deref());
    fs::write(&out, html)?;
    eprintln!(
        "reporeel: web replay written to {} - open in browser to view.",
        out.display()
    );
    Ok(())
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);

        encoded.push(TABLE[(b0 >> 2) as usize] as char);
        encoded.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

#[allow(clippy::too_many_arguments)]
fn run_export(
    repos: Vec<PathBuf>,
    from: Option<String>,
    to: Option<String>,
    out: PathBuf,
    fps: u16,
    width: u16,
    height: u16,
    max_frames: usize,
) -> Result<()> {
    validate_export_args(width, height, fps, max_frames)?;
    let track = load_track(repos, from, to)?;
    let options = ExportOptions {
        width,
        height,
        fps,
        max_frames,
    };
    export_gif(track.git_events, options, out)?;
    Ok(())
}

fn validate_export_args(width: u16, height: u16, fps: u16, max_frames: usize) -> Result<()> {
    if width < 1 {
        bail!("width must be >= 1");
    }
    if height < 1 {
        bail!("height must be >= 1");
    }
    if fps < 1 {
        bail!("fps must be >= 1");
    }
    if max_frames < 1 {
        bail!("max-frames must be >= 1");
    }
    Ok(())
}

fn run_demo(out: PathBuf) -> Result<()> {
    export_gif(
        synthetic_demo_events(),
        ExportOptions {
            width: 120,
            height: 40,
            fps: 8,
            max_frames: 90,
        },
        out.clone(),
    )?;
    eprintln!(
        "reporeel: demo written to {} — open it to view.",
        out.display()
    );
    Ok(())
}

fn run_events(
    repos: Vec<PathBuf>,
    from: Option<String>,
    to: Option<String>,
    json: bool,
) -> Result<()> {
    let track = load_track(repos, from, to)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&track)?);
    } else {
        print_summary(&track);
    }

    Ok(())
}

fn run_play(repos: Vec<PathBuf>, from: Option<String>, to: Option<String>) -> Result<()> {
    let repos = default_repos(repos);
    eprintln!("loading {}...", repo_list_label(&repos));
    let track = load_track(repos, from, to)?;
    eprintln!("loaded {} events, starting...", track.git_events.len());
    let layout = build_layout(&track.git_events);
    let authors = author_infos_from_events(&track.git_events);
    let node_details = node_details_from_events(&track.git_events, &layout);
    let repo_title = repo_title_from_events(&track.git_events);
    let span_text = span_text_from_events(&track.git_events);
    let player = TimelinePlayer::new(track.git_events);

    let mut terminal = ratatui::try_init()?;
    {
        let size = terminal.size()?;
        // Some non-interactive parents (script, pipes) report 0x0; only warn on real-but-small sizes.
        if size.width != 0 && size.height != 0 && size.height < 20 {
            eprintln!(
                "reporeel: warning: terminal is small ({}x{}); enlarge the window for a clearer view",
                size.width, size.height
            );
        }
    }
    let render_result = run_terminal_scene(
        &mut terminal,
        layout,
        player,
        authors,
        node_details,
        repo_title,
        span_text,
    );
    let restore_result = ratatui::try_restore();
    restore_result?;
    render_result
}

fn run_terminal_scene(
    terminal: &mut ratatui::DefaultTerminal,
    layout: Layout,
    mut player: TimelinePlayer,
    authors: Vec<AuthorInfo>,
    mut node_details: BTreeMap<PathBuf, render::NodeDetail>,
    repo_title: String,
    span_text: String,
) -> Result<()> {
    let mut paused = true;
    let mut show_help = true;
    let mut show_authors = false;
    let mut focus_author_idx: Option<usize> = None;
    let mut nav_mode = false;
    let mut cursor_path: Option<PathBuf> = None;
    let mut diff_open = false;
    let mut diff_scroll = 0;
    let mut author_scroll = 0;
    let mut speed_remainder = 0.0;

    draw_play_frame(
        terminal,
        &layout,
        &player,
        &authors,
        &repo_title,
        &span_text,
        &node_details,
        focus_author_idx,
        cursor_path.clone(),
        paused,
        show_help,
        show_authors,
        nav_mode,
        diff_open,
        diff_scroll,
        author_scroll,
    )?;

    loop {
        let mut redraw = false;

        if event::poll(FRAME_INTERVAL)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('q') => break,
                    KeyCode::Esc if nav_mode && diff_open => {
                        diff_open = false;
                        diff_scroll = 0;
                        redraw = true;
                    }
                    KeyCode::Esc if nav_mode => {
                        nav_mode = false;
                        diff_open = false;
                        diff_scroll = 0;
                        redraw = true;
                    }
                    KeyCode::Esc => break,
                    KeyCode::Tab => {
                        show_help = false;
                        nav_mode = !nav_mode;
                        if nav_mode {
                            paused = true;
                            cursor_path = cursor_path.or_else(|| {
                                initial_cursor_path(
                                    &layout,
                                    &playback_paths(
                                        player.events_up_to_cursor(),
                                        focus_author_idx
                                            .and_then(|index| authors.get(index))
                                            .map(|author| author.key.as_str()),
                                        ACTIVE_EVENT_COUNT,
                                    ),
                                )
                            });
                        } else {
                            diff_open = false;
                            diff_scroll = 0;
                        }
                        redraw = true;
                    }
                    KeyCode::Char('?') => {
                        show_help = !show_help;
                        paused = true;
                        redraw = true;
                    }
                    KeyCode::Up if nav_mode && diff_open => {
                        diff_scroll = diff_scroll.saturating_sub(DIFF_SCROLL_PAGE_LINES);
                        redraw = true;
                    }
                    KeyCode::Down if nav_mode && diff_open => {
                        diff_scroll = diff_scroll
                            .saturating_add(DIFF_SCROLL_PAGE_LINES)
                            .min(max_diff_scroll(&node_details, cursor_path.as_deref()));
                        redraw = true;
                    }
                    KeyCode::Left | KeyCode::Right if nav_mode && diff_open => {}
                    KeyCode::Up if nav_mode => {
                        cursor_path =
                            move_cursor_path(&layout, cursor_path.as_deref(), Direction::Up);
                        diff_open = false;
                        diff_scroll = 0;
                        redraw = true;
                    }
                    KeyCode::Down if nav_mode => {
                        cursor_path =
                            move_cursor_path(&layout, cursor_path.as_deref(), Direction::Down);
                        diff_open = false;
                        diff_scroll = 0;
                        redraw = true;
                    }
                    KeyCode::Left if nav_mode => {
                        cursor_path =
                            move_cursor_path(&layout, cursor_path.as_deref(), Direction::Left);
                        diff_open = false;
                        diff_scroll = 0;
                        redraw = true;
                    }
                    KeyCode::Right if nav_mode => {
                        cursor_path =
                            move_cursor_path(&layout, cursor_path.as_deref(), Direction::Right);
                        diff_open = false;
                        diff_scroll = 0;
                        redraw = true;
                    }
                    KeyCode::Enter if nav_mode && diff_open => {
                        diff_open = false;
                        diff_scroll = 0;
                        redraw = true;
                    }
                    KeyCode::Enter | KeyCode::Char(' ') if nav_mode => {
                        show_help = false;
                        paused = true;
                        load_selected_file_diff(&mut node_details, cursor_path.as_deref())?;
                        diff_open = selected_file_has_diff(&node_details, cursor_path.as_deref());
                        diff_scroll = 0;
                        redraw = true;
                    }
                    KeyCode::Up if show_authors => {
                        author_scroll = author_scroll.saturating_sub(1);
                        redraw = true;
                    }
                    KeyCode::Down if show_authors => {
                        author_scroll = author_scroll
                            .saturating_add(1)
                            .min(authors.len().saturating_sub(1));
                        redraw = true;
                    }
                    KeyCode::Char(' ') => {
                        if show_help {
                            show_help = false;
                            paused = false;
                        } else {
                            paused = !paused;
                        }
                        redraw = true;
                    }
                    KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        show_help = false;
                        player.step_event(-JUMP_EVENT_COUNT);
                        paused = true;
                        speed_remainder = 0.0;
                        redraw = true;
                    }
                    KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        show_help = false;
                        player.step_event(JUMP_EVENT_COUNT);
                        paused = true;
                        speed_remainder = 0.0;
                        redraw = true;
                    }
                    KeyCode::PageUp => {
                        show_help = false;
                        player.step_event(-JUMP_EVENT_COUNT);
                        paused = true;
                        speed_remainder = 0.0;
                        redraw = true;
                    }
                    KeyCode::PageDown => {
                        show_help = false;
                        player.step_event(JUMP_EVENT_COUNT);
                        paused = true;
                        speed_remainder = 0.0;
                        redraw = true;
                    }
                    KeyCode::Char('<') => {
                        show_help = false;
                        player.step_event(-SMALL_JUMP_EVENT_COUNT);
                        paused = true;
                        speed_remainder = 0.0;
                        redraw = true;
                    }
                    KeyCode::Char('>') => {
                        show_help = false;
                        player.step_event(SMALL_JUMP_EVENT_COUNT);
                        paused = true;
                        speed_remainder = 0.0;
                        redraw = true;
                    }
                    KeyCode::Left => {
                        show_help = false;
                        player.step_event(-1);
                        paused = true;
                        speed_remainder = 0.0;
                        redraw = true;
                    }
                    KeyCode::Right => {
                        show_help = false;
                        player.step_event(1);
                        paused = true;
                        speed_remainder = 0.0;
                        redraw = true;
                    }
                    KeyCode::Up => {
                        show_help = false;
                        cycle_speed(&mut player, 1);
                        redraw = true;
                    }
                    KeyCode::Down => {
                        show_help = false;
                        cycle_speed(&mut player, -1);
                        redraw = true;
                    }
                    KeyCode::Char('f') => {
                        show_help = false;
                        focus_author_idx = next_focus_author(focus_author_idx, authors.len());
                        redraw = true;
                    }
                    KeyCode::Char('a') => {
                        show_help = false;
                        show_authors = !show_authors;
                        if !show_authors {
                            author_scroll = 0;
                        }
                        redraw = true;
                    }
                    KeyCode::Char('r') => {
                        show_help = false;
                        player.seek_to_event(0);
                        speed_remainder = 0.0;
                        redraw = true;
                    }
                    _ => {}
                },
                Event::Resize(width, height) => {
                    let event_area = Rect::new(0, 0, width, height);
                    terminal.resize(event_area)?;
                    let backend_size = terminal.size()?;
                    let resized_area = if backend_size.width == 0 || backend_size.height == 0 {
                        event_area
                    } else {
                        Rect::new(0, 0, backend_size.width, backend_size.height)
                    };
                    if resized_area != event_area {
                        terminal.resize(resized_area)?;
                    }
                    redraw = true;
                }
                _ => {}
            }
        } else if !paused && player.cursor() < player.total() {
            speed_remainder += player.speed();
            while speed_remainder >= 1.0 && player.cursor() < player.total() {
                player.step_event(1);
                speed_remainder -= 1.0;
            }
            redraw = true;
        }

        if redraw {
            draw_play_frame(
                terminal,
                &layout,
                &player,
                &authors,
                &repo_title,
                &span_text,
                &node_details,
                focus_author_idx,
                cursor_path.clone(),
                paused,
                show_help,
                show_authors,
                nav_mode,
                diff_open,
                diff_scroll,
                author_scroll,
            )?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_play_frame(
    terminal: &mut ratatui::DefaultTerminal,
    layout: &Layout,
    player: &TimelinePlayer,
    authors: &[AuthorInfo],
    repo_title: &str,
    span_text: &str,
    node_details: &BTreeMap<PathBuf, render::NodeDetail>,
    focus_author_idx: Option<usize>,
    cursor_path: Option<PathBuf>,
    paused: bool,
    show_help: bool,
    show_authors: bool,
    nav_mode: bool,
    diff_open: bool,
    diff_scroll: usize,
    author_scroll: usize,
) -> Result<()> {
    let focus = focus_author_idx
        .and_then(|index| authors.get(index))
        .map(|author| author.label.clone());
    let focus_key = focus_author_idx
        .and_then(|index| authors.get(index))
        .map(|author| author.key.as_str());
    let paths = playback_paths(player.events_up_to_cursor(), focus_key, ACTIVE_EVENT_COUNT);
    let pulses = build_pulses(
        player.events_up_to_cursor(),
        layout,
        authors,
        PULSE_EVENT_COUNT,
    );
    let hot_files = hot_files_from_events(player.events_up_to_cursor(), focus_key, HOT_FILE_LIMIT);
    let scene = SceneState {
        layout: layout.clone(),
        repo_title: repo_title.to_string(),
        span_text: span_text.to_string(),
        focus,
        visible_paths: paths.visible_paths,
        active_paths: paths.active_paths,
        hot_files,
        authors: authors.to_vec(),
        pulses,
        now: player.cursor_time(),
        cursor: player.cursor(),
        total: player.total(),
        speed: format!("{:.1}x", player.speed()),
        paused,
        show_help,
        show_authors,
        cursor_path,
        nav_mode,
        diff_open,
        diff_scroll,
        author_scroll,
        node_details: node_details.clone(),
    };

    terminal.draw(|frame| {
        let area = frame.area();
        render_scene(frame, area, &scene);
    })?;

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaybackPaths {
    visible_paths: HashSet<PathBuf>,
    active_paths: HashSet<PathBuf>,
}

fn playback_paths(
    events: &[HistoryEvent],
    focus_author: Option<&str>,
    active_event_count: usize,
) -> PlaybackPaths {
    let is_focused_event = |event: &&HistoryEvent| {
        focus_author.is_none_or(|author| event.author.identity_key() == author)
    };

    let visible_paths = events
        .iter()
        .filter(is_focused_event)
        .flat_map(|event| event.changes.iter().map(|change| change.path.clone()))
        .collect();

    let active_paths = events
        .iter()
        .rev()
        .filter(is_focused_event)
        .take(active_event_count)
        .flat_map(|event| event.changes.iter().map(|change| change.path.clone()))
        .collect();

    PlaybackPaths {
        visible_paths,
        active_paths,
    }
}

fn initial_cursor_path(layout: &Layout, paths: &PlaybackPaths) -> Option<PathBuf> {
    paths
        .active_paths
        .iter()
        .chain(paths.visible_paths.iter())
        .find(|path| layout.by_path.contains_key(*path))
        .cloned()
        .or_else(|| layout.nodes.first().map(|node| node.path.clone()))
}

fn move_cursor_path(
    layout: &Layout,
    current: Option<&Path>,
    direction: Direction,
) -> Option<PathBuf> {
    if layout.nodes.is_empty() {
        return None;
    }

    let current_index = current
        .and_then(|path| layout.by_path.get(path).copied())
        .unwrap_or(0);
    let current_node = &layout.nodes[current_index];
    let epsilon = 0.0001;

    layout
        .nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != current_index)
        .filter_map(|(_, node)| {
            let dx = node.x - current_node.x;
            let dy = node.y - current_node.y;
            let in_direction = match direction {
                Direction::Up => dy < -epsilon,
                Direction::Down => dy > epsilon,
                Direction::Left => dx < -epsilon,
                Direction::Right => dx > epsilon,
            };
            if !in_direction {
                return None;
            }

            let primary = match direction {
                Direction::Up | Direction::Down => dy.abs(),
                Direction::Left | Direction::Right => dx.abs(),
            };
            let perpendicular = match direction {
                Direction::Up | Direction::Down => dx.abs(),
                Direction::Left | Direction::Right => dy.abs(),
            };
            Some((primary + perpendicular * 0.75, node.path.clone()))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, path)| path)
        .or_else(|| Some(current_node.path.clone()))
}

fn cycle_speed(player: &mut TimelinePlayer, delta: i32) {
    let current = SPEEDS
        .iter()
        .position(|speed| (*speed - player.speed()).abs() < f64::EPSILON)
        .unwrap_or(1);
    let next = (current as i32 + delta).rem_euclid(SPEEDS.len() as i32) as usize;
    player.set_speed(SPEEDS[next]);
}

fn next_focus_author(current: Option<usize>, author_count: usize) -> Option<usize> {
    if author_count == 0 {
        return None;
    }

    match current {
        None => Some(0),
        Some(index) if index + 1 < author_count => Some(index + 1),
        Some(_) => None,
    }
}

fn load_selected_file_diff(
    node_details: &mut BTreeMap<PathBuf, render::NodeDetail>,
    cursor_path: Option<&Path>,
) -> Result<()> {
    let Some(path) = cursor_path else {
        return Ok(());
    };

    let Some(detail) = node_details.get(path) else {
        return Ok(());
    };
    if detail.is_dir || detail.diff.is_some() {
        return Ok(());
    }

    let Some(repo_path) = detail.repo_path.clone() else {
        return Ok(());
    };
    let file_path = detail.path.display().to_string();
    let diff = recent_file_diff(&repo_path, &file_path)?;

    if let Some(detail) = node_details.get_mut(path) {
        detail.diff = diff;
    }

    Ok(())
}

fn selected_file_has_diff(
    node_details: &BTreeMap<PathBuf, render::NodeDetail>,
    cursor_path: Option<&Path>,
) -> bool {
    cursor_path
        .and_then(|path| node_details.get(path))
        .is_some_and(|detail| !detail.is_dir && detail.diff.is_some())
}

fn max_diff_scroll(
    node_details: &BTreeMap<PathBuf, render::NodeDetail>,
    cursor_path: Option<&Path>,
) -> usize {
    cursor_path
        .and_then(|path| node_details.get(path))
        .and_then(|detail| detail.diff.as_ref())
        .map(|diff| {
            diff.added
                .len()
                .saturating_add(diff.removed.len())
                .saturating_sub(DIFF_SCROLL_PAGE_LINES)
        })
        .unwrap_or(0)
}

fn load_track(
    repos: Vec<PathBuf>,
    from: Option<String>,
    to: Option<String>,
) -> Result<TimelineTrack> {
    let repos = default_repos(repos);
    let range = (from.is_some() || to.is_some()).then_some(RefRange { from, to });
    let mut events = Vec::new();

    for repo in repos {
        let source = GitLogSource::new(repo, range.clone());
        events.extend(source.events()?);
    }

    events.sort_by_key(|event| event.author_time);
    Ok(TimelineTrack::from_git_events(events))
}

fn default_repos(repos: Vec<PathBuf>) -> Vec<PathBuf> {
    if repos.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        repos
    }
}

fn repo_list_label(repos: &[PathBuf]) -> String {
    if repos.len() == 1 {
        return repos[0].display().to_string();
    }

    format!("{} repositories", repos.len())
}

fn print_summary(track: &TimelineTrack) {
    let commit_count = track.git_events.len();
    if commit_count == 0 {
        println!("no commits yet in this repository");
        return;
    }

    let mut authors = HashSet::new();
    let mut files = HashSet::new();
    let mut contributor_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut file_counts: BTreeMap<PathBuf, usize> = BTreeMap::new();

    for event in &track.git_events {
        authors.insert(event.author.identity_key().to_string());
        *contributor_counts
            .entry(event.author.name.trim().to_string())
            .or_default() += 1;
        for change in &event.changes {
            files.insert(change.path.clone());
            *file_counts.entry(change.path.clone()).or_default() += 1;
        }
    }

    let span = match (track.git_events.first(), track.git_events.last()) {
        (Some(first), Some(last)) => {
            format!("{} to {}", first.author_time, last.author_time)
        }
        _ => "empty".to_string(),
    };

    let repo_title = repo_title_from_events(&track.git_events);
    let repo_count = unique_repo_count(&track.git_events);

    println!("reporeel: parsed git history for {repo_title}");
    println!("(use --json for full machine-readable timeline)");
    if repo_count == 1 {
        println!("repo: {repo_title}");
    } else {
        println!("repos: {repo_count}");
        println!("repo title: {repo_title}");
    }
    println!("commits: {}", format_count(commit_count));
    println!("authors: {}", authors.len());
    println!("files: {}", format_count(files.len()));
    println!("span: {span}");

    println!("top contributors:");
    for (name, count) in top_contributors(contributor_counts, 3) {
        println!("  - {name} ({count} commits)");
    }

    println!("recent commits:");
    for event in track.git_events.iter().rev().take(3) {
        let short_oid = short_commit_oid(&event.commit_oid);
        let date = event.author_time.format("%Y-%m-%d");
        let message = truncate_chars(event.message.lines().next().unwrap_or("").trim(), 40);
        println!("  - {short_oid} {date} {message}");
    }

    println!("most active files:");
    for (path, count) in top_files(file_counts, 3) {
        println!("  - {} ({count} changes)", path.display());
    }
}

fn unique_repo_count(events: &[HistoryEvent]) -> usize {
    events
        .iter()
        .map(|event| event.repo.as_str())
        .collect::<HashSet<_>>()
        .len()
}

fn top_contributors(counts: BTreeMap<String, usize>, limit: usize) -> Vec<(String, usize)> {
    let mut contributors = counts.into_iter().collect::<Vec<_>>();
    contributors.sort_by(|(left_name, left_count), (right_name, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_name.cmp(right_name))
    });
    contributors.truncate(limit);
    contributors
}

fn top_files(counts: BTreeMap<PathBuf, usize>, limit: usize) -> Vec<(PathBuf, usize)> {
    let mut files = counts.into_iter().collect::<Vec<_>>();
    files.sort_by(|(left_path, left_count), (right_path, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| path_sort_key(left_path).cmp(&path_sort_key(right_path)))
    });
    files.truncate(limit);
    files
}

fn path_sort_key(path: &Path) -> String {
    path.display().to_string()
}

fn short_commit_oid(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}

fn format_count(count: usize) -> String {
    let digits = count.to_string();
    let first_group_len = digits.len() % 3;
    let first_group_len = if first_group_len == 0 {
        3
    } else {
        first_group_len
    };
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    formatted.push_str(&digits[..first_group_len]);
    for chunk_start in (first_group_len..digits.len()).step_by(3) {
        formatted.push(',');
        formatted.push_str(&digits[chunk_start..chunk_start + 3]);
    }
    formatted
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else if limit <= 3 {
        ".".repeat(limit)
    } else {
        format!("{}...", text.chars().take(limit - 3).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command as ProcessCommand,
        time::{SystemTime, UNIX_EPOCH},
    };

    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::ir::{Author, ChangeKind, FileChange, RepoId};

    fn event(author: &str, secs: i64, paths: &[&str]) -> HistoryEvent {
        HistoryEvent {
            repo: RepoId::new("fixture"),
            commit_oid: format!("commit-{secs}"),
            author: Author::normalized(author, author),
            author_time: Utc.timestamp_opt(secs, 0).single().unwrap(),
            commit_time: Utc.timestamp_opt(secs, 0).single().unwrap(),
            changes: paths
                .iter()
                .map(|path| FileChange {
                    path: PathBuf::from(path),
                    kind: ChangeKind::Modify,
                    lines_added: 1,
                    lines_deleted: 0,
                })
                .collect(),
            message: "fixture".to_string(),
            tags: Vec::new(),
        }
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("reporeel-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git -C {} {} failed: {}",
            repo.display(),
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo_with_commit(repo: &Path, author_name: &str, author_email: &str, secs: i64) {
        fs::create_dir_all(repo).unwrap();
        let output = ProcessCommand::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        git(repo, &["config", "user.name", author_name]);
        git(repo, &["config", "user.email", author_email]);
        fs::write(repo.join("file.txt"), format!("{secs}\n")).unwrap();
        git(repo, &["add", "file.txt"]);

        let date = format!("@{secs} +0000");
        let output = ProcessCommand::new("git")
            .arg("-C")
            .arg(repo)
            .arg("commit")
            .arg("-q")
            .arg("-m")
            .arg(format!("commit {secs}"))
            .env("GIT_AUTHOR_NAME", author_name)
            .env("GIT_AUTHOR_EMAIL", author_email)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn playback_paths_builds_visible_union_and_recent_active_paths() {
        let events = vec![
            event("ada@example.com", 10, &["a.rs"]),
            event("lin@example.com", 20, &["b.rs"]),
            event("ada@example.com", 30, &["c.rs"]),
            event("ada@example.com", 40, &["d.rs"]),
        ];

        let paths = playback_paths(&events, Some("ada@example.com"), 2);

        assert_eq!(
            paths.visible_paths,
            HashSet::from([
                PathBuf::from("a.rs"),
                PathBuf::from("c.rs"),
                PathBuf::from("d.rs"),
            ])
        );
        assert_eq!(
            paths.active_paths,
            HashSet::from([PathBuf::from("c.rs"), PathBuf::from("d.rs")])
        );
    }

    #[test]
    fn load_track_merges_multiple_repos_by_author_time() {
        let root = temp_test_dir("merge-order");
        let repo_a = root.join("alpha");
        let repo_b = root.join("beta");
        init_repo_with_commit(&repo_a, "Ada", "ada@example.com", 30);
        init_repo_with_commit(&repo_b, "Lin", "lin@example.com", 10);

        let track = load_track(vec![repo_a, repo_b], None, None).unwrap();
        let times = track
            .git_events
            .iter()
            .map(|event| event.author_time.timestamp())
            .collect::<Vec<_>>();

        assert_eq!(times, vec![10, 30]);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn load_track_keeps_same_email_author_identity_across_repos() {
        let root = temp_test_dir("author-identity");
        let repo_a = root.join("alpha");
        let repo_b = root.join("beta");
        init_repo_with_commit(&repo_a, "Ada A", "ADA@example.com", 10);
        init_repo_with_commit(&repo_b, "Ada B", "ada@example.com", 20);

        let track = load_track(vec![repo_a, repo_b], None, None).unwrap();
        let authors = author_infos_from_events(&track.git_events);

        assert_eq!(authors.len(), 1);
        assert_eq!(authors[0].key, "ada@example.com");
        assert_eq!(authors[0].commits, 2);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn load_selected_file_diff_caches_diff_for_current_file_node() {
        let repo = temp_test_dir("selected-diff");
        init_repo_with_commit(&repo, "Ada", "ada@example.com", 30);

        fs::write(repo.join("file.txt"), "40\n").unwrap();
        git(&repo, &["add", "file.txt"]);
        let output = ProcessCommand::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("commit")
            .arg("-q")
            .arg("-m")
            .arg("modify")
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_AUTHOR_DATE", "@40 +0000")
            .env("GIT_COMMITTER_DATE", "@40 +0000")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let path = PathBuf::from("file.txt");
        let mut details = BTreeMap::from([(
            path.clone(),
            render::NodeDetail {
                path: path.clone(),
                is_dir: false,
                repo_path: Some(repo.clone()),
                heat: 2,
                child_file_count: 0,
                total_changes: 2,
                recent_commit: None,
                contributors: Vec::new(),
                diff: None,
            },
        )]);

        load_selected_file_diff(&mut details, Some(path.as_path())).unwrap();

        let diff = details
            .get(&path)
            .and_then(|detail| detail.diff.as_ref())
            .unwrap();
        assert_eq!(diff.added, vec!["40"]);
        assert_eq!(diff.removed, vec!["30"]);
        fs::remove_dir_all(repo).ok();
    }
}
