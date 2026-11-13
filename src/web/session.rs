//! Per-browser state. Pure: the socket loop feeds it messages and the clock, and executes its effects.

use std::time::{Duration, Instant};

use super::protocol::{AgentView, ClientMsg, ServerMsg};
use crate::actions::{self, Action, KeyInput};
use crate::fleet::Fleet;

pub const INTERACT_IDLE: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Watch(Option<u32>),
    Act { id: u64, action: Action },
    Keys { pane: u32, keys: Vec<KeyInput> },
    Send(ServerMsg),
    OpenTty7,
}

#[derive(Default)]
pub struct Session {
    selected: Option<u32>,
    /// The pane keystrokes may go to, and when the last one arrived.
    interact: Option<(u32, Instant)>,
    last_snapshot: Option<(Vec<AgentView>, Vec<String>)>,
    last_preview: Option<String>,
}

impl Session {
    pub fn selected(&self) -> Option<u32> {
        self.selected
    }

    pub fn on_client(&mut self, msg: ClientMsg, now: Instant) -> Vec<Effect> {
        match msg {
            ClientMsg::Watch { pane } => {
                let mut effects = self.end_interact_unless(pane);
                self.selected = pane;
                self.last_preview = None;
                effects.push(Effect::Watch(pane));
                effects
            }
            ClientMsg::Approve { id, pane } => vec![Effect::Act {
                id,
                action: Action::Approve { pane },
            }],
            ClientMsg::Deny { id, pane } => vec![Effect::Act {
                id,
                action: Action::Deny { pane },
            }],
            ClientMsg::Reply { id, pane, text } => {
                vec![Effect::Act {
                    id,
                    action: Action::Reply { pane, text },
                }]
            }
            ClientMsg::Interact { pane, on } => {
                if on && self.selected == Some(pane) {
                    self.interact = Some((pane, now));
                    vec![Effect::Send(ServerMsg::Interact { pane, on: true })]
                } else {
                    if self.interact.is_some_and(|(p, _)| p == pane) {
                        self.interact = None;
                    }
                    vec![Effect::Send(ServerMsg::Interact { pane, on: false })]
                }
            }
            ClientMsg::OpenTty7 => vec![Effect::OpenTty7],
            ClientMsg::Close { id, pane } => vec![Effect::Act {
                id,
                action: Action::Close { pane },
            }],
            ClientMsg::Keys { pane, keys } => match self.interact {
                Some((p, last)) if p == pane && now.duration_since(last) < INTERACT_IDLE => {
                    self.interact = Some((pane, now));
                    vec![Effect::Keys { pane, keys }]
                }
                _ => vec![Effect::Send(ServerMsg::Interact { pane, on: false })],
            },
        }
    }

    /// Messages the page needs after a fleet change or a clock tick.
    pub fn updates(&mut self, fleet: &Fleet, now: Instant) -> Vec<ServerMsg> {
        let mut out = Vec::new();
        if let Some((pane, last)) = self.interact
            && (now.duration_since(last) >= INTERACT_IDLE || !fleet.panes.contains_key(&pane))
        {
            self.interact = None;
            out.push(ServerMsg::Interact { pane, on: false });
        }

        let agents: Vec<AgentView> = fleet.ordered().into_iter().map(AgentView::from).collect();
        let snapshot = (agents, fleet.banners());
        if self.last_snapshot.as_ref() != Some(&snapshot) {
            out.push(ServerMsg::Snapshot {
                agents: snapshot.0.clone(),
                banners: snapshot.1.clone(),
            });
            self.last_snapshot = Some(snapshot);
        }

        if let Some(pane) = self.selected
            && let Some(text) = fleet.panes.get(&pane).and_then(|p| p.preview.as_ref())
            && self.last_preview.as_ref() != Some(text)
        {
            out.push(ServerMsg::Preview {
                pane,
                text: text.clone(),
                permission_prompt: actions::is_permission_prompt(text),
            });
            self.last_preview = Some(text.clone());
        }
        out
    }

