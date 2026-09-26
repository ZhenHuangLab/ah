# Changelog

Versions follow [Semantic Versioning](https://semver.org/). Each release is tagged `vX.Y.Z`, and
`ah --version` prints the version of the installed binary.

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
