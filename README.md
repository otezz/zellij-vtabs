# zellij-vtabs

A vertical, grouped, collapsible tab sidebar for [Zellij](https://zellij.dev) — with attention indicators driven by Claude Code.

> ⚠️ **This is a personal tool, built for my own setup — not a general-purpose plugin.**
> It hardcodes assumptions about *my* workflow: my plugin path, my `:`-based tab-naming
> convention, my Zellij version, my Claude Code hooks. It is published for my own reuse
> across machines, not as something meant to work out-of-the-box for anyone else. There
> are no stability guarantees, no issue tracker, and no intention to generalize it. If you
> stumbled on this: feel free to read/fork, but expect to change paths and conventions to
> fit your own environment.

## What it does

Zellij has no native vertical tabs. This plugin renders a left sidebar showing tabs as a
**collapsible tree grouped by name prefix**, so I can keep many tabs organized at a glance:

![zellij-vtabs sidebar](docs/screenshot.png)

Groups (`▼`/`▶`) collapse and expand; `◆` (yellow) marks a tab needing input, `✓` (green) a
finished one, and `●` marks the active tab.

- **Grouping** — a tab named `group:label` goes under group **group** with label **label**;
  a tab with no `:` lands in **General**. (First `:` wins.)
- **Collapse / expand** groups (`▶`/`▼`).
- **Reorder groups** — with a group header selected, `Shift+J`/`Shift+K` (or
  `Shift+↓`/`Shift+↑`) move it down/up the list.
- **Persistent state** — group order and collapse state survive session restarts and stay
  in sync across every tab's sidebar (stored per session in the plugin's cache dir).
- **Navigate** with `j`/`k`/arrows, `Enter`/`Space` to switch tab or toggle a group.
- **Mouse**: left-click to switch/toggle, scroll to move the selection.
- **Active tab** marked with `●`; the selection highlight follows it.
- **Attention icons** — `◆` (yellow, needs input) and `✓` (green, done), appearing on the
  tab and **rolling up to a collapsed group's header**. Cleared automatically when I focus
  the tab. Wired to Claude Code's `Notification`/`Stop` hooks.

## Requirements

- **Zellij 0.44.2** (the `zellij-tile` dependency is pinned to this exact version — see notes)
- **Rust** + `cargo`, with the `wasm32-wasip1` target: `rustup target add wasm32-wasip1`

## Build & install

```bash
cargo build --release --target wasm32-wasip1
cp target/wasm32-wasip1/release/zellij-vtabs.wasm ~/.config/zellij/plugins/zellij-vtabs.wasm
```

Or skip the Rust toolchain and grab the prebuilt `zellij-vtabs.wasm` from the
[latest release](https://github.com/otezz/zellij-vtabs/releases/latest):

```bash
curl -Lo ~/.config/zellij/plugins/zellij-vtabs.wasm \
  https://github.com/otezz/zellij-vtabs/releases/latest/download/zellij-vtabs.wasm
```

Pre-seed the plugin's permissions (Zellij's in-pane grant prompt doesn't render usably in a
narrow sidebar), in `~/.cache/zellij/permissions.kdl`. Note Zellij keys this by the **resolved
absolute** path, so use your real home dir here (not `~`):

```kdl
"/home/<you>/.config/zellij/plugins/zellij-vtabs.wasm" {
    ReadApplicationState
    ChangeApplicationState
    ReadCliPipes
}
```

Use the layout in `layouts/vtabs.kdl` (also copied to `~/.config/zellij/layouts/vtabs.kdl`),
and set it as the default in `~/.config/zellij/config.kdl`:

```kdl
default_layout "vtabs"
```

Fast dev loop (code-only changes — layout/config changes still need a fresh session):

```bash
cargo build --release --target wasm32-wasip1 \
  && cp target/wasm32-wasip1/release/zellij-vtabs.wasm ~/.config/zellij/plugins/zellij-vtabs.wasm
zellij action start-or-reload-plugin file:~/.config/zellij/plugins/zellij-vtabs.wasm
```

