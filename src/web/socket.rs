use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use tokio::time::interval;
use tracing::{debug, warn};

use super::protocol::{ClientMsg, ServerMsg};
use super::server::AppState;
use super::session::{Effect, Session};
use crate::actions::{Action, Outcome};
use crate::runtime::Handle;

const PROCS_EVERY: Duration = Duration::from_secs(2);

pub async fn upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let client = format!("web-{}", state.next_client.fetch_add(1, Ordering::Relaxed));
    let handle = state.handle.clone();
    ws.on_upgrade(move |socket| run(socket, handle, client))
}

async fn run(mut socket: WebSocket, handle: Handle, client: String) {
    if let Err(err) = session_loop(&mut socket, &handle, &client).await {
        debug!("web session {client} ended: {err:#}");
    }
    handle.watch(&client, HashSet::new()).await;
}

async fn session_loop(socket: &mut WebSocket, handle: &Handle, client: &str) -> anyhow::Result<()> {
    let mut snapshots = handle.snapshots.clone();
    let mut session = Session::default();
    // Also drives the interact-mode idle timeout.
    let mut clock = interval(Duration::from_secs(1));
    let mut procs_tick = interval(PROCS_EVERY);

    loop {
        let fleet = snapshots.borrow_and_update().clone();
        for msg in session.updates(&fleet, Instant::now()) {
            send(socket, &msg).await?;
        }

        tokio::select! {
            changed = snapshots.changed() => changed?,
            _ = clock.tick() => {}
            _ = procs_tick.tick() => {
                if let Some(pane) = session.selected()
                    && let Some(resp) = handle.procs(pane).await
                {
                    send(socket, &ServerMsg::Procs { pane, procs: resp.procs, ports: resp.ports }).await?;
                }
            }
            incoming = socket.recv() => {
                let Some(message) = incoming else { return Ok(()) };
                let Message::Text(text) = message? else { continue };
                let msg: ClientMsg = match serde_json::from_str(&text) {
                    Ok(msg) => msg,
                    Err(err) => {
                        warn!("ignoring malformed browser message: {err}");
                        continue;
                    }
                };
                for effect in session.on_client(msg, Instant::now()) {
                    match effect {
                        Effect::Watch(pane) => handle.watch(client, pane.into_iter().collect()).await,
                        Effect::Send(msg) => send(socket, &msg).await?,
                        Effect::Act { id, action } => {
                            let outcome = handle.act(action).await;
                            let ok = outcome == Outcome::Sent;
                            send(socket, &ServerMsg::ActionResult { id, ok, message: outcome.message() }).await?;
                        }
                        Effect::OpenTty7 => {
                            if let Err(err) = handle.activate_gui().await {
                                let message = Some(format!("couldn't open tty7: {err}"));
                                send(socket, &ServerMsg::ActionResult { id: 0, ok: false, message }).await?;
                            }
                        }
                        Effect::Keys { pane, keys } => {
                            let outcome = handle.act(Action::Keys { pane, keys }).await;
                            if outcome != Outcome::Sent {
                                let msg = ServerMsg::ActionResult { id: 0, ok: false, message: outcome.message() };
                                send(socket, &msg).await?;
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn send(socket: &mut WebSocket, msg: &ServerMsg) -> anyhow::Result<()> {
    socket
        .send(Message::Text(serde_json::to_string(msg)?.into()))
        .await?;
    Ok(())
}
