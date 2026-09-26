# Changelog

Versions follow [Semantic Versioning](https://semver.org/). Each release is tagged `vX.Y.Z`, and
`ah --version` prints the version of the installed binary.

## 0.3.0 (2026-09-26)

- Each release has binaries for macOS (Apple Silicon and Intel) and Linux (x86_64 and ARM64); the
  Linux ones are statically linked and run on any distribution.
- `ah serve` can answer on a host name of your own through a tunnel such as Cloudflare Tunnel, set
  with `[public] host` in `~/.config/ah/config.toml`. Browsers there sign in with a link from
  `ah login`, which also shows a QR code, and stay signed in for 30 days; "Sign in on another
  device" in the command list shows such a link. The tailnet and 127.0.0.1 need no sign-in.
- Sessions can be shared by link: the share button in the header, `s` in the terminal, and
  `ah share list` and `ah share stop` to see and stop shares. A share is a snapshot of the chat or
  the answers, without tool calls, thinking or the folder, that lasts 1, 7 or 30 days or until
  stopped. Guests see the conversation without the session list.
- The browser has a pixel style, the default, with square edges drawn in pixels, hard shadows,
  pixel icons and the selection in inverted colors; the minimal style is still in the command
  list. The font is monospace by default, or sans-serif or pixel.
- The theme moved from the header into the command list, the view button shows its view as an
  icon, and the highlight in groups of buttons slides to the one picked.
- Codex `!` shell commands show as shell notices instead of prompts, and skill instructions are
  left out.

## 0.2.0 (2026-09-26)

- Pages close their live update streams while hidden and catch up when shown again, and the server
  sends a ping every 15 seconds that pages watch to replace lost connections. Several open tabs or a
  flaky network no longer leave new pages loading forever.
- `ah serve` also listens on the machine's Tailscale IPv6 address.
- The command list has buttons for the text size of the conversation (`+`, `-`) and for a centered
  column or the full window width (`w`).
- The empty page shows the mark and the slogan.
- The session list slides in and out, and dragging its edge far to the left hides it. While it
  shows, the menu button shows a close mark.
- The mark above the session list links to the repository.

## 0.1.0

The first version: the terminal viewer and `ah serve` for Claude Code, Codex and pi sessions.