## Claude Code integration

The attention icons are driven by broadcast pipes. In `~/.claude/settings.json`:

```json
{
  "hooks": {
    "Notification": [{ "hooks": [{ "type": "command",
      "command": "zellij pipe --name \"zellij-vtabs::waiting::$ZELLIJ_PANE_ID\" < /dev/null" }] }],
    "Stop": [{ "hooks": [{ "type": "command",
      "command": "zellij pipe --name \"zellij-vtabs::completed::$ZELLIJ_PANE_ID\" < /dev/null" }] }]
  }
}
```

The `< /dev/null` is required: `zellij pipe` without a payload argument reads its payload
from stdin until EOF, and in a hook it inherits Claude's hook-JSON stdin — which stays open
until the hook exits, deadlocking every response for the full 60s hook timeout. Redirecting
stdin gives it an immediate EOF.

- `Notification` (Claude needs input) → `◆` on that tab
- `Stop` (Claude finished) → `✓` on that tab
- Focusing the tab clears it

Manual test — note it must target a **non-active** tab (the plugin never marks the tab you're
currently on, by design):

```bash
# on tab B:
echo $ZELLIJ_PANE_ID        # e.g. 3
# switch to another tab, then:
zellij pipe --name "zellij-vtabs::waiting::3"
```

## Configuration

Optional plugin config in the layout's `plugin` block (defaults shown):

```kdl
plugin location="file:~/.config/zellij/plugins/zellij-vtabs.wasm" {
    separator ":"        // tab-name group separator
    waiting_icon "◆"     // rendered yellow
    completed_icon "✓"   // rendered green
}
```

## Architecture notes (why it's built this way)

The one non-obvious design decision, learned the hard way:

**Zellij spawns one plugin instance per tab, and per-instance mutable state always diverges** —
broadcast CLI pipes and `pipe_message_to_plugin` don't reliably fan out to every instance, and
`Event::Visible` / `PaneUpdate.is_focused` aren't usable signals here (`is_focused` came through
as `None`). The *only* state every instance reads identically is the **tab name** (via
`TabUpdate`). So attention is encoded as a name suffix (` ⏳`/` ✅`) applied with `rename_tab`
(a global mutation), then parsed back out for display.

The working model, which avoids any set/clear race:

- **Set** marks a tab *only if it isn't the active tab* (you don't need a cue for what you're
  looking at).
- **Clear** strips the marker from the active tab on every `TabUpdate` (switching to a tab makes
  it active → it's "seen" → cleared).

Group order and collapse state follow the same "only global state survives" rule: they live
in a small file under the plugin's `/cache` mount (host side:
`~/.cache/zellij/<plugin-location>/plugin_cache/`), which Zellij keys by plugin *location* —
so every per-tab instance reads the same file. One file **per session** (named from
`ModeUpdate`'s `session_name`), because a single shared file would let each session's save
wipe the others' groups. Instances re-read the file on every `TabUpdate`, and only the
focused (visible) instance ever writes, so the sidebar you're looking at is always fresh.

Two build gotchas on modern Rust + Zellij:

1. **Pin `zellij-tile` to the exact running Zellij version** (`= 0.44.2`). A caret range grabs a
   newer patch whose plugin ABI the runtime rejects (`could not find exported function`).
2. **Build as a binary crate, not `cdylib`.** On current Rust, a `cdylib` for `wasm32-wasip1`
   emits a WASI *reactor* (no `_start`); Zellij needs a *command* (`_start`). A bin crate lets
   `register_plugin!`'s own `main` become `_start`. (Don't add your own `fn main` — the macro
   defines one.)

## Layout / status bar

`layouts/vtabs.kdl` puts the 28-col sidebar left of the main pane, with Zellij's single-line
`status-bar` (`size=1`) at the bottom to keep the key hints — matching the default layout's look.
