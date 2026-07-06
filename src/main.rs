use std::collections::{BTreeMap, BTreeSet};
use zellij_tile::prelude::*;

/// Blank lines above the tree / spaces to the left of each row (breathing room).
const TOP_PAD: usize = 1;
const LEFT_PAD: usize = 1;

/// Attention is encoded as a suffix on the tab NAME — global state that every
/// sidebar instance reads identically (no per-instance divergence). We add it on
/// the attention pipe and strip it when the tab is focused; both are `rename_tab`
/// calls that mutate the shared tab name.
const MARK_WAITING: &str = " ⏳";
const MARK_COMPLETED: &str = " ✅";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Attention {
    Waiting,
    Completed,
}

fn merge(acc: Option<Attention>, x: Attention) -> Attention {
    match (acc, x) {
        (Some(Attention::Waiting), _) | (_, Attention::Waiting) => Attention::Waiting,
        _ => Attention::Completed,
    }
}

/// Split the attention marker off a tab name: "work:api ⏳" -> (Waiting, "work:api").
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
    /// terminal pane id -> tab position (rebuilt on each PaneUpdate)
    pane_tab: BTreeMap<u32, usize>,
    /// active tab position at the last TabUpdate — used to distinguish a real
    /// tab *switch* (should clear) from a rename-triggered TabUpdate (should not).
    last_active: Option<usize>,
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

    fn icon(&self, att: Option<Attention>) -> String {
        match att {
            Some(Attention::Waiting) => format!("\u{1b}[33m{}\u{1b}[39m ", self.waiting_icon),
            Some(Attention::Completed) => format!("\u{1b}[32m{}\u{1b}[39m ", self.completed_icon),
            None => String::new(),
        }
    }

    /// Add an attention marker to the tab containing `pane_id` (global rename).
    fn set_attention(&self, pane_id: u32, marker: &str) {
        let target = self.pane_tab.get(&pane_id).and_then(|&tab_pos| {
            self.tabs.iter().find(|t| t.position == tab_pos).map(|t| {
                let (_, base) = parse_attention(&t.name);
                (tab_pos as u32 + 1, format!("{}{}", base, marker))
            })
        });
        if let Some((pos, name)) = target {
            rename_tab(pos, name);
        }
    }

    /// Strip the attention marker from the active tab (global rename) — this is
    /// the "clear on focus" path, keyed on the focused TAB not pane focus.
    fn clear_active_tab(&self) {
        let target = self.tabs.iter().find(|t| t.active).and_then(|t| {
            let (att, base) = parse_attention(&t.name);
            att.map(|_| (t.position as u32 + 1, base.to_string()))
        });
        if let Some((pos, name)) = target {
            rename_tab(pos, name);
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
            PermissionType::ReadCliPipes,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::Key,
            EventType::Mouse,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::TabUpdate(tabs) => {
                self.tabs = tabs;
                // Clear on focus, but ONLY when the active tab actually changed
                // (a real switch) — not on the TabUpdate our own rename triggers,
                // which would instantly strip a marker set on the current tab.
                let active = self.tabs.iter().find(|t| t.active).map(|t| t.position);
                if active != self.last_active {
                    self.last_active = active;
                    self.clear_active_tab();
                }
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
            Event::PaneUpdate(manifest) => {
                self.pane_tab.clear();
                for (tab_pos, panes) in manifest.panes {
                    for p in panes {
                        if !p.is_plugin {
                            self.pane_tab.insert(p.id, tab_pos);
                        }
                    }
                }
                false
            }
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => false,
        }
    }

    /// Attention signals: `zellij-vtabs::waiting|completed::<pane_id>` (broadcast CLI pipe).
    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        if let Some(rest) = pipe_message.name.strip_prefix("zellij-vtabs::") {
            let parts: Vec<&str> = rest.split("::").collect();
            if parts.len() == 2 {
                if let Ok(pane_id) = parts[1].parse::<u32>() {
                    match parts[0] {
                        "waiting" => {
                            self.set_attention(pane_id, MARK_WAITING);
                            return false;
                        }
                        "completed" => {
                            self.set_attention(pane_id, MARK_COMPLETED);
                            return false;
                        }
                        _ => {}
                    }
                }
            }
        }
        false
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
