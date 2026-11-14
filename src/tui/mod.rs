//! The terminal UI.

pub mod app;
pub mod view;

use std::collections::HashSet;
use std::process::Stdio;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use tokio::sync::mpsc;

use crate::runtime::{Handle, now_ms};
use app::{App, Command};

/// Where the dashboard lives. `login` carries the token and is only handed to the browser.
pub struct WebLinks {
    pub url: String,
    pub login: String,
}

pub async fn run(handle: Handle, web: Option<WebLinks>) -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, handle, web).await;
    ratatui::restore();
    result
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    handle: Handle,
    web: Option<WebLinks>,
) -> anyhow::Result<()> {
    let mut keys = spawn_key_reader();
    let mut snapshots = handle.snapshots.clone();
    // Redraws once a second even without changes, so elapsed times keep counting.
    let mut clock = tokio::time::interval(Duration::from_secs(1));
    let mut app = App {
        web_url: web.as_ref().map(|w| w.url.clone()),
        ..App::default()
    };
    let mut watched = None;

    loop {
        let fleet = snapshots.borrow_and_update().clone();
        app.sync_selection(&fleet);
        if app.selected != watched {
            watched = app.selected;
            handle
                .watch("tui", watched.into_iter().collect::<HashSet<_>>())
                .await;
        }
        terminal.draw(|frame| view::render(frame, &fleet, &app, now_ms()))?;

        tokio::select! {
            changed = snapshots.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
            }
            _ = clock.tick() => {}
            key = keys.recv() => {
                let Some(key) = key else { return Ok(()) };
                app.flash = None;
                match app.handle_key(key, &fleet) {
                    Some(Command::Quit) => return Ok(()),
                    Some(Command::OpenWeb) => app.flash = Some(open_web(web.as_ref())),
                    Some(Command::Act(action)) => {
                        let note = app::sent_note(&action, &fleet);
                        let outcome = handle.act(action).await;
                        // `message()` is `None` only for a sent action.
                        app.flash = outcome.message().or(note);
                    }
                    None => {}
                }
            }
        }
    }
}

fn open_web(web: Option<&WebLinks>) -> String {
    let Some(web) = web else {
        return "the web dashboard isn't running".into();
    };
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let spawned = std::process::Command::new(opener)
        .arg(&web.login)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            // Reaps the opener so it doesn't linger as a zombie.
            std::thread::spawn(move || child.wait());
            format!("opened {}", web.url)
        }
        Err(err) => format!(
            "couldn't start {opener} ({err}); open {} yourself",
            web.login
        ),
    }
}

/// crossterm's `event::read` blocks, so it runs on its own thread and forwards key presses.
fn spawn_key_reader() -> mpsc::UnboundedReceiver<KeyEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        loop {
            match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    if tx.send(key).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    rx
}
