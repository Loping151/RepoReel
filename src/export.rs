use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::File,
    io::{ErrorKind, Write},
    path::PathBuf,
};

use anyhow::{Result, bail};
use chrono::{TimeZone, Utc};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::{
    font::{Rgb, render_char},
    ir::{Author, ChangeKind, FileChange, HistoryEvent, RepoId},
    layout::{Layout, build_layout},
    render::{
        AuthorInfo, SceneState, author_infos_from_events, build_pulses, draw_scene_to_buffer,
        hot_files_from_events, node_details_from_events, repo_title_from_events,
        span_text_from_events,
    },
    timeline::TimelinePlayer,
};

const DEFAULT_TAIL_FRAMES: usize = 6;
const PULSE_EVENT_COUNT: usize = 5;
const HOT_FILE_LIMIT: usize = 32;
const CELL_W: u16 = 12;
const CELL_H: u16 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    pub width: u16,
    pub height: u16,
    pub fps: u16,
    pub max_frames: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFrame {
    pub buffer: Buffer,
}

pub fn export_gif(events: Vec<HistoryEvent>, options: ExportOptions, out: PathBuf) -> Result<()> {
    eprintln!(
        "reporeel: rendering {} events → up to {} frames…",
        events.len(),
        options.max_frames
    );
    let frames = render_frames(&events, options);
    let out_display = out.display().to_string();
    let width = options.width.max(1);
    let height = options.height.max(1);
    let pixel_width = width.saturating_mul(CELL_W);
    let pixel_height = height.saturating_mul(CELL_H);
    eprintln!(
        "reporeel: encoding {} frames ({}x{} cells → {}x{} px) → {}",
        frames.len(),
        width,
        height,
        pixel_width,
        pixel_height,
        out_display
    );
    write_gif(&frames, options, out)?;
    eprintln!("reporeel: done → {}", out_display);
    eprintln!("reporeel: open the GIF to view it.");
    Ok(())
}

pub fn render_frames(events: &[HistoryEvent], options: ExportOptions) -> Vec<RenderedFrame> {
    let width = options.width.max(1);
    let height = options.height.max(1);
    let area = Rect::new(0, 0, width, height);
    let layout = build_layout(events);
    let authors = author_infos_from_events(events);
    let repo_title = repo_title_from_events(events);
    let span_text = span_text_from_events(events);
    let mut player = TimelinePlayer::new(events.to_vec());
    let total = player.total();
    // Sample cursor positions so the frame count stays bounded regardless of repo
    // size. Without this a 9k-commit repo yields a 27MB / 9k-frame GIF.
    let steps = total.saturating_add(1);
    let max_frames = options.max_frames.max(1);
    let sampled: Vec<usize> = if steps <= max_frames {
        (0..steps).collect()
    } else {
        (0..max_frames)
            .map(|i| (i.saturating_mul(steps) / max_frames).min(total))
            .collect()
    };
    let mut frames = Vec::with_capacity(sampled.len().saturating_add(DEFAULT_TAIL_FRAMES));

    let render_total = sampled.len();
    for (index, cursor) in sampled.into_iter().enumerate() {
        player.seek_to_event(cursor);
        frames.push(render_player_frame(
            &layout,
            &player,
            area,
            &authors,
            &repo_title,
            &span_text,
        ));
        let rendered = index + 1;
        if rendered % 10 == 0 || rendered == render_total {
            eprint!("\rreporeel: rendering frame {rendered}/{render_total}...");
        }
    }

    if let Some(last) = frames.last().cloned() {
        for _ in 0..DEFAULT_TAIL_FRAMES {
            frames.push(last.clone());
        }
    }

    frames
}

pub fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Reset => (0, 0, 0),
        Color::Black => (0, 0, 0),
        Color::Red => (205, 49, 49),
        Color::Green => (13, 188, 121),
        Color::Yellow => (229, 229, 16),
        Color::Blue => (36, 114, 200),
        Color::Magenta => (188, 63, 188),
        Color::Cyan => (17, 168, 205),
        Color::Gray => (229, 229, 229),
        Color::DarkGray => (102, 102, 102),
        Color::LightRed => (241, 76, 76),
        Color::LightGreen => (35, 209, 139),
        Color::LightYellow => (245, 245, 67),
        Color::LightBlue => (59, 142, 234),
        Color::LightMagenta => (214, 112, 214),
        Color::LightCyan => (41, 184, 219),
        Color::White => (255, 255, 255),
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(index) => indexed_color_to_rgb(index),
    }
}

