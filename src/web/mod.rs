//! The web dashboard: an axum server on 127.0.0.1.

pub mod auth;
pub mod protocol;
pub mod server;
pub mod session;
mod socket;

use std::net::Ipv4Addr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::warn;

use crate::runtime::Handle;

pub struct WebServer {
    pub port: u16,
    token: Arc<str>,
}

impl WebServer {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// One-time link that swaps the token for a cookie.
    pub fn login_url(&self) -> String {
        format!("{}/?token={}", self.url(), self.token)
    }
}

/// Binds 127.0.0.1 only. Falls back to a free port when `preferred_port` is taken.
pub async fn start(handle: Handle, preferred_port: u16) -> anyhow::Result<WebServer> {
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, preferred_port)).await {
        Ok(listener) => listener,
        Err(err) => {
            warn!("port {preferred_port} unavailable ({err}); using a free port");
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?
        }
    };
    let port = listener.local_addr()?.port();
    let token: Arc<str> = auth::new_token().into();
    let app = server::router(server::AppState::new(handle, token.clone(), port));
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            warn!("web server stopped: {err}");
        }
    });
    Ok(WebServer { port, token })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime;
    use crate::tty7::fake::{FakeCli, Sent};
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    type Ws = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    const WAITING_54: &str = r#"{"agents":[{"pane_id":54,"agent":"Claude","state":{"status":"waiting","cwd":"/tmp/x"}}]}"#;

    async fn connect(server: &WebServer) -> Ws {
        let mut request = format!("ws://127.0.0.1:{}/ws", server.port)
            .into_client_request()
            .unwrap();
        let headers = request.headers_mut();
        headers.insert(
            "Cookie",
            format!("shipherd={}", server.token).parse().unwrap(),
        );
        headers.insert(
            "Origin",
            format!("http://127.0.0.1:{}", server.port).parse().unwrap(),
        );
        tokio_tungstenite::connect_async(request).await.unwrap().0
    }

    async fn next_of_type(ws: &mut Ws, kind: &str) -> serde_json::Value {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
                .await
                .expect("timed out")
                .expect("socket closed")
                .unwrap();
            if let Ok(text) = msg.to_text() {
                let value: serde_json::Value = serde_json::from_str(text).unwrap();
                if value["type"] == kind {
                    return value;
                }
            }
        }
    }

    async fn send(ws: &mut Ws, json: serde_json::Value) {
        ws.send(Message::Text(json.to_string().into()))
            .await
            .unwrap();
    }

    /// The returned sender keeps the fake event stream open; dropping it would simulate tty7 exiting.
    async fn setup() -> (
        Arc<FakeCli>,
        WebServer,
        Ws,
        tokio::sync::mpsc::Sender<String>,
    ) {
        let cli = Arc::new(FakeCli::with_agents(WAITING_54));
        let events = cli.push_event_stream();
        let server = start(runtime::start(cli.clone()), 0).await.unwrap();
        let mut ws = connect(&server).await;
        loop {
            let snapshot = next_of_type(&mut ws, "snapshot").await;
            if snapshot["agents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["pane"] == 54)
            {
                break;
            }
        }
        (cli, server, ws, events)
    }

    #[tokio::test]
    async fn browser_gets_a_snapshot_and_can_approve() {
        let (cli, _server, mut ws, _events) = setup().await;
        send(
            &mut ws,
            serde_json::json!({"type": "approve", "id": 7, "pane": 54}),
        )
        .await;
        let result = next_of_type(&mut ws, "action_result").await;
        assert_eq!(result["id"], 7);
        assert_eq!(result["ok"], true);
        assert_eq!(
            cli.sent(),
            vec![Sent::Text {
                pane: 54,
                text: "1".into(),
                enter: false
            }]
        );
    }

    #[tokio::test]
    async fn keys_only_reach_the_pane_in_interact_mode() {
        let (cli, _server, mut ws, _events) = setup().await;
        let keys = serde_json::json!({"type": "keys", "pane": 54, "keys": [{"kind": "text", "value": "hi"}]});

        send(&mut ws, keys.clone()).await;
        assert_eq!(next_of_type(&mut ws, "interact").await["on"], false);
        assert!(cli.sent().is_empty());

        send(&mut ws, serde_json::json!({"type": "watch", "pane": 54})).await;
        send(
            &mut ws,
            serde_json::json!({"type": "interact", "pane": 54, "on": true}),
        )
        .await;
        assert_eq!(next_of_type(&mut ws, "interact").await["on"], true);
        send(&mut ws, keys).await;

        for _ in 0..50 {
            if !cli.sent().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(
            cli.sent(),
            vec![Sent::Text {
                pane: 54,
                text: "hi".into(),
                enter: false
            }]
        );
    }
}
