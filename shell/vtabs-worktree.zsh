# zellij-vtabs worktree launchers — pair these with the sidebar's auto-grouping.
#
#   nw <name> [base-ref]   create a git worktree named <name> under the repo's
#                          .claude/worktrees/, open it in a NEW zellij tab named
#                          "<repo>:<name>" (so it groups under the repo in the
#                          sidebar) running `claude`.
#   rw [-f]                remove the current linked worktree and close its tab.
#                          Refuses on the main worktree; `-f` forces past changes.
#
# Source this from ~/.zshrc:  source ~/.config/zellij/vtabs-worktree.zsh
# The new tab uses the `vtabs-claude` layout (a vtabs layout whose main pane runs
# claude); override with VTABS_CLAUDE_LAYOUT if you name it differently.

nw() {
  emulate -L zsh
  # NB: do not name a local `path` — zsh ties $path to $PATH and would blank it.
  local name=$1 base=$2 common root repo wtpath
  [[ -n $name ]] || { print -u2 "usage: nw <name> [base-ref]"; return 1 }
  common=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) \
    || { print -u2 "nw: not inside a git repo"; return 1 }
  root=${common%/.git}
  repo=${root:t}
  wtpath=$root/.claude/worktrees/$name
  [[ -e $wtpath ]] && { print -u2 "nw: $wtpath already exists"; return 1 }
  if ! git -C "$root" worktree add -b "$name" "$wtpath" ${base:+"$base"} 2>/dev/null; then
    git -C "$root" worktree add "$wtpath" "$name" \
      || { print -u2 "nw: could not create worktree (branch '$name' checked out elsewhere?)"; return 1 }
  fi
  zellij action new-tab --layout "${VTABS_CLAUDE_LAYOUT:-vtabs-claude}" --cwd "$wtpath" --name "$repo:$name"
}

rw() {
  emulate -L zsh
  local force common root wt
  [[ $1 == -f ]] && force=--force
  common=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) \
    || { print -u2 "rw: not inside a git repo"; return 1 }
  root=${common%/.git}
  wt=$(git rev-parse --show-toplevel 2>/dev/null)
  [[ $wt == $root ]] && { print -u2 "rw: this is the main worktree — refusing"; return 1 }
  git -C "$root" worktree remove $force "$wt" \
    || { print -u2 "rw: worktree has changes — commit/stash, or 'rw -f'"; return 1 }
  zellij action close-tab
}