pub fn synthetic_demo_events() -> Vec<HistoryEvent> {
    let repo = RepoId::new("reporeel-demo");
    let base = 1_704_067_200;
    let specs = [
        (
            "Ada Lovelace",
            "ada@example.com",
            "0001",
            "initial project skeleton",
            &[
                ("README.md", ChangeKind::Add, 42, 0),
                ("Cargo.toml", ChangeKind::Add, 18, 0),
                ("src/main.rs", ChangeKind::Add, 64, 0),
            ][..],
        ),
        (
            "Lin Chen",
            "lin@example.com",
            "0002",
            "add git log source",
            &[
                ("src/source.rs", ChangeKind::Add, 141, 0),
                ("src/main.rs", ChangeKind::Modify, 22, 4),
            ][..],
        ),
        (
            "Maya Patel",
            "maya@example.com",
            "0003",
            "define timeline ir",
            &[("src/ir.rs", ChangeKind::Add, 92, 0)][..],
        ),
        (
            "Ada Lovelace",
            "ada@example.com",
            "0004",
            "parse file changes",
            &[
                ("src/source.rs", ChangeKind::Modify, 88, 12),
                ("tests/source.rs", ChangeKind::Add, 57, 0),
            ][..],
        ),
        (
            "Noah Kim",
            "noah@example.com",
            "0005",
            "build stable layout",
            &[("src/layout.rs", ChangeKind::Add, 123, 0)][..],
        ),
        (
            "Maya Patel",
            "maya@example.com",
            "0006",
            "draw tree scene",
            &[
                ("src/render.rs", ChangeKind::Add, 178, 0),
                ("src/main.rs", ChangeKind::Modify, 31, 6),
            ][..],
        ),
        (
            "Lin Chen",
            "lin@example.com",
            "0007",
            "wire timeline player",
            &[
                ("src/timeline.rs", ChangeKind::Add, 73, 0),
                ("src/main.rs", ChangeKind::Modify, 48, 9),
            ][..],
        ),
        (
            "Ada Lovelace",
            "ada@example.com",
            "0008",
            "add keyboard controls",
            &[("src/main.rs", ChangeKind::Modify, 84, 18)][..],
        ),
        (
            "Noah Kim",
            "noah@example.com",
            "0009",
            "split render tests",
            &[
                ("src/render.rs", ChangeKind::Modify, 34, 3),
                ("tests/render.rs", ChangeKind::Add, 88, 0),
            ][..],
        ),
        (
            "Maya Patel",
            "maya@example.com",
            "0010",
            "document playback",
            &[
                ("README.md", ChangeKind::Modify, 52, 8),
                ("docs/playback.md", ChangeKind::Add, 76, 0),
            ][..],
        ),
        (
            "Lin Chen",
            "lin@example.com",
            "0011",
            "refactor modules",
            &[
                ("src/app.rs", ChangeKind::Add, 96, 0),
                ("src/main.rs", ChangeKind::Modify, 14, 77),
            ][..],
        ),
        (
            "Ada Lovelace",
            "ada@example.com",
            "0012",
            "add export buffer path",
            &[("src/export.rs", ChangeKind::Add, 132, 0)][..],
        ),
        (
            "Noah Kim",
            "noah@example.com",
            "0013",
            "polish ci workflow",
            &[(".github/workflows/ci.yml", ChangeKind::Add, 44, 0)][..],
        ),
        (
            "Maya Patel",
            "maya@example.com",
            "0014",
            "prepare release docs",
            &[
                ("CHANGELOG.md", ChangeKind::Add, 39, 0),
                ("README.md", ChangeKind::Modify, 28, 6),
            ][..],
        ),
        (
            "Lin Chen",
            "lin@example.com",
            "0015",
            "tag v0.1.0",
            &[
                ("Cargo.toml", ChangeKind::Modify, 2, 2),
                ("README.md", ChangeKind::Modify, 10, 1),
            ][..],
        ),
    ];

    specs
        .iter()
        .enumerate()
        .map(
            |(index, (name, email, oid, message, changes))| HistoryEvent {
                repo: repo.clone(),
                commit_oid: format!("demo-{oid}"),
                author: Author::normalized(name, email),
                author_time: Utc
                    .timestamp_opt(base + i64::try_from(index).unwrap() * 86_400, 0)
                    .single()
                    .unwrap(),
                commit_time: Utc
                    .timestamp_opt(base + i64::try_from(index).unwrap() * 86_400 + 300, 0)
                    .single()
                    .unwrap(),
                changes: changes
                    .iter()
                    .map(|(path, kind, lines_added, lines_deleted)| FileChange {
                        path: PathBuf::from(path),
                        kind: kind.clone(),
                        lines_added: *lines_added,
                        lines_deleted: *lines_deleted,
                    })
                    .collect(),
                message: (*message).to_string(),
                tags: (index == specs.len() - 1)
                    .then(|| "v0.1.0".to_string())
                    .into_iter()
                    .collect(),
            },
        )
        .collect()
}

