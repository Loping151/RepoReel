use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

use crate::ir::HistoryEvent;

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutNode {
    pub path: PathBuf,
    pub depth: u32,
    pub row: u32,
    pub x: f64,
    pub y: f64,
    pub heat: u32,
    pub is_dir: bool,
    pub children: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub nodes: Vec<LayoutNode>,
    pub total_rows: u32,
    pub by_path: HashMap<PathBuf, usize>,
    // Row budget belongs in layout because directory aggregation/sampling must happen before render traversal.
    pub max_visible_rows: u32,
}

pub fn build_layout(events: &[HistoryEvent]) -> Layout {
    let mut entries = BTreeMap::new();
    let mut heat = HashMap::new();
    for event in events {
        for change in &event.changes {
            insert_path(&mut entries, &change.path);
            record_heat(&mut heat, &change.path);
        }
    }

    let mut nodes = Vec::with_capacity(entries.len());
    let mut by_path = HashMap::with_capacity(entries.len());

    for (path, is_dir) in entries {
        let index = nodes.len();
        let depth = path_depth(&path);
        by_path.insert(path.clone(), index);
        nodes.push(LayoutNode {
            path,
            depth,
            row: 0,
            x: 0.5,
            y: 0.5,
            heat: 0,
            is_dir,
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

    let roots: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let has_parent = parent_path(&node.path)
                .as_ref()
                .is_some_and(|parent| by_path.contains_key(parent));
            (!has_parent).then_some(index)
        })
        .collect();

    let mut next_row = 0;
    for root in roots.iter().copied() {
        assign_rows(root, &mut nodes, &mut next_row);
    }
    for node in &mut nodes {
        node.heat = heat.get(&node.path).copied().unwrap_or(0);
    }
    assign_spatial_coords(&mut nodes, &roots, 1.0, 1.0);

    let total_rows = u32::try_from(nodes.len()).unwrap_or(u32::MAX);
    Layout {
        nodes,
        total_rows,
        by_path,
        max_visible_rows: total_rows,
    }
}

pub fn assign_spatial_coords(
    nodes: &mut [LayoutNode],
    roots: &[usize],
    canvas_width: f64,
    canvas_height: f64,
) {
    if nodes.is_empty() || roots.is_empty() {
        return;
    }

    let weights = subtree_weights(nodes);
    let max_depth = nodes
        .iter()
        .map(|node| node.depth)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let max_radial_depth = if roots.len() == 1 {
        max_depth.max(1)
    } else {
        max_depth.saturating_add(1).max(1)
    };

    let center_x = canvas_width * 0.5;
    let center_y = canvas_height * 0.54;
    let radius_x = canvas_width * 0.46;
    let radius_y = canvas_height * 0.38;

    if roots.len() == 1 {
        let root = roots[0];
        nodes[root].x = center_x;
        nodes[root].y = center_y;
        assign_child_coords(
            root,
            0.0,
            std::f64::consts::TAU,
            1,
            max_radial_depth,
            nodes,
            &weights,
            (center_x, center_y),
            (radius_x, radius_y),
        );
        distribute_rows_vertically(nodes, canvas_height);
        return;
    }

    let total_weight = roots
        .iter()
        .map(|index| weights[*index])
        .sum::<u32>()
        .max(1);
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for root in roots {
        let span = std::f64::consts::TAU * f64::from(weights[*root]) / f64::from(total_weight);
        let center_angle = angle + span * 0.5;
        place_node(
            *root,
            center_angle,
            1,
            max_radial_depth,
            nodes,
            (center_x, center_y),
            (radius_x, radius_y),
        );
        assign_child_coords(
            *root,
            angle,
            span,
            2,
            max_radial_depth,
            nodes,
            &weights,
            (center_x, center_y),
            (radius_x, radius_y),
        );
        angle += span;
    }

    distribute_rows_vertically(nodes, canvas_height);
}

fn insert_path(entries: &mut BTreeMap<PathBuf, bool>, path: &Path) {
    let parts = path_parts(path);
    if parts.is_empty() {
        return;
    }

    let mut current = PathBuf::new();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let is_dir = index + 1 < parts.len();
        entries
            .entry(current.clone())
            .and_modify(|stored_is_dir| *stored_is_dir |= is_dir)
            .or_insert(is_dir);
    }
}

fn record_heat(heat: &mut HashMap<PathBuf, u32>, path: &Path) {
    let parts = path_parts(path);
    let mut current = PathBuf::new();
    for part in parts {
        current.push(part);
        *heat.entry(current.clone()).or_default() += 1;
    }
}

fn path_parts(path: &Path) -> Vec<OsString> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_os_string()),
            Component::ParentDir => Some(OsString::from("..")),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
        })
        .collect()
}

fn path_depth(path: &Path) -> u32 {
    path_parts(path)
        .len()
        .saturating_sub(1)
        .try_into()
        .unwrap_or(u32::MAX)
}

fn parent_path(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    (!parent.as_os_str().is_empty()).then(|| parent.to_path_buf())
}

fn assign_rows(index: usize, nodes: &mut [LayoutNode], next_row: &mut u32) {
    nodes[index].row = *next_row;
    *next_row = next_row.saturating_add(1);

    let children = nodes[index].children.clone();
    for child in children {
        assign_rows(child, nodes, next_row);
    }
}

