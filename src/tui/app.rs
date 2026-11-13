use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::actions::{self, Action, Outcome};
use crate::fleet::Fleet;
use crate::tty7::types::Status;

/// TUI state that isn't part of the fleet. Selection is a pane id, not an index, so it
/// stays on the same agent when the order changes.
#[derive(Debug, Default)]
pub struct App {
    pub selected: Option<u32>,
    /// `Some` while typing a reply.
    pub reply: Option<String>,
    /// One-line message in the footer, cleared on the next key press.
    pub flash: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Act(Action),
    Quit,
}

impl App {
    pub fn sync_selection(&mut self, fleet: &Fleet) {
        if !self
            .selected
            .is_some_and(|id| fleet.panes.contains_key(&id))
        {
            self.selected = fleet.ordered().first().map(|p| p.pane);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, fleet: &Fleet) -> Option<Command> {
        if self.reply.is_some() {
            return self.handle_reply_key(key);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Command::Quit);
        }
        match key.code {
            KeyCode::Char('q') => Some(Command::Quit),
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(fleet, 1);
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(fleet, -1);
                None
            }
            KeyCode::Char('y') => self.answer_prompt(fleet, true),
            KeyCode::Char('n') => self.answer_prompt(fleet, false),
            KeyCode::Char('r') => {
                self.start_reply(fleet);
                None
            }
            _ => None,
        }
    }

    fn handle_reply_key(&mut self, key: KeyEvent) -> Option<Command> {
        let buffer = self.reply.as_mut()?;
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            if key.code == KeyCode::Char('c') {
                self.reply = None;
            }
            return None;
        }
        match key.code {
            KeyCode::Esc => self.reply = None,
            KeyCode::Backspace => {
                buffer.pop();
            }
            KeyCode::Char(c) => buffer.push(c),
            KeyCode::Enter => {
                let text = self.reply.take().unwrap_or_default();
                if text.trim().is_empty() {
                    return None;
                }
                return self
                    .selected
                    .map(|pane| Command::Act(Action::Reply { pane, text }));
            }
            _ => {}
        }
        None
    }

    fn move_selection(&mut self, fleet: &Fleet, delta: isize) {
        let ordered = fleet.ordered();
        if ordered.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .and_then(|id| ordered.iter().position(|p| p.pane == id))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, ordered.len() as isize - 1) as usize;
        self.selected = Some(ordered[next].pane);
    }

    fn answer_prompt(&mut self, fleet: &Fleet, approve: bool) -> Option<Command> {
        let pane = fleet.panes.get(&self.selected?)?;
        let agent = pane.agent.to_lowercase();
        if pane.status != Status::Waiting {
            self.flash = Some(format!("{agent} isn't waiting on you"));
            return None;
        }
        if !actions::supports_prompt_answers(&pane.agent) {
            self.flash = Some(format!(
                "approve/deny isn't supported for {agent} yet; answer in its pane"
            ));
            return None;
        }
        let pane = pane.pane;
        Some(Command::Act(if approve {
            Action::Approve { pane }
        } else {
            Action::Deny { pane }
        }))
    }

    fn start_reply(&mut self, fleet: &Fleet) {
        let Some(pane) = self.selected.and_then(|id| fleet.panes.get(&id)) else {
            return;
        };
        match pane.status {
            Status::Done | Status::Idle => self.reply = Some(String::new()),
            Status::Waiting => {
                self.flash = Some(format!(
                    "{} has a prompt open; answer it with y or n",
                    pane.agent.to_lowercase()
                ))
            }
            other => {
                self.flash = Some(format!(
                    "{} is {}; reply once it's done",
                    pane.agent.to_lowercase(),
                    other.label()
                ))
            }
        }
    }
}

