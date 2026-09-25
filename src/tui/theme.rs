//! Colors for a dark terminal, in the 256-color palette so they survive tmux and SSH.

use ratatui::style::{Color, Modifier, Style};

const fn fg(c: u8) -> Style {
    Style::new().fg(Color::Indexed(c))
}

pub const TEXT: Style = Style::new();
pub const BRIGHT: Style = fg(255);
pub const DIM: Style = fg(242);
pub const MUTED: Style = fg(246);
pub const PROMPT_BAND: Style = Style::new().bg(Color::Indexed(236));
pub const PROMPT_MARK: Style = fg(244);
pub const BULLET: Style = fg(255);
pub const TOOLS: Style = fg(144);
pub const FOLD: Style = fg(242);
pub const OK: Style = fg(71);
pub const ERR: Style = fg(167);
pub const RUN: Style = fg(179);
pub const THINKING: Style = fg(244).add_modifier(Modifier::ITALIC);
pub const WARN: Style = fg(179);
pub const HEADING: Style = fg(75).add_modifier(Modifier::BOLD);
pub const STRONG: Style = Style::new().add_modifier(Modifier::BOLD);
pub const CODE: Style = fg(153);
pub const CODE_BLOCK: Style = fg(252);
pub const CODE_BG: Style = Style::new().bg(Color::Indexed(235));
pub const ADDED: Style = fg(114);
pub const REMOVED: Style = fg(174);
pub const HUNK: Style = fg(110);
pub const QUOTE: Style = fg(247).add_modifier(Modifier::ITALIC);
pub const RULE: Style = fg(240);
pub const LINK: Style = fg(75).add_modifier(Modifier::UNDERLINED);
pub const MATH: Style = fg(180);
pub const FOCUS: Style = Style::new().bg(Color::Indexed(238));
pub const MATCH: Style = Style::new().bg(Color::Indexed(58)).fg(Color::Indexed(230));
pub const MATCH_ON: Style = Style::new().bg(Color::Indexed(136)).fg(Color::Indexed(16));
pub const BAR: Style = Style::new().bg(Color::Indexed(235)).fg(Color::Indexed(250));
pub const BAR_KEY: Style = Style::new().bg(Color::Indexed(235)).fg(Color::Indexed(75));
pub const SELECTED: Style = Style::new().bg(Color::Indexed(237));
/// The mark above the session list, in the 256-color green nearest the web page's (#2df9c0).
pub const LOGO: Style = fg(49).add_modifier(Modifier::BOLD);

pub fn agent(name: &str) -> Style {
    match name {
        "claude" => fg(173),
        "codex" => fg(79),
        _ => fg(141),
    }
}
