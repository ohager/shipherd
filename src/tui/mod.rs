//! The terminal UI.

pub mod app;
pub mod view;

use std::collections::HashSet;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use tokio::sync::mpsc;

use crate::runtime::{Handle, now_ms};
use app::{App, Command};

pub async fn run(handle: Handle) -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, handle).await;
    ratatui::restore();
    result
}

async fn event_loop(terminal: &mut DefaultTerminal, handle: Handle) -> anyhow::Result<()> {
    let mut keys = spawn_key_reader();
    let mut snapshots = handle.snapshots.clone();
    // Redraws once a second even without changes, so elapsed times keep counting.
    let mut clock = tokio::time::interval(Duration::from_secs(1));
    let mut app = App::default();
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
                    Some(Command::Act(action)) => {
                        let outcome = handle.act(action).await;
                        app.flash = app::describe(&outcome);
                    }
                    None => {}
                }
            }
        }
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
