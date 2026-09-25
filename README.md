# ah

Agent history: read Claude Code, Codex and pi sessions in the terminal or in a
browser on another machine.

`ah` finds every session of the three agents on this machine and shows the whole
transcript, including everything before a context compaction. Tool calls and
thinking fold into one summary line between messages, and a chat-only mode hides
them entirely. Both views follow the transcript file, so a running session
updates as the agent writes.

## Install

```sh
cargo install --git https://github.com/ZhenHuangLab/ah
```

The binary embeds the web front end, including KaTeX and highlight.js, so it
works offline.

## Terminal

```sh
ah                 # pick a session; this directory's sessions come first
ah e671b1          # open a session by id or unique id prefix
ah path/to.jsonl   # open any transcript, including subagent ones
```

In the list, type to filter and press Enter to open. In a session:

| Keys | |
|---|---|
| `j` `k`, `space` `b`, `ctrl-d` `ctrl-u` | scroll |
| `g` `G` | top, bottom (bottom keeps following new messages) |
| `[` `]` | previous, next prompt |
| `t` | chat only: hide tool calls, thinking and notices |
| `tab` `enter`, click | move between folds, open or close one |
| `e` | expand or collapse all |
| `/` `n` `N` | search |
| `y` | copy the message, or the focused tool call |
| drag | select text and copy it |
| `q` | back to the list |

Copying uses OSC 52, which reaches your local clipboard over SSH. Inside tmux,
set `set -g set-clipboard on`.

Math is shown as Unicode (`ϕᵢⱼ(q) = 2πq·(dᵢ - dⱼ)`) rather than TeX source.

## Browser

```sh
ah serve
```

This listens on the machine's Tailscale address and on 127.0.0.1, port 7447,
so any device on the tailnet can open `http://<machine>:7447/`. Use
`--addr IP:PORT` (repeatable) to choose other addresses. There is no login, so
the server only answers requests that name it by IP address, `localhost` or its
Tailscale name (`machine` or `machine.<tailnet>.ts.net`); this keeps web pages
from reaching it through DNS rebinding.

The page renders Markdown and math, folds tool calls (details load when opened),
has a rail of the conversation's turns on the right that previews each turn on
hover and jumps on click, and updates live. Keys: `t` chat only, `j` `k` next and
previous turn, `/` filter sessions.

To keep it running, as a systemd user service:

```ini
# ~/.config/systemd/user/ah.service
[Unit]
Description=ah: web viewer for Claude Code, Codex and pi sessions
After=network-online.target

[Service]
ExecStart=%h/.cargo/bin/ah serve
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now ah
loginctl enable-linger "$USER"   # keep it running while logged out
```

`ah serve` exits when the machine has no Tailscale address, and systemd retries
until one appears.

## Where sessions come from

| Agent | Location | Override |
|---|---|---|
| Claude Code | `~/.claude/projects/*/*.jsonl` | `CLAUDE_CONFIG_DIR` |
| Codex | `~/.codex/sessions/`, `~/.codex/archived_sessions/` | `CODEX_HOME` |
| pi | `~/.pi/agent/sessions/*/*.jsonl` | `PI_CODING_AGENT_SESSION_DIR`, `PI_CODING_AGENT_DIR` |

Subagent transcripts are left out of the lists; open them by path. Transcripts
are shown in file order. When a pi session goes back to an earlier point with
`/tree`, the abandoned branch stays visible and a notice marks where the
conversation resumes.

## License notes

`assets/vendor` contains KaTeX (MIT) and highlight.js (BSD-3-Clause) with their
license files.
