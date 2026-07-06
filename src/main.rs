use std::collections::{BTreeMap, BTreeSet};
use zellij_tile::prelude::*;

/// Blank lines above the tree / spaces to the left of each row (breathing room).
const TOP_PAD: usize = 1;
const LEFT_PAD: usize = 1;

/// Markers appended to tab names by the zellij-attention plugin. We read them
/// (global, consistent state that every sidebar instance sees identically),
/// strip them for a clean label, and render our own icon + group rollup.
const MARK_WAITING: &str = " ⏳";
const MARK_COMPLETED: &str = " ✅";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Attention {
    Waiting,
    Completed,
}

/// Waiting always wins over Completed when aggregating a group.
fn merge(acc: Option<Attention>, x: Attention) -> Attention {
    match (acc, x) {
        (Some(Attention::Waiting), _) | (_, Attention::Waiting) => Attention::Waiting,
        _ => Attention::Completed,
    }
}

/// Split a zellij-attention marker off a tab name: ("work:api ⏳") -> (Waiting, "work:api").
fn parse_attention(name: &str) -> (Option<Attention>, &str) {
    if let Some(base) = name.strip_suffix(MARK_WAITING) {
        (Some(Attention::Waiting), base)
    } else if let Some(base) = name.strip_suffix(MARK_COMPLETED) {
        (Some(Attention::Completed), base)
    } else {
        (None, name)
    }
}

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    collapsed: BTreeSet<String>,
    selected: usize,
    separator: char,
    waiting_icon: String,
    completed_icon: String,
}

register_plugin!(State);

enum Row {
    Group { name: String, collapsed: bool, count: usize, attention: Option<Attention> },
    Tab { position: usize, label: String, active: bool, attention: Option<Attention> },
}

