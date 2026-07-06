use std::collections::{BTreeMap, BTreeSet};
use zellij_tile::prelude::*;

#[derive(Default)]
struct State {
    tabs: Vec<TabInfo>,
    collapsed: BTreeSet<String>,
    selected: usize,
    separator: char,
}

register_plugin!(State);

/// A rendered line in the sidebar: either a group header or a tab under it.
enum Row {
    Group { name: String, collapsed: bool, count: usize },
    Tab { position: usize, label: String, active: bool },
}

impl State {
    /// Split a tab name into (group, label) on the first separator.
    /// "work:api" -> ("work", "api"); "scratch" -> ("General", "scratch").
    fn group_of(&self, name: &str) -> (String, String) {
        match name.find(self.separator) {
            Some(i) => (
                name[..i].to_string(),
                name[i + self.separator.len_utf8()..].trim().to_string(),
            ),
            None => ("General".to_string(), name.to_string()),
        }
    }

    /// Build the flat list of visible rows, groups in first-seen order.
    fn build_rows(&self) -> Vec<Row> {
        let mut order: Vec<String> = Vec::new();
        let mut groups: BTreeMap<String, Vec<(usize, String, bool)>> = BTreeMap::new();
        for t in &self.tabs {
            let (g, label) = self.group_of(&t.name);
            if !order.contains(&g) {
                order.push(g.clone());
            }
            groups.entry(g).or_default().push((t.position, label, t.active));
        }
        let mut rows = Vec::new();
        for g in &order {
            let items = groups.get(g).cloned().unwrap_or_default();
            let collapsed = self.collapsed.contains(g);
            rows.push(Row::Group { name: g.clone(), collapsed, count: items.len() });
            if !collapsed {
                for (position, label, active) in items {
                    rows.push(Row::Tab { position, label, active });
                }
            }
        }
        rows
    }

    /// Activate the row at `idx`: toggle a group's collapse, or switch to a tab.
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
                // Zellij tab indices are 1-based; TabInfo.position is 0-based.
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
                if row < 0 || row as usize >= len {
                    return false;
                }
                let idx = row as usize;
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
                let len = self.build_rows().len();
                if len > 0 && self.selected >= len {
                    self.selected = len - 1;
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
        if visible.is_empty() {
            print!("\u{1b}[2m(no tabs)\u{1b}[0m");
            return;
        }
        let mut out = String::new();
        for (i, row) in visible.iter().enumerate() {
            let text = match row {
                Row::Group { name, collapsed, count } => {
                    let disc = if *collapsed { "▸" } else { "▾" };
                    format!("{} {} ({})", disc, name, count)
                }
                Row::Tab { label, active, .. } => {
                    let marker = if *active { "●" } else { " " };
                    format!("  {} {}", marker, label)
                }
            };
            let text = truncate(&text, cols);
            if i == self.selected {
                out.push_str(&format!("\u{1b}[7m{}\u{1b}[0m", text));
            } else {
                out.push_str(&text);
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