/// Footer text for an action's result; `None` when there's nothing worth saying.
pub fn describe(outcome: &Outcome) -> Option<String> {
    match outcome {
        Outcome::Sent => None,
        Outcome::NotReady {
            status: Some(status),
        } => Some(format!("agent moved on (now {})", status.label())),
        Outcome::NotReady { status: None } => Some("agent is gone".into()),
        Outcome::UnknownPane => Some("that pane is gone".into()),
        Outcome::Unsupported => Some("not supported for this agent".into()),
        Outcome::Failed(err) => Some(format!("failed: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::testing::fleet_with;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn press(app: &mut App, fleet: &Fleet, codes: &[KeyCode]) -> Option<Command> {
        let mut last = None;
        for code in codes {
            last = app.handle_key(key(*code), fleet);
        }
        last
    }

    fn sample() -> Fleet {
        fleet_with(&[
            (1, "Claude", Status::Waiting, 100),
            (2, "Codex", Status::Waiting, 200),
            (3, "Claude", Status::Working, 50),
            (4, "Claude", Status::Done, 10),
        ])
    }

    #[test]
    fn selection_starts_at_the_top_and_moves() {
        let fleet = sample();
        let mut app = App::default();
        app.sync_selection(&fleet);
        assert_eq!(app.selected, Some(1));
        press(&mut app, &fleet, &[KeyCode::Char('j'), KeyCode::Down]);
        assert_eq!(app.selected, Some(3));
        press(
            &mut app,
            &fleet,
            &[KeyCode::Char('j'), KeyCode::Char('j'), KeyCode::Char('j')],
        );
        assert_eq!(app.selected, Some(4));
        press(&mut app, &fleet, &[KeyCode::Up]);
        assert_eq!(app.selected, Some(3));
    }

    #[test]
    fn selection_follows_a_pane_that_still_exists() {
        let mut app = App {
            selected: Some(3),
            ..App::default()
        };
        app.sync_selection(&sample());
        assert_eq!(app.selected, Some(3));
        app.sync_selection(&fleet_with(&[(9, "Claude", Status::Done, 0)]));
        assert_eq!(app.selected, Some(9));
    }

    #[test]
    fn y_and_n_answer_a_waiting_claude() {
        let fleet = sample();
        let mut app = App {
            selected: Some(1),
            ..App::default()
        };
        assert_eq!(
            press(&mut app, &fleet, &[KeyCode::Char('y')]),
            Some(Command::Act(Action::Approve { pane: 1 }))
        );
        assert_eq!(
            press(&mut app, &fleet, &[KeyCode::Char('n')]),
            Some(Command::Act(Action::Deny { pane: 1 }))
        );
    }

    #[test]
    fn y_on_an_agent_that_is_not_waiting_explains_why() {
        let fleet = sample();
        let mut app = App {
            selected: Some(3),
            ..App::default()
        };
        assert_eq!(press(&mut app, &fleet, &[KeyCode::Char('y')]), None);
        assert_eq!(app.flash.as_deref(), Some("claude isn't waiting on you"));
    }

    #[test]
    fn y_on_an_unsupported_agent_explains_why() {
        let fleet = sample();
        let mut app = App {
            selected: Some(2),
            ..App::default()
        };
        assert_eq!(press(&mut app, &fleet, &[KeyCode::Char('y')]), None);
        assert_eq!(
            app.flash.as_deref(),
            Some("approve/deny isn't supported for codex yet; answer in its pane")
        );
    }

    #[test]
    fn r_types_a_reply_for_a_finished_agent() {
        let fleet = sample();
        let mut app = App {
            selected: Some(4),
            ..App::default()
        };
        press(
            &mut app,
            &fleet,
            &[
                KeyCode::Char('r'),
                KeyCode::Char('o'),
                KeyCode::Char('k'),
                KeyCode::Char('x'),
            ],
        );
        assert_eq!(app.reply.as_deref(), Some("okx"));
        press(&mut app, &fleet, &[KeyCode::Backspace]);
        assert_eq!(
            press(&mut app, &fleet, &[KeyCode::Enter]),
            Some(Command::Act(Action::Reply {
                pane: 4,
                text: "ok".into()
            }))
        );
        assert_eq!(app.reply, None);
    }

    #[test]
    fn control_keys_are_not_typed_into_a_reply() {
        let fleet = sample();
        let mut app = App {
            selected: Some(4),
            ..App::default()
        };
        press(&mut app, &fleet, &[KeyCode::Char('r'), KeyCode::Char('a')]);
        // A pasted newline arrives as Ctrl+J.
        let ctrl_j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        assert_eq!(app.handle_key(ctrl_j, &fleet), None);
        assert_eq!(app.reply.as_deref(), Some("a"));

        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle_key(ctrl_c, &fleet), None);
        assert_eq!(app.reply, None);
    }

    #[test]
    fn reply_keys_do_not_trigger_shortcuts() {
        let fleet = sample();
        let mut app = App {
            selected: Some(4),
            ..App::default()
        };
        assert_eq!(
            press(&mut app, &fleet, &[KeyCode::Char('r'), KeyCode::Char('q')]),
            None
        );
        assert_eq!(app.reply.as_deref(), Some("q"));
    }

    #[test]
    fn esc_cancels_and_empty_replies_are_dropped() {
        let fleet = sample();
        let mut app = App {
            selected: Some(4),
            ..App::default()
        };
        press(
            &mut app,
            &fleet,
            &[KeyCode::Char('r'), KeyCode::Char('a'), KeyCode::Esc],
        );
        assert_eq!(app.reply, None);
        assert_eq!(
            press(&mut app, &fleet, &[KeyCode::Char('r'), KeyCode::Enter]),
            None
        );
    }

    #[test]
    fn r_is_refused_while_a_prompt_is_open() {
        let fleet = sample();
        let mut app = App {
            selected: Some(1),
            ..App::default()
        };
        press(&mut app, &fleet, &[KeyCode::Char('r')]);
        assert_eq!(app.reply, None);
        assert_eq!(
            app.flash.as_deref(),
            Some("claude has a prompt open; answer it with y or n")
        );
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let fleet = sample();
        let mut app = App::default();
        assert_eq!(
            press(&mut app, &fleet, &[KeyCode::Char('q')]),
            Some(Command::Quit)
        );
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(app.handle_key(ctrl_c, &fleet), Some(Command::Quit));
    }

    #[test]
    fn describes_outcomes() {
        assert_eq!(describe(&Outcome::Sent), None);
        assert_eq!(
            describe(&Outcome::NotReady {
                status: Some(Status::Working)
            })
            .as_deref(),
            Some("agent moved on (now working)")
        );
        assert_eq!(
            describe(&Outcome::NotReady { status: None }).as_deref(),
            Some("agent is gone")
        );
    }
}
