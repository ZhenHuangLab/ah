<p align="center">
  <img src="docs/social-preview.png" alt="ah: agent history, anywhere! The session list in a terminal in front of a session in a browser">
</p>

<h1 align="center">ah</h1>

<p align="center">
  Agent history: read Claude Code, Codex and pi sessions in the terminal or in a
  browser on another machine.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/Rust-DEA584?logo=rust&logoColor=black" alt="Written in Rust">
  <img src="https://img.shields.io/badge/works_with-Claude_Code_%C2%B7_Codex_%C2%B7_pi-2DF9C0" alt="Works with Claude Code, Codex and pi">
</p>

<p align="center"><b>English</b> · <a href="README.zh-CN.md">简体中文</a></p>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/all-dark.png">
  <img src="docs/all-light.png" alt="The browser view: sessions by date on the left, and a Claude Code session with one group of tool calls opened">
</picture>

`ah` finds every session of the three agents on this machine and shows the whole
transcript, including everything before a context compaction. Tool calls and
thinking fold into one summary line between messages. The chat view hides them
entirely, and the answers view shows only your prompts and the last message of
each turn, leaving out the notes agents write between tool calls. Both the
terminal and the browser follow the transcript file, so a running session
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

<img src="docs/terminal-list.png" alt="The session list in a terminal: sessions from this directory, marked with an asterisk, then the rest by recency">

In the list, type to filter and press Enter to open. A session opens with its
thinking and tool calls folded:

<img src="docs/terminal-session.png" alt="A Claude Code session in a terminal, with its thinking and tool calls folded between the messages">

In a session:

| Keys | |
|---|---|
| `j` `k`, `space` `b`, `ctrl-d` `ctrl-u` | scroll |
| `g` `G` | top, bottom (bottom keeps following new messages) |
| `[` `]` | previous, next prompt |
| `t` | chat only: hide tool calls, thinking and notices |
| `a` | answers only: prompts and the final answer of each turn |
| `tab` `enter`, click | move between folds, open or close one |
| `e` | expand or collapse all |
| `/` `n` `N` | search |
| `y` | copy the message, or the focused tool call |
| drag | select text and copy it |
| `q` | back to the list |

Copying uses OSC 52, which reaches your local clipboard over SSH. Inside tmux,
set `set -g set-clipboard on`.

Math is shown as Unicode (`ϕᵢⱼ(q) = 2πq·(dᵢ - dⱼ)`) rather than TeX source, as
in this pi session in the answers view:

<img src="docs/terminal-answers.png" alt="A pi session in the terminal's answers view, with its formulas and a table drawn in Unicode">

## Browser

```sh
ah serve
```

This listens on the machine's Tailscale addresses (IPv4 and IPv6) and on
127.0.0.1, port 7447, so any device on the tailnet can open
`http://<machine>:7447/`. Use
`--addr IP:PORT` (repeatable) to choose other addresses. There is no login, so
the server only answers requests that name it by IP address, `localhost` or its
Tailscale name (`machine` or `machine.<tailnet>.ts.net`); this keeps web pages
from reaching it through DNS rebinding.

The page renders Markdown and math, folds tool calls (details load when opened),
and updates live. Sessions of more than two turns get a rail of their turns on
the right that previews each turn on hover and jumps on click. The button in the
header names the current view and switches to the next: everything, chat only,
answers only (`t`, `a`). Chat only hides tool calls, thinking and notices:

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/chat-dark.png">
  <img src="docs/chat-light.png" alt="The same turn in the chat view: the prompt, the notes between tool calls and the answer">
</picture>

Answers only keeps your prompts and the final answer of each turn. Here the
session list is grouped by folder:

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/answers-dark.png">
  <img src="docs/answers-light.png" alt="The answers view: prompts and final answers only, next to a session list grouped by folder">
</picture>

Press `?` or Ctrl-K, or click ⋯, for a list of all commands with their keys; type
to filter it, and pick one with the arrow keys and Enter or with the mouse.
Buttons above the commands set the text size of the conversation (`+`, `-`) and
switch between a centered column and the full width of the window (`w`).

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/palette-dark.png">
  <img src="docs/palette-light.png" alt="The command list open over a pi session, with each command and its keys">
</picture>

The session list is grouped by date, or by folder under headers that open and
close ("By date" switches). The button at the top left or `s` hides and shows it,
and dragging its edge changes its width; dragging the edge most of the way to the
left hides it. The mark above it links to this repository. On a phone it opens as
a drawer.

Copying a selection gives Markdown: formulas as TeX, code as fenced blocks, and
lists, tables and emphasis in Markdown syntax. The copy icon under a message
copies its Markdown source; beside it is the time the message was written, in
the browser's time zone.

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

## License

MIT; see [LICENSE](LICENSE). `assets/vendor` contains KaTeX (MIT) and
highlight.js (BSD-3-Clause) with their license files. The screenshots show
invented sessions.

## Acknowledgments

Thanks to the <a href="https://linux.do/"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/linuxdo-dark.png"><img src="docs/linuxdo-light.png" alt="LINUX DO" height="28" align="absmiddle"></picture></a> community for its support and feedback.
