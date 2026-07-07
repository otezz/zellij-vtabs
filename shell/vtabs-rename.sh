#!/bin/sh
# Pipe this pane's cwd facts to zellij-vtabs so it can auto-name the tab
# (see the autogroup_* keys in the layout's plugin config).
#
# Usage: vtabs-rename.sh [force]
#   default — rename only if the tab still has a default "Tab #N" name
#   force   — rename even a manually-named tab (used by the Claude worktree hook)
#
# Callers:
#   ~/.zshrc:                (~/.config/zellij/vtabs-rename.sh &) 2>/dev/null
#   Claude Code SessionStart: ~/.config/zellij/vtabs-rename.sh force
#
# `timeout 3` matters: zellij pipe blocks forever if no plugin instance is
# listening in this session (e.g. a session without the vtabs layout).
[ -n "$ZELLIJ" ] && [ -n "$ZELLIJ_PANE_ID" ] || exit 0
mode=cwd
[ "$1" = force ] && mode=cwd-force
facts="cwd=$PWD"
if toplevel=$(git rev-parse --path-format=absolute --show-toplevel 2>/dev/null); then
    common=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)
    branch=$(git symbolic-ref --short HEAD 2>/dev/null)
    facts=$(printf '%s\ntoplevel=%s\ncommon=%s\nbranch=%s' "$facts" "$toplevel" "$common" "$branch")
fi
exec timeout 3 zellij pipe --name "zellij-vtabs::$mode::$ZELLIJ_PANE_ID" -- "$facts" </dev/null
