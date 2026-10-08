use anyhow::{Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

/// Just enough of the Chrome DevTools Protocol to send a command and read its
/// result. Events are ignored since no domain is ever enabled.
pub struct Cdp {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl Cdp {
    pub async fn connect(ws_url: &str) -> Result<Self> {
        let (ws, _) = connect_async(ws_url).await?;
        Ok(Self { ws, next_id: 1 })
    }

    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({"id": id, "method": method, "params": params});
        self.ws.send(Message::text(request.to_string())).await?;

        while let Some(message) = self.ws.next().await {
            let Message::Text(text) = message? else {
                continue;
            };
            let response: Value = serde_json::from_str(&text)?;
            if response["id"] != id {
                continue;
            }
            if let Some(error) = response.get("error") {
                bail!("{method} a échoué : {error}");
            }
            return Ok(response["result"].clone());
        }
        Err(anyhow!("connexion DevTools fermée pendant {method}"))
    }
}