    fn end_interact_unless(&mut self, pane: Option<u32>) -> Vec<Effect> {
        match self.interact {
            Some((p, _)) if Some(p) != pane => {
                self.interact = None;
                vec![Effect::Send(ServerMsg::Interact { pane: p, on: false })]
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::testing::fleet_with;
    use crate::tty7::types::Status;
    use std::time::Duration;

    fn keys() -> Vec<KeyInput> {
        vec![KeyInput::Text("x".into())]
    }

    #[test]
    fn first_update_sends_a_snapshot_and_repeats_send_nothing() {
        let fleet = fleet_with(&[(1, "Claude", Status::Working, 0)]);
        let mut session = Session::default();
        let now = Instant::now();
        let first = session.updates(&fleet, now);
        assert!(matches!(&first[..], [ServerMsg::Snapshot { agents, .. }] if agents.len() == 1));
        assert!(session.updates(&fleet, now).is_empty());
    }

    #[test]
    fn previews_go_out_for_the_watched_pane_when_they_change() {
        let mut fleet = fleet_with(&[
            (1, "Claude", Status::Working, 0),
            (2, "Claude", Status::Done, 0),
        ]);
        let mut session = Session::default();
        let now = Instant::now();
        session.updates(&fleet, now);
        assert_eq!(
            session.on_client(ClientMsg::Watch { pane: Some(1) }, now),
            vec![Effect::Watch(Some(1))]
        );

        fleet.set_preview(2, "other".into());
        assert!(session.updates(&fleet, now).is_empty());
        fleet.set_preview(1, "mine".into());
        assert_eq!(
            session.updates(&fleet, now),
            vec![ServerMsg::Preview {
                pane: 1,
                text: "mine".into(),
                permission_prompt: false,
            }]
        );
        assert!(session.updates(&fleet, now).is_empty());
    }

    #[test]
    fn actions_become_effects() {
        let mut session = Session::default();
        assert_eq!(
            session.on_client(ClientMsg::Approve { id: 9, pane: 1 }, Instant::now()),
            vec![Effect::Act {
                id: 9,
                action: Action::Approve { pane: 1 }
            }]
        );
    }

    #[test]
    fn keys_need_interact_mode_on_the_watched_pane() {
        let mut session = Session::default();
        let now = Instant::now();
        let off = Effect::Send(ServerMsg::Interact { pane: 1, on: false });

        assert_eq!(
            session.on_client(
                ClientMsg::Keys {
                    pane: 1,
                    keys: keys()
                },
                now
            ),
            vec![off.clone()]
        );
        // Interact can only be switched on for the pane being watched.
        assert_eq!(
            session.on_client(ClientMsg::Interact { pane: 1, on: true }, now),
            vec![off.clone()]
        );

        session.on_client(ClientMsg::Watch { pane: Some(1) }, now);
        assert_eq!(
            session.on_client(ClientMsg::Interact { pane: 1, on: true }, now),
            vec![Effect::Send(ServerMsg::Interact { pane: 1, on: true })]
        );
        assert_eq!(
            session.on_client(
                ClientMsg::Keys {
                    pane: 1,
                    keys: keys()
                },
                now
            ),
            vec![Effect::Keys {
                pane: 1,
                keys: keys()
            }]
        );
        assert_eq!(
            session.on_client(
                ClientMsg::Keys {
                    pane: 2,
                    keys: keys()
                },
                now
            )[0],
            Effect::Send(ServerMsg::Interact { pane: 2, on: false })
        );
    }

    #[test]
    fn interact_ends_after_two_idle_minutes() {
        let fleet = fleet_with(&[(1, "Claude", Status::Working, 0)]);
        let mut session = Session::default();
        let start = Instant::now();
        session.on_client(ClientMsg::Watch { pane: Some(1) }, start);
        session.on_client(ClientMsg::Interact { pane: 1, on: true }, start);
        session.updates(&fleet, start);

        let typing = start + Duration::from_secs(100);
        assert_eq!(
            session
                .on_client(
                    ClientMsg::Keys {
                        pane: 1,
                        keys: keys()
                    },
                    typing
                )
                .len(),
            1
        );
        assert!(
            session
                .updates(&fleet, typing + Duration::from_secs(119))
                .is_empty()
        );
        assert_eq!(
            session.updates(&fleet, typing + INTERACT_IDLE),
            vec![ServerMsg::Interact { pane: 1, on: false }]
        );
        assert!(matches!(
            session.on_client(
                ClientMsg::Keys {
                    pane: 1,
                    keys: keys()
                },
                typing + INTERACT_IDLE
            )[0],
            Effect::Send(ServerMsg::Interact { on: false, .. })
        ));
    }

    #[test]
    fn interact_ends_when_switching_panes_or_when_the_pane_closes() {
        let fleet = fleet_with(&[(1, "Claude", Status::Working, 0)]);
        let mut session = Session::default();
        let now = Instant::now();
        session.on_client(ClientMsg::Watch { pane: Some(1) }, now);
        session.on_client(ClientMsg::Interact { pane: 1, on: true }, now);
        assert_eq!(
            session.on_client(ClientMsg::Watch { pane: Some(2) }, now),
            vec![
                Effect::Send(ServerMsg::Interact { pane: 1, on: false }),
                Effect::Watch(Some(2))
            ]
        );

        session.on_client(ClientMsg::Watch { pane: Some(1) }, now);
        session.on_client(ClientMsg::Interact { pane: 1, on: true }, now);
        session.updates(&fleet, now);
        let closed = fleet_with(&[]);
        assert!(
            session
                .updates(&closed, now)
                .contains(&ServerMsg::Interact { pane: 1, on: false })
        );
    }

    #[test]
    fn open_tty7_becomes_an_effect() {
        let mut session = Session::default();
        assert_eq!(
            session.on_client(ClientMsg::OpenTty7, Instant::now()),
            vec![Effect::OpenTty7]
        );
    }

    #[test]
    fn previews_say_whether_a_permission_prompt_is_showing() {
        let mut fleet = fleet_with(&[(1, "Claude", Status::Waiting, 0)]);
        let mut session = Session::default();
        let now = Instant::now();
        session.on_client(ClientMsg::Watch { pane: Some(1) }, now);
        fleet.set_preview(
            1,
            include_str!("../../tests/fixtures/screen-permission.txt").into(),
        );
        assert!(matches!(
            session.updates(&fleet, now).last(),
            Some(ServerMsg::Preview {
                permission_prompt: true,
                ..
            })
        ));
    }

    #[test]
    fn close_becomes_an_action() {
        let mut session = Session::default();
        assert_eq!(
            session.on_client(ClientMsg::Close { id: 3, pane: 22 }, Instant::now()),
            vec![Effect::Act {
                id: 3,
                action: Action::Close { pane: 22 }
            }]
        );
    }
}