fn render_player_frame(
    layout: &Layout,
    player: &TimelinePlayer,
    area: Rect,
    authors: &[AuthorInfo],
    repo_title: &str,
    span_text: &str,
) -> RenderedFrame {
    let paths = playback_paths(player.events_up_to_cursor(), 3);
    let pulses = build_pulses(
        player.events_up_to_cursor(),
        layout,
        authors,
        PULSE_EVENT_COUNT,
        &paths.0,
        &paths.1,
        false,
    );
    let hot_files = hot_files_from_events(player.events_up_to_cursor(), None, HOT_FILE_LIMIT);
    let node_details = node_details_from_events(player.events_up_to_cursor(), layout);
    let scene = SceneState {
        layout: layout.clone(),
        repo_title: repo_title.to_string(),
        span_text: span_text.to_string(),
        focus: None,
        visible_paths: paths.0,
        active_paths: paths.1,
        hot_files,
        authors: authors.to_vec(),
        pulses,
        now: player.cursor_time(),
        cursor: player.cursor(),
        total: player.total(),
        speed: "1.0x".to_string(),
        paused: true,
        show_help: false,
        show_authors: false,
        cursor_path: None,
        nav_mode: false,
        diff_open: false,
        diff_scroll: 0,
        author_scroll: 0,
        node_details,
    };
    let mut buffer = Buffer::empty(area);
    draw_scene_to_buffer(&scene, area, &mut buffer);
    RenderedFrame { buffer }
}

fn playback_paths(
    events: &[HistoryEvent],
    active_event_count: usize,
) -> (HashSet<PathBuf>, HashSet<PathBuf>) {
    let visible_paths = events
        .iter()
        .flat_map(|event| event.changes.iter().map(|change| change.path.clone()))
        .collect();
    let active_paths = events
        .iter()
        .rev()
        .take(active_event_count)
        .flat_map(|event| event.changes.iter().map(|change| change.path.clone()))
        .collect();

    (visible_paths, active_paths)
}