impl State {
    /// Split a tab name into (group, label) on the first separator.
    fn group_of<'a>(&self, name: &'a str) -> (String, &'a str) {
        match name.find(self.separator) {
            Some(i) => (
                name[..i].to_string(),
                name[i + self.separator.len_utf8()..].trim_start(),
            ),
            None => ("General".to_string(), name),
        }
    }

    fn build_rows(&self) -> Vec<Row> {
        let mut order: Vec<String> = Vec::new();
        let mut groups: BTreeMap<String, Vec<(usize, String, bool, Option<Attention>)>> =
            BTreeMap::new();
        for t in &self.tabs {
            let (att, base) = parse_attention(&t.name);
            let (g, label) = self.group_of(base);
            if !order.contains(&g) {
                order.push(g.clone());
            }
            groups
                .entry(g)
                .or_default()
                .push((t.position, label.to_string(), t.active, att));
        }
        let mut rows = Vec::new();
        for g in &order {
            let items = groups.get(g).cloned().unwrap_or_default();
            let collapsed = self.collapsed.contains(g);
            let group_att = items.iter().fold(None, |acc, (_, _, _, a)| match a {
                Some(x) => Some(merge(acc, *x)),
                None => acc,
            });
            rows.push(Row::Group {
                name: g.clone(),
                collapsed,
                count: items.len(),
                attention: group_att,
            });
            if !collapsed {
                for (position, label, active, attention) in items {
                    rows.push(Row::Tab { position, label, active, attention });
                }
            }
        }
        rows
    }

    fn active_row_index(&self) -> Option<usize> {
        self.build_rows()
            .iter()
            .position(|r| matches!(r, Row::Tab { active: true, .. }))
    }

    /// Colored icon (with trailing space) for a row's attention state, or empty.
    /// `\e[39m` resets only the foreground so it nests inside the `\e[7m` highlight.
    fn icon(&self, att: Option<Attention>) -> String {
        match att {
            Some(Attention::Waiting) => format!("\u{1b}[33m{}\u{1b}[39m ", self.waiting_icon),
            Some(Attention::Completed) => format!("\u{1b}[32m{}\u{1b}[39m ", self.completed_icon),
            None => String::new(),
        }
    }

    fn activate(&mut self, idx: usize) -> bool {
        let rows = self.build_rows();
        if idx >= rows.len() {
            return false;
        }
        match &rows[idx] {
            Row::Group { name, .. } => {
                if self.collapsed.contains(name) {
                    self.collapsed.remove(name);
                } else {
                    self.collapsed.insert(name.clone());
                }
                true
            }
            Row::Tab { position, .. } => {
                switch_tab_to(*position as u32 + 1);
                true
            }
        }
    }

    fn handle_key(&mut self, key: KeyWithModifier) -> bool {
        let len = self.build_rows().len();
        if len == 0 {
            return false;
        }
        match key.bare_key {
            BareKey::Char('j') | BareKey::Down => {
                self.selected = (self.selected + 1).min(len - 1);
                true
            }
            BareKey::Char('k') | BareKey::Up => {
                self.selected = self.selected.saturating_sub(1);
                true
            }
            BareKey::Enter | BareKey::Char(' ') => {
                let sel = self.selected;
                self.activate(sel)
            }
            _ => false,
        }
    }

    fn handle_mouse(&mut self, mouse: Mouse) -> bool {
        let len = self.build_rows().len();
        if len == 0 {
            return false;
        }
        match mouse {
            Mouse::LeftClick(row, _col) => {
                let idx = row - TOP_PAD as isize;
                if idx < 0 || idx as usize >= len {
                    return false;
                }
                let idx = idx as usize;
                self.selected = idx;
                self.activate(idx);
                true
            }
            Mouse::ScrollUp(_) => {
                self.selected = self.selected.saturating_sub(1);
                true
            }
            Mouse::ScrollDown(_) => {
                self.selected = (self.selected + 1).min(len - 1);
                true
            }
            _ => false,
        }
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.separator = configuration
            .get("separator")
            .and_then(|s| s.chars().next())
            .unwrap_or(':');
        self.waiting_icon = configuration
            .get("waiting_icon")
            .cloned()
            .unwrap_or_else(|| "◆".to_string());
        self.completed_icon = configuration
            .get("completed_icon")
            .cloned()
            .unwrap_or_else(|| "✓".to_string());
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        subscribe(&[EventType::TabUpdate, EventType::Key, EventType::Mouse]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::TabUpdate(tabs) => {
                self.tabs = tabs;
                if let Some(i) = self.active_row_index() {
                    self.selected = i;
                } else {
                    let len = self.build_rows().len();
                    if len > 0 && self.selected >= len {
                        self.selected = len - 1;
                    }
                }
                true
            }
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, cols: usize) {
        let visible = self.build_rows();
        let pad = " ".repeat(LEFT_PAD);
        let mut out = String::new();
        for _ in 0..TOP_PAD {
            out.push_str("\r\n");
        }
        if visible.is_empty() {
            out.push_str(&format!("{}\u{1b}[2m(no tabs)\u{1b}[0m", pad));
            print!("{}", out);
            return;
        }
        for (i, row) in visible.iter().enumerate() {
            let core = match row {
                Row::Group { name, collapsed, count, attention } => {
                    let disc = if *collapsed { "▸" } else { "▾" };
                    let icon = if *collapsed { self.icon(*attention) } else { String::new() };
                    format!("{} {}{} ({})", disc, icon, name, count)
                }
                Row::Tab { label, active, attention, .. } => {
                    let dot = if *active { "●" } else { " " };
                    format!("  {} {}{}", dot, self.icon(*attention), label)
                }
            };
            let line = truncate(&format!("{}{}", pad, core), cols);
            if i == self.selected {
                let w = line.chars().count();
                let bar = if w < cols {
                    format!("{}{}", line, " ".repeat(cols - w))
                } else {
                    line
                };
                out.push_str(&format!("\u{1b}[7m{}\u{1b}[0m", bar));
            } else {
                out.push_str(&line);
            }
            out.push_str("\r\n");
        }
        print!("{}", out);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let keep = max.saturating_sub(1);
        s.chars().take(keep).collect::<String>() + "…"
    }
}
