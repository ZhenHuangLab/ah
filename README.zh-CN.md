<p align="center">
  <img src="docs/social-preview.png" alt="ah：agent history, anywhere！终端里的会话列表，后面是浏览器里的一个会话">
</p>

<h1 align="center">ah</h1>

<p align="center">
  Agent history：在终端里，或在任何地方的浏览器里，阅读和分享 Claude Code、Codex 和 pi 的会话记录。
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

每个 [release](https://github.com/ZhenHuangLab/ah/releases) 都附有 macOS（Apple Silicon 和 Intel）和 Linux（x86_64 和 ARM64）的程序。Linux 版本是静态链接的，任何发行版都能运行。把最新版本装到 `~/.local/bin`：

```sh
target=aarch64-apple-darwin  # 或 x86_64-apple-darwin、x86_64-unknown-linux-musl、aarch64-unknown-linux-musl
mkdir -p ~/.local/bin
curl -fsSL https://github.com/ZhenHuangLab/ah/releases/latest/download/ah-$target.tar.gz | tar xz -C ~/.local/bin ah
```

在 macOS 上用浏览器下载的程序会被系统拦截，运行 `xattr -d com.apple.quarantine ah` 即可放行。也可以从源码编译：

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
| `s` | 用链接分享对话或回答（见[分享会话](#分享会话)） |
| 拖动 | 选中文字并复制 |
| `q` | 返回列表 |

复制使用 OSC 52，通过 SSH 也能写入你本地的剪贴板。在 tmux 中需要设置 `set -g set-clipboard on`。

数学公式以 Unicode 显示（`ϕᵢⱼ(q) = 2πq·(dᵢ - dⱼ)`），而不是 TeX 源码，比如下面这个处于仅回答视图的 pi 会话：

<img src="docs/terminal-answers.png" alt="终端仅回答视图中的一个 pi 会话，公式和表格都以 Unicode 绘制">

## 浏览器

```sh
ah serve
```

它监听本机的 Tailscale 地址（IPv4 和 IPv6）和 127.0.0.1 的 7447 端口，tailnet 中的任何设备都可以打开 `http://<machine>:7447/`。用 `--addr IP:PORT`（可重复）可以指定其他地址。服务没有登录，所以只响应用 IP 地址、`localhost` 或 Tailscale 名称（`machine` 或 `machine.<tailnet>.ts.net`）访问它的请求，这样网页无法通过 DNS 重绑定（DNS rebinding）访问到它。

页面会渲染 Markdown 和数学公式，折叠工具调用（展开时才加载细节），并实时更新。超过两轮的会话在右侧有一条轮次导航：悬停可以预览每一轮，点击即可跳转。页头的按钮用图标显示当前视图（几行横线、一个对话气泡、带对勾的对话气泡），点击切换到下一个视图：全部（All）、仅对话（Chat）、仅回答（Answers），快捷键分别是 `t` 和 `a`。仅对话视图隐藏工具调用、思考过程和提示信息：

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/chat-dark.png">
  <img src="docs/chat-light.png" alt="同一轮在仅对话视图中的样子：提问、工具调用之间的说明和回答">
</picture>

仅回答视图只保留你的提问和每一轮的最终回答。这张截图里的会话列表按文件夹分组：

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/answers-dark.png">
  <img src="docs/answers-light.png" alt="仅回答视图：只有提问和最终回答，左侧的会话列表按文件夹分组">
</picture>

按 `?` 或 Ctrl-K，或点击 ⋯，可以打开命令列表，里面有所有命令和它们的快捷键；输入文字即可筛选，用方向键加 Enter 或者鼠标选择。命令上方的按钮可以调整对话的字号（`+`、`-`），在居中的窄栏和铺满窗口的宽屏之间切换（`w`），选择亮色或暗色主题、风格和字体。默认的像素风格用像素画出方角边框和硬阴影，选中的项目以反色显示；简约风格是圆角和柔和的高亮。页面和对话的字体默认是等宽字体，也可以选无衬线或像素字体。

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/palette-dark.png">
  <img src="docs/palette-light.png" alt="在一个 pi 会话上打开的命令列表，列出每个命令和它的快捷键">
</picture>

会话列表可以按日期分组，也可以按文件夹分组，文件夹的标题可以展开或收起（点击 “By date” 切换）。左上角的按钮或 `s` 可以隐藏和显示列表，拖动它的边缘可以调整宽度，把边缘往左拖到底也会隐藏列表。列表上方的标志链接到本仓库。在手机上，列表以抽屉的形式打开。

复制选中的内容会得到 Markdown：公式是 TeX，代码是围栏代码块，列表、表格和强调都用 Markdown 语法。消息下方的复制图标会复制这条消息的 Markdown 源文；旁边是这条消息写下的时间，按浏览器所在的时区显示。

如果想让它一直运行，可以把它设为 systemd 用户服务：

```ini
# ~/.config/systemd/user/ah.service
[Unit]
Description=ah: web viewer for Claude Code, Codex and pi sessions
After=network-online.target

[Service]
ExecStart=%h/.local/bin/ah serve
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now ah
loginctl enable-linger "$USER"   # 退出登录后也保持运行
```

用 Cargo 安装时，`ah` 在 `~/.cargo/bin` 里。本机既没有 Tailscale 地址、也没有配置公网域名（见下文）时，`ah serve` 会退出，systemd 会一直重试，直到地址出现。

## 从任何地方访问

`ah serve` 还可以通过 [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/) 在你自己的域名上提供服务，使用 HTTPS，不需要开放端口。通过这个域名访问的浏览器需要用登录链接登录；从 tailnet 或 127.0.0.1 访问仍然不需要登录。

1. 在 Cloudflare 后台的 Networking › Tunnels 里创建一个隧道，给它添加一个公网主机名，比如 `ah.example.com`，服务填 `http://localhost:7448`。
2. 安装 `cloudflared`，用隧道的 token 把它装成系统服务：`sudo cloudflared service install <token>`。
3. 在 `~/.config/ah/config.toml` 里写上这个域名，然后重启 `ah serve`：

   ```toml
   [public]
   host = "ah.example.com"
   ```

之后 `ah serve` 会额外在 127.0.0.1:7448 上监听，供隧道连接；没有 Tailscale 地址时也能运行。

要登录，在这台机器上运行 `ah login`。它会打印一个 10 分钟内有效的链接，并附上方便手机扫描的二维码；在浏览器里打开链接并确认后，会保持登录 30 天。在 tailnet 里的浏览器，或者已经登录的浏览器，可以在命令列表里选择 “Sign in on another device”，为另一台设备显示这样的链接。删除 `~/.local/share/ah/key` 并重启 `ah serve`，所有浏览器都会退出登录。

### 分享会话

配置了公网域名后，点击会话右上角的分享按钮，或者在终端里按 `s`，就能为会话此刻的内容生成一个链接。链接显示对话（提问和 agent 的消息）或回答（提问和每一轮的最终回答）；工具调用、思考过程和会话所在的文件夹都不会包含在内。链接的有效期可以是 1 天、7 天、30 天，或者一直有效直到你停止它。分享之前请先读一遍：提问和回答里可能有密钥、token 或私密路径。

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/share-dark.png">
  <img src="docs/share-light.png" alt="访客看到的分享会话：提问和 agent 的消息，没有会话列表，底部有一行关于 ah 的介绍">
</picture>

打开链接的人看到的是没有会话列表的对话页面，页面也会要求搜索引擎不要收录。分享对话框会列出这个会话已有的链接，并可以停止它们；`ah share list` 和 `ah share stop <id>` 可以对所有会话做同样的事。每个链接的快照保存在 `~/.local/share/ah/shares` 里，所以即使 agent 删除了原来的记录文件，只要 `ah serve` 在运行，链接就仍然可以打开。

## 会话文件的位置

| Agent | 默认位置 | 可用环境变量覆盖 |
|---|---|---|
| Claude Code | `~/.claude/projects/*/*.jsonl` | `CLAUDE_CONFIG_DIR` |
| Codex | `~/.codex/sessions/`、`~/.codex/archived_sessions/` | `CODEX_HOME` |
| pi | `~/.pi/agent/sessions/*/*.jsonl` | `PI_CODING_AGENT_SESSION_DIR`、`PI_CODING_AGENT_DIR` |

列表中不包含 subagent 的记录，可以按路径打开它们。记录按文件中的顺序显示。pi 会话用 `/tree` 回到较早的位置时，被放弃的分支仍然可见，并有一条提示标出对话从哪里继续。

## 许可证

MIT，见 [LICENSE](LICENSE)。`assets/vendor` 中包含 KaTeX（MIT）、highlight.js（BSD-3-Clause）和 Pixelify Sans 字体（SIL Open Font License 1.1）以及它们的许可证文件。截图中的会话都是虚构的。

## 致谢

感谢 <a href="https://linux.do/"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/linuxdo-dark.png"><img src="docs/linuxdo-light.png" alt="LINUX DO" height="28" align="absmiddle"></picture></a> 社区的支持与反馈。