fn write_gif(frames: &[RenderedFrame], options: ExportOptions, out: PathBuf) -> Result<()> {
    if frames.is_empty() {
        bail!("cannot export GIF without frames");
    }

    let width = options.width.max(1);
    let height = options.height.max(1);
    let pixel_width = width
        .checked_mul(CELL_W)
        .ok_or_else(|| anyhow::anyhow!("GIF width {} cells exceeds export pixel limit", width))?;
    let pixel_height = height
        .checked_mul(CELL_H)
        .ok_or_else(|| anyhow::anyhow!("GIF height {} cells exceeds export pixel limit", height))?;
    let rgb_frames = frames
        .iter()
        .map(|frame| buffer_to_pixels(&frame.buffer, width, height))
        .collect::<Vec<_>>();
    let palette = build_palette(&rgb_frames)?;
    let palette_index = palette
        .iter()
        .enumerate()
        .map(|(index, rgb)| (*rgb, u8::try_from(index).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let palette_size = palette_table_size(palette.len());
    let min_code_size = lzw_min_code_size(palette_size);
    let delay = gif_delay(options.fps);

    let mut image = match File::create(&out) {
        Ok(image) => image,
        Err(error) => {
            if out.is_dir() || error.kind() == ErrorKind::IsADirectory {
                bail!(
                    "output path {} is a directory; specify a filename (e.g. hero.gif)",
                    out.display()
                );
            }
            let parent_missing = out
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .is_some_and(|parent| !parent.exists());
            if parent_missing || error.kind() == ErrorKind::NotFound {
                bail!(
                    "cannot create output file {}: directory does not exist; create it first",
                    out.display()
                );
            }
            bail!("cannot create output file {}: {}", out.display(), error);
        }
    };
    image.write_all(b"GIF89a")?;
    write_u16_le(&mut image, pixel_width)?;
    write_u16_le(&mut image, pixel_height)?;
    let size_code = palette_size.trailing_zeros() as u8 - 1;
    image.write_all(&[0b1000_0000 | 0b0111_0000 | size_code, 0, 0])?;
    for rgb in &palette {
        image.write_all(&[rgb.0, rgb.1, rgb.2])?;
    }
    for _ in palette.len()..palette_size {
        image.write_all(&[0, 0, 0])?;
    }
    image.write_all(&[0x21, 0xff, 0x0b])?;
    image.write_all(b"NETSCAPE2.0")?;
    image.write_all(&[0x03, 0x01, 0x00, 0x00, 0x00])?;

    for rgb in rgb_frames {
        image.write_all(&[0x21, 0xf9, 0x04, 0x00])?;
        write_u16_le(&mut image, delay)?;
        image.write_all(&[0x00, 0x00])?;
        image.write_all(&[0x2c])?;
        write_u16_le(&mut image, 0)?;
        write_u16_le(&mut image, 0)?;
        write_u16_le(&mut image, pixel_width)?;
        write_u16_le(&mut image, pixel_height)?;
        image.write_all(&[0x00, min_code_size])?;

        let indices = rgb
            .iter()
            .map(|color| palette_index[color])
            .collect::<Vec<_>>();
        write_sub_blocks(&mut image, &lzw_passthrough(&indices, min_code_size))?;
    }
    image.write_all(&[0x3b])?;

    Ok(())
}

fn buffer_to_pixels(buffer: &Buffer, width: u16, height: u16) -> Vec<Rgb> {
    let pixel_width = usize::from(width) * usize::from(CELL_W);
    let pixel_height = usize::from(height) * usize::from(CELL_H);
    let mut pixels = vec![(0, 0, 0); pixel_width * pixel_height];
    let cell_width = usize::from(CELL_W);
    let cell_height = usize::from(CELL_H);
    let mut cell_pixels = vec![(0, 0, 0); cell_width * cell_height];

    for y in 0..height {
        for x in 0..width {
            let cell = &buffer[(x, y)];
            let style = cell.style();
            let bg = color_to_rgb(style.bg.unwrap_or(Color::Black));
            let fg = color_to_rgb(style.fg.unwrap_or(Color::White));
            let ch = cell.symbol().chars().next().unwrap_or(' ');
            render_char(ch, fg, bg, &mut cell_pixels, cell_width, cell_height);

            let start_x = usize::from(x) * usize::from(CELL_W);
            let start_y = usize::from(y) * usize::from(CELL_H);
            for row in 0..cell_height {
                let dst_start = (start_y + row) * pixel_width + start_x;
                let src_start = row * cell_width;
                pixels[dst_start..dst_start + cell_width]
                    .copy_from_slice(&cell_pixels[src_start..src_start + cell_width]);
            }
        }
    }

    debug_assert_eq!(pixels.len(), pixel_width * pixel_height);
    pixels
}

fn gif_delay(fps: u16) -> u16 {
    let fps = fps.max(1);
    ((100 + fps / 2) / fps).max(1)
}

fn build_palette(frames: &[Vec<(u8, u8, u8)>]) -> Result<Vec<(u8, u8, u8)>> {
    let colors = frames
        .iter()
        .flat_map(|frame| frame.iter().copied())
        .collect::<BTreeSet<_>>();
    if colors.len() > 256 {
        bail!("GIF export supports up to 256 colors, got {}", colors.len());
    }

    let mut palette = colors.into_iter().collect::<Vec<_>>();
    if !palette.contains(&(0, 0, 0)) {
        palette.insert(0, (0, 0, 0));
    }
    if palette.len() == 1 {
        palette.push((255, 255, 255));
    }
    Ok(palette)
}

fn palette_table_size(color_count: usize) -> usize {
    color_count.next_power_of_two().clamp(2, 256)
}

fn lzw_min_code_size(palette_size: usize) -> u8 {
    let bits = usize::BITS - (palette_size - 1).leading_zeros();
    u8::try_from(bits.max(2)).unwrap()
}

fn lzw_passthrough(indices: &[u8], min_code_size: u8) -> Vec<u8> {
    let clear_code = 1_u16 << min_code_size;
    let end_code = clear_code + 1;
    let code_width = min_code_size + 1;
    let mut writer = BitWriter::default();

    for index in indices {
        writer.write(clear_code, code_width);
        writer.write(u16::from(*index), code_width);
    }
    writer.write(clear_code, code_width);
    writer.write(end_code, code_width);
    writer.finish()
}

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    current: u32,
    used_bits: u8,
}

impl BitWriter {
    fn write(&mut self, code: u16, width: u8) {
        self.current |= u32::from(code) << self.used_bits;
        self.used_bits += width;

        while self.used_bits >= 8 {
            self.bytes.push((self.current & 0xff) as u8);
            self.current >>= 8;
            self.used_bits -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used_bits > 0 {
            self.bytes.push(self.current as u8);
        }
        self.bytes
    }
}

fn write_sub_blocks(mut writer: impl Write, bytes: &[u8]) -> std::io::Result<()> {
    for chunk in bytes.chunks(255) {
        writer.write_all(&[u8::try_from(chunk.len()).unwrap()])?;
        writer.write_all(chunk)?;
    }
    writer.write_all(&[0])
}

fn write_u16_le(mut writer: impl Write, value: u16) -> std::io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn indexed_color_to_rgb(index: u8) -> (u8, u8, u8) {
    const ANSI_16: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];

    match index {
        0..=15 => ANSI_16[usize::from(index)],
        16..=231 => {
            let n = index - 16;
            let component = |value: u8| if value == 0 { 0 } else { value * 40 + 55 };
            (component(n / 36), component((n % 36) / 6), component(n % 6))
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            (gray, gray, gray)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ratatui::style::Style;

    use super::*;

    #[test]
    fn maps_named_and_rgb_colors() {
        assert_eq!(color_to_rgb(Color::Black), (0, 0, 0));
        assert_eq!(color_to_rgb(Color::White), (255, 255, 255));
        assert_eq!(color_to_rgb(Color::LightYellow), (245, 245, 67));
        assert_eq!(color_to_rgb(Color::Rgb(1, 2, 3)), (1, 2, 3));
        assert_eq!(color_to_rgb(Color::Indexed(16)), (0, 0, 0));
        assert_eq!(color_to_rgb(Color::Indexed(231)), (255, 255, 255));
    }

    #[test]
    fn synthetic_demo_has_expected_shape() {
        let events = synthetic_demo_events();
        let authors = events
            .iter()
            .map(|event| event.author.identity_key().to_string())
            .collect::<HashSet<_>>();

        assert_eq!(events.len(), 15);
        assert_eq!(authors.len(), 4);
        assert!(events.iter().any(|event| event.tags.as_slice() == ["v0.1.0"]));
    }

    #[test]
    fn render_frames_produces_non_empty_buffers_for_small_input() {
        let events = synthetic_demo_events()
            .into_iter()
            .take(2)
            .collect::<Vec<_>>();
        let frames = render_frames(
            &events,
            ExportOptions {
                width: 40,
                height: 12,
                fps: 15,
                max_frames: 90,
            },
        );

        assert!(frames.len() > events.len());
        assert!(frames.iter().any(|frame| {
            frame
                .buffer
                .content()
                .iter()
                .any(|cell| !cell.symbol().trim().is_empty())
        }));
    }

    #[test]
    fn buffer_to_pixels_renders_text_ink_over_background() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(area);
        buffer[(0, 0)]
            .set_symbol("R")
            .set_style(Style::default().fg(Color::White).bg(Color::Black));

        let pixels = buffer_to_pixels(&buffer, 1, 1);

        assert!(pixels.contains(&(255, 255, 255)));
        assert!(pixels.contains(&(0, 0, 0)));
    }
}
