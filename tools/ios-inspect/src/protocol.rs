//! WebKit Web Inspector protocol client over WebSocket.
//!
//! WebKit's dialect of DevTools protocol wraps per-page commands in
//! `Target.sendMessageToTarget` (after `Target.targetCreated` on connect)
//! and unwraps responses from `Target.dispatchMessageFromTarget`. This
//! module hides that dance behind a small `Session` API.
//!
//! Quirks captured from the spike:
//! - No `DOM.enable` / `CSS.enable` (always-on; calling them errors).
//! - `Page.snapshotRect` returns a `data:image/png;base64,…` dataURL that
//!   can exceed 1 MB, so we bump `max_message_size` explicitly.

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct Session {
    ws: WsStream,
    target_id: String,
    next_outer: u64,
    next_inner: u64,
}

impl Session {
    pub async fn connect(ws_url: &str) -> Result<Self> {
        let config = WebSocketConfig {
            max_message_size: Some(32 * 1024 * 1024),
            max_frame_size: Some(32 * 1024 * 1024),
            ..Default::default()
        };
        let (mut ws, _) = tokio_tungstenite::connect_async_with_config(ws_url, Some(config), false)
            .await
            .with_context(|| format!("connect to {ws_url}"))?;

        // Expect Target.targetCreated for a page.
        let target_id = loop {
            let msg = recv_text(&mut ws).await?;
            let v: Value = serde_json::from_str(&msg)?;
            if v.get("method").and_then(Value::as_str) == Some("Target.targetCreated") {
                if v.pointer("/params/targetInfo/type").and_then(Value::as_str) == Some("page") {
                    break v
                        .pointer("/params/targetInfo/targetId")
                        .and_then(Value::as_str)
                        .context("targetCreated without targetId")?
                        .to_string();
                }
            }
        };

        Ok(Self { ws, target_id, next_outer: 1, next_inner: 100 })
    }

    /// Send a wrapped command, read until we see the matching response.
    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let inner_id = self.next_inner;
        self.next_inner += 1;
        let outer_id = self.next_outer;
        self.next_outer += 1;

        let inner = json!({"id": inner_id, "method": method, "params": params});
        let outer = json!({
            "id": outer_id,
            "method": "Target.sendMessageToTarget",
            "params": {"targetId": self.target_id, "message": inner.to_string()},
        });
        self.ws
            .send(tokio_tungstenite::tungstenite::Message::Text(outer.to_string()))
            .await?;

        // Drain incoming messages until the matching inner id arrives.
        // (The proxy may also send unrelated events along the way.)
        for _ in 0..200 {
            let raw = recv_text(&mut self.ws).await?;
            let v: Value = serde_json::from_str(&raw)?;
            if v.get("method").and_then(Value::as_str) == Some("Target.dispatchMessageFromTarget")
            {
                let inner_str = v
                    .pointer("/params/message")
                    .and_then(Value::as_str)
                    .context("dispatchMessageFromTarget without message")?;
                let inner_resp: Value = serde_json::from_str(inner_str)?;
                if inner_resp.get("id").and_then(Value::as_u64) == Some(inner_id) {
                    if let Some(err) = inner_resp.get("error") {
                        bail!("{method} error: {err}");
                    }
                    return Ok(inner_resp.get("result").cloned().unwrap_or(Value::Null));
                }
            }
        }
        bail!("no response to {method} within 200 messages");
    }

    // ---- convenience wrappers ----

    pub async fn runtime_evaluate(&mut self, expression: &str) -> Result<Value> {
        self.call(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true}),
        )
        .await
    }

    pub async fn query_selector(&mut self, selector: &str) -> Result<Option<u64>> {
        let doc = self.call("DOM.getDocument", json!({})).await?;
        let root_id = doc
            .pointer("/root/nodeId")
            .and_then(Value::as_u64)
            .context("DOM.getDocument missing root.nodeId")?;
        let q = self
            .call(
                "DOM.querySelector",
                json!({"nodeId": root_id, "selector": selector}),
            )
            .await?;
        let node_id = q.get("nodeId").and_then(Value::as_u64).unwrap_or(0);
        Ok(if node_id == 0 { None } else { Some(node_id) })
    }

    pub async fn computed_style(&mut self, node_id: u64) -> Result<Vec<ComputedProp>> {
        let r = self
            .call(
                "CSS.getComputedStyleForNode",
                json!({"nodeId": node_id}),
            )
            .await?;
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(rename = "computedStyle")]
            computed_style: Vec<ComputedProp>,
        }
        let w: Wrap = serde_json::from_value(r)?;
        Ok(w.computed_style)
    }

    /// Dump DOM. With a selector, returns the matching element's outerHTML as
    /// a string value (wrapped in JSON for consistency). Without a selector,
    /// returns the full `DOM.getDocument` tree at the requested depth.
    pub async fn dump_dom(&mut self, selector: Option<&str>, depth: i32) -> Result<Value> {
        if let Some(sel) = selector {
            let node_id = self
                .query_selector(sel)
                .await?
                .ok_or_else(|| anyhow::anyhow!("no element matches {sel:?}"))?;
            let r = self
                .call("DOM.getOuterHTML", json!({"nodeId": node_id}))
                .await?;
            return Ok(r);
        }
        self.call("DOM.getDocument", json!({"depth": depth})).await
    }

    pub async fn viewport_size(&mut self) -> Result<(f64, f64)> {
        let r = self
            .runtime_evaluate("JSON.stringify([window.innerWidth, window.innerHeight])")
            .await?;
        let s = r
            .pointer("/result/value")
            .and_then(Value::as_str)
            .context("viewport_size: no string value")?;
        let v: Vec<f64> = serde_json::from_str(s)?;
        Ok((v[0], v[1]))
    }

    /// Wraps `Page.snapshotRect` and decodes the `data:image/png;base64,…`
    /// result to raw PNG bytes. Payload can be multi-MB; we raised the WS
    /// max_message_size at connect time to accommodate.
    pub async fn snapshot_rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> Result<Vec<u8>> {
        use base64::Engine as _;
        let r = self
            .call(
                "Page.snapshotRect",
                json!({
                    "x": x, "y": y, "width": w, "height": h,
                    "coordinateSystem": "Viewport",
                }),
            )
            .await?;
        let data_url = r
            .get("dataURL")
            .and_then(Value::as_str)
            .context("snapshotRect: no dataURL in result")?;
        let (_, b64) = data_url
            .split_once(",")
            .context("snapshotRect: malformed dataURL")?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
        Ok(bytes)
    }
}

async fn recv_text(ws: &mut WsStream) -> Result<String> {
    loop {
        let Some(msg) = ws.next().await else {
            bail!("websocket closed unexpectedly");
        };
        match msg? {
            tokio_tungstenite::tungstenite::Message::Text(t) => return Ok(t),
            tokio_tungstenite::tungstenite::Message::Binary(_) => continue,
            tokio_tungstenite::tungstenite::Message::Ping(p) => {
                ws.send(tokio_tungstenite::tungstenite::Message::Pong(p)).await?;
            }
            tokio_tungstenite::tungstenite::Message::Close(_) => {
                bail!("websocket closed");
            }
            _ => continue,
        }
    }
}

#[derive(Deserialize)]
pub struct ComputedProp {
    pub name: String,
    pub value: String,
}
