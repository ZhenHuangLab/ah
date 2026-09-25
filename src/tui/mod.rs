//! The terminal viewer: a session picker and a conversation view that follows the file.

mod doc;
mod md;
mod picker;
mod tex;
mod text;
mod theme;
mod view;

use std::collections::HashSet;
use std::io::stdout;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use notify::{RecursiveMode, Watcher};
use ratatui::Frame;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::execute;

use crate::discover::{self, Roots};
use picker::{Pick, Picker};
use view::{Action, Viewer};

struct App {
    roots: Roots,
    picker: Picker,
    viewer: Option<Viewer>,
    /// Opened straight from the command line: leaving the view quits.
    direct: bool,
}

pub fn run(query: Option<&str>) -> Result<()> {
    let roots = Roots::detect();
    let list = discover::scan(&roots);
    let viewer = match query {
        Some(q) => Some(Viewer::open(discover::resolve(&list, q).map_err(|e| anyhow!(e))?)?),
        None if list.is_empty() => bail!("no Claude Code, Codex or pi sessions found"),
        None => None,
    };
    let cwd = std::env::current_dir().ok().and_then(|p| p.to_str().map(String::from)).unwrap_or_default();
    let mut app = App { direct: viewer.is_some(), viewer, picker: Picker::new(list, cwd), roots };

    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            for p in ev.paths.into_iter().filter(|p| p.extension().is_some_and(|e| e == "jsonl")) {
                let _ = tx.send(p);
            }
        }
    })?;
    let mut dirs: Vec<PathBuf> = app.roots.dirs().into_iter().map(PathBuf::from).collect();
    if let Some(dir) = app.viewer.as_ref().and_then(|v| v.live.meta.path.parent())
        && !dirs.iter().any(|d| dir.starts_with(d))
    {
        dirs.push(dir.to_path_buf());
    }
    for d in &dirs {
        watcher.watch(d, RecursiveMode::Recursive)?;
    }

    let mut term = ratatui::try_init()?;
    execute!(stdout(), EnableMouseCapture)?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        hook(info);
    }));
    let res = (|| -> Result<()> {
        loop {
            term.draw(|f| app.draw(f))?;
            if event::poll(Duration::from_millis(500))? {
                loop {
                    if app.handle(event::read()?)? {
                        return Ok(());
                    }
                    if !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            let changed: HashSet<PathBuf> = rx.try_iter().collect();
            for p in &changed {
                app.changed(p);
            }
        }
    })();
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();
    res
}

impl App {
    fn draw(&mut self, f: &mut Frame) {
        match &mut self.viewer {
            Some(v) => v.draw(f),
            None => self.picker.draw(f),
        }
    }

    /// Returns true to quit.
    fn handle(&mut self, ev: Event) -> Result<bool> {
        if let Event::Key(k) = &ev
            && k.kind != KeyEventKind::Press
        {
            return Ok(false);
        }
        if let Some(v) = &mut self.viewer {
            let action = match ev {
                Event::Key(k) => v.key(k),
                Event::Mouse(m) => {
                    v.mouse(m);
                    Action::None
                }
                _ => Action::None,
            };
            match action {
                Action::Quit => return Ok(true),
                Action::Back if self.direct => return Ok(true),
                Action::Back => self.viewer = None,
                Action::None => {}
            }
            return Ok(false);
        }
        let pick = match ev {
            Event::Key(k) => self.picker.key(k),
            Event::Mouse(m) => self.picker.mouse(m),
            _ => Pick::None,
        };
        match pick {
            Pick::Quit => return Ok(true),
            Pick::Open(m) => match Viewer::open(m) {
                Ok(v) => self.viewer = Some(v),
                Err(e) => self.picker.msg = Some(format!("{e:#}")),
            },
            Pick::None => {}
        }
        Ok(false)
    }

    fn changed(&mut self, path: &PathBuf) {
        if let Some(v) = &mut self.viewer
            && &v.live.meta.path == path
        {
            v.refresh();
        }
        self.picker.changed(&self.roots, path);
    }
}