fn subtree_weights(nodes: &[LayoutNode]) -> Vec<u32> {
    let mut weights = vec![0; nodes.len()];
    for index in (0..nodes.len()).rev() {
        let weight = nodes[index]
            .children
            .iter()
            .map(|child| weights[*child])
            .sum::<u32>()
            .max(1);
        weights[index] = weight;
    }
    weights
}

#[allow(clippy::too_many_arguments)]
fn assign_child_coords(
    parent: usize,
    start_angle: f64,
    span: f64,
    radial_depth: u32,
    max_radial_depth: u32,
    nodes: &mut [LayoutNode],
    weights: &[u32],
    center: (f64, f64),
    radius: (f64, f64),
) {
    let children = nodes[parent].children.clone();
    if children.is_empty() {
        return;
    }

    let total_weight = children
        .iter()
        .map(|child| weights[*child])
        .sum::<u32>()
        .max(1);
    let mut angle = start_angle;

    for child in children {
        let child_span = span * f64::from(weights[child]) / f64::from(total_weight);
        let center_angle = angle + child_span * 0.5;
        place_node(
            child,
            center_angle,
            radial_depth,
            max_radial_depth,
            nodes,
            center,
            radius,
        );
        assign_child_coords(
            child,
            angle,
            child_span,
            radial_depth.saturating_add(1),
            max_radial_depth,
            nodes,
            weights,
            center,
            radius,
        );
        angle += child_span;
    }
}

fn place_node(
    index: usize,
    angle: f64,
    radial_depth: u32,
    max_radial_depth: u32,
    nodes: &mut [LayoutNode],
    center: (f64, f64),
    radius: (f64, f64),
) {
    let amount = f64::from(radial_depth) / f64::from(max_radial_depth.max(1));
    nodes[index].x = (center.0 + angle.cos() * radius.0 * amount).clamp(0.02, 0.98);
    nodes[index].y = (center.1 + angle.sin() * radius.1 * amount).clamp(0.10, 0.95);
}

fn distribute_rows_vertically(nodes: &mut [LayoutNode], canvas_height: f64) {
    if nodes.is_empty() {
        return;
    }

    if nodes.len() == 1 {
        nodes[0].y = canvas_height * 0.5;
        return;
    }

    let max_row = nodes.iter().map(|node| node.row).max().unwrap_or(0).max(1);
    for node in nodes {
        // Render maps normalized y into the tree rectangle; row-based y keeps
        // the final tree using the whole title/status-free vertical budget.
        node.y = (f64::from(node.row) / f64::from(max_row) * canvas_height).clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::ir::{Author, ChangeKind, FileChange, HistoryEvent, RepoId};

    fn event(paths: &[&str]) -> HistoryEvent {
        HistoryEvent {
            repo: RepoId::new("fixture"),
            commit_oid: "abc123".to_string(),
            author: Author::normalized("Ada", "ada@example.com"),
            author_time: Utc.timestamp_opt(0, 0).single().unwrap(),
            commit_time: Utc.timestamp_opt(0, 0).single().unwrap(),
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

    #[test]
    fn builds_full_historical_tree() {
        let layout = build_layout(&[event(&["src/main.rs", "src/lib/mod.rs", "README.md"])]);

        assert_eq!(layout.nodes.len(), 5);
        assert_eq!(layout.total_rows, 5);
        assert!(layout.by_path.contains_key(&PathBuf::from("README.md")));
        assert!(layout.by_path.contains_key(&PathBuf::from("src")));
        assert!(layout.by_path.contains_key(&PathBuf::from("src/lib")));
        assert!(
            layout
                .by_path
                .contains_key(&PathBuf::from("src/lib/mod.rs"))
        );

        let src = layout.by_path[&PathBuf::from("src")];
        let main = layout.by_path[&PathBuf::from("src/main.rs")];
        assert!(layout.nodes[src].is_dir);
        assert_eq!(layout.nodes[src].depth, 0);
        assert_eq!(layout.nodes[main].depth, 1);
        assert!(layout.nodes[src].children.contains(&main));
    }

    #[test]
    fn assigns_stable_rows_from_path_set() {
        let first = build_layout(&[event(&["src/main.rs"]), event(&["README.md", "src/lib.rs"])]);
        let second = build_layout(&[event(&["src/lib.rs", "README.md"]), event(&["src/main.rs"])]);

        for path in ["README.md", "src", "src/lib.rs", "src/main.rs"] {
            let path = PathBuf::from(path);
            let first_node = &first.nodes[first.by_path[&path]];
            let second_node = &second.nodes[second.by_path[&path]];
            assert_eq!(first_node.row, second_node.row);
            assert_eq!(first_node.x, second_node.x);
            assert_eq!(first_node.y, second_node.y);
            assert_eq!(first_node.depth, second_node.depth);
            assert_eq!(first_node.is_dir, second_node.is_dir);
        }
    }

    #[test]
    fn assigns_heat_to_changed_paths_and_ancestors() {
        let layout = build_layout(&[
            event(&["src/main.rs"]),
            event(&["src/main.rs", "src/lib.rs"]),
        ]);

        let src = layout.by_path[&PathBuf::from("src")];
        let main = layout.by_path[&PathBuf::from("src/main.rs")];
        let lib = layout.by_path[&PathBuf::from("src/lib.rs")];

        assert_eq!(layout.nodes[src].heat, 3);
        assert_eq!(layout.nodes[main].heat, 2);
        assert_eq!(layout.nodes[lib].heat, 1);
    }
}
