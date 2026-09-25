<p align="center">
  <img src="docs/social-preview.png" alt="ah：agent history, anywhere！终端里的会话列表，后面是浏览器里的一个会话">
</p>

<h1 align="center">ah</h1>

<p align="center">
  Agent history：在终端里，或在另一台机器的浏览器里，阅读 Claude Code、Codex 和 pi 的会话记录。
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="许可证：MIT"></a>
  <img src="https://img.shields.io/badge/Rust-DEA584?logo=rust&logoColor=black" alt="用 Rust 编写">
  <img src="https://img.shields.io/badge/works_with-Claude_Code_%C2%B7_Codex_%C2%B7_pi-2DF9C0" alt="支持 Claude Code、Codex 和 pi">
</p>

<p align="center"><a href="README.md">English</a> · <b>简体中文</b></p>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/all-dark.png">
  <img src="docs/all-light.png" alt="浏览器界面：左侧是按日期分组的会话列表，右侧是一个 Claude Code 会话，其中一组工具调用已展开">
</picture>

`ah` 会找出本机上这三种 agent 的全部会话，并显示完整的记录，包括上下文压缩（compaction）之前的内容。工具调用和思考过程在消息之间折叠成一行摘要。仅对话视图会把它们完全隐藏；仅回答视图只显示你的提问和每一轮的最后一条消息，略去 agent 在工具调用之间写下的进度说明。终端和浏览器都会跟随记录文件，所以正在运行的会话会随着 agent 的输出实时更新。

## 安装

```sh
cargo install --git https://github.com/ZhenHuangLab/ah
```

程序本身内嵌了网页前端（包括 KaTeX 和 highlight.js），因此可以离线使用。

## 终端

```sh
ah                 # 选择会话；当前目录的会话排在最前
ah e671b1          # 按 id 或唯一的 id 前缀打开会话
ah path/to.jsonl   # 打开任意记录文件，包括 subagent 的记录
```

<img src="docs/terminal-list.png" alt="终端里的会话列表：当前目录的会话标有星号，其余按最近时间排列">

在列表中直接输入即可筛选，按 Enter 打开。打开的会话里，思考过程和工具调用默认是折叠的：

<img src="docs/terminal-session.png" alt="终端里的一个 Claude Code 会话，消息之间的思考过程和工具调用已折叠">

会话中的按键：

| 按键 | |
|---|---|
| `j` `k`、`space` `b`、`ctrl-d` `ctrl-u` | 滚动 |
| `g` `G` | 跳到开头、结尾（停在结尾时会继续跟随新消息） |
| `[` `]` | 上一个、下一个提问 |
| `t` | 仅对话：隐藏工具调用、思考过程和提示信息 |
| `a` | 仅回答：只显示提问和每一轮的最终回答 |
| `tab` `enter`、点击 | 在折叠项之间移动，展开或收起 |
| `e` | 全部展开或全部收起 |
| `/` `n` `N` | 搜索 |
| `y` | 复制当前消息，或复制选中的工具调用 |
| 拖动 | 选中文字并复制 |
| `q` | 返回列表 |

复制使用 OSC 52，通过 SSH 也能写入你本地的剪贴板。在 tmux 中需要设置 `set -g set-clipboard on`。

数学公式以 Unicode 显示（`ϕᵢⱼ(q) = 2πq·(dᵢ - dⱼ)`），而不是 TeX 源码，比如下面这个处于仅回答视图的 pi 会话：

<img src="docs/terminal-answers.png" alt="终端仅回答视图中的一个 pi 会话，公式和表格都以 Unicode 绘制">

## 浏览器

```sh
ah serve
```

它监听本机的 Tailscale 地址和 127.0.0.1 的 7447 端口，tailnet 中的任何设备都可以打开 `http://<machine>:7447/`。用 `--addr IP:PORT`（可重复）可以指定其他地址。服务没有登录，所以只响应用 IP 地址、`localhost` 或 Tailscale 名称（`machine` 或 `machine.<tailnet>.ts.net`）访问它的请求，这样网页无法通过 DNS 重绑定（DNS rebinding）访问到它。

页面会渲染 Markdown 和数学公式，折叠工具调用（展开时才加载细节），并实时更新。超过两轮的会话在右侧有一条轮次导航：悬停可以预览每一轮，点击即可跳转。页头的按钮显示当前视图的名称，点击切换到下一个视图：全部（All）、仅对话（Chat）、仅回答（Answers），快捷键分别是 `t` 和 `a`。仅对话视图隐藏工具调用、思考过程和提示信息：

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/chat-dark.png">
  <img src="docs/chat-light.png" alt="同一轮在仅对话视图中的样子：提问、工具调用之间的说明和回答">
</picture>

仅回答视图只保留你的提问和每一轮的最终回答。这张截图里的会话列表按文件夹分组：

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/answers-dark.png">
  <img src="docs/answers-light.png" alt="仅回答视图：只有提问和最终回答，左侧的会话列表按文件夹分组">
</picture>

按 `?` 或 Ctrl-K，或点击 ⋯，可以打开命令列表，里面有所有命令和它们的快捷键；输入文字即可筛选，用方向键加 Enter 或者鼠标选择。

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/palette-dark.png">
  <img src="docs/palette-light.png" alt="在一个 pi 会话上打开的命令列表，列出每个命令和它的快捷键">
</picture>

会话列表可以按日期分组，也可以按文件夹分组，文件夹的标题可以展开或收起（点击 “By date” 切换）。☰ 或 `s` 可以隐藏列表，拖动它的边缘可以调整宽度。在手机上，列表以抽屉的形式打开。

复制选中的内容会得到 Markdown：公式是 TeX，代码是围栏代码块，列表、表格和强调都用 Markdown 语法。消息下方的复制图标会复制这条消息的 Markdown 源文；旁边是这条消息写下的时间，按浏览器所在的时区显示。

如果想让它一直运行，可以把它设为 systemd 用户服务：

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
loginctl enable-linger "$USER"   # 退出登录后也保持运行
```

本机没有 Tailscale 地址时，`ah serve` 会退出，systemd 会一直重试，直到地址出现。

## 会话文件的位置

| Agent | 默认位置 | 可用环境变量覆盖 |
|---|---|---|
| Claude Code | `~/.claude/projects/*/*.jsonl` | `CLAUDE_CONFIG_DIR` |
| Codex | `~/.codex/sessions/`、`~/.codex/archived_sessions/` | `CODEX_HOME` |
| pi | `~/.pi/agent/sessions/*/*.jsonl` | `PI_CODING_AGENT_SESSION_DIR`、`PI_CODING_AGENT_DIR` |

列表中不包含 subagent 的记录，可以按路径打开它们。记录按文件中的顺序显示。pi 会话用 `/tree` 回到较早的位置时，被放弃的分支仍然可见，并有一条提示标出对话从哪里继续。

## 许可证

MIT，见 [LICENSE](LICENSE)。`assets/vendor` 中包含 KaTeX（MIT）和 highlight.js（BSD-3-Clause）以及它们的许可证文件。截图中的会话都是虚构的。

## 致谢

感谢 <a href="https://linux.do/"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/linuxdo-dark.png"><img src="docs/linuxdo-light.png" alt="LINUX DO" height="28" align="absmiddle"></picture></a> 社区的支持与反馈。
