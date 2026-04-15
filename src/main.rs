//! Phase 1 spike 1e: verify Axum + eframe can coexist in one binary on Windows.
//!
//! - Tokio multi-thread runtime runs on a background thread, serving an Axum app
//!   with REST + WebSocket endpoints.
//! - eframe owns the main thread, displaying a QR code for the server URL and a
//!   live WebSocket-client count.
//! - A shared `AtomicUsize` communicates client count across the boundary.
//!
//! This is NOT production structure — Phase 2 will split server and phone SPA
//! into separate workspace crates. Keep this file self-contained.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;

const INDEX_HTML: &str = include_str!("../assets/spike_index.html");

#[derive(Clone)]
struct AppState {
    clients: Arc<AtomicUsize>,
}

#[derive(Serialize)]
struct Status {
    clients: usize,
    server: &'static str,
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn status(State(state): State<AppState>) -> Json<Status> {
    Json(Status {
        clients: state.clients.load(Ordering::Relaxed),
        server: "skylander-portal-controller spike",
    })
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    state.clients.fetch_add(1, Ordering::Relaxed);
    tracing::info!(
        "client connected; total = {}",
        state.clients.load(Ordering::Relaxed)
    );

    let (mut sender, mut receiver) = socket.split();

    let _ = sender
        .send(Message::Text("hello from server".into()))
        .await;

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(t) => {
                let echo = format!("echo: {t}");
                if sender.send(Message::Text(echo)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    state.clients.fetch_sub(1, Ordering::Relaxed);
    tracing::info!(
        "client disconnected; total = {}",
        state.clients.load(Ordering::Relaxed)
    );
}

fn first_non_loopback_ipv4() -> Option<Ipv4Addr> {
    match local_ip_address::local_ip() {
        Ok(IpAddr::V4(v4)) if !v4.is_loopback() => Some(v4),
        _ => None,
    }
}

fn start_server(state: AppState, bind: SocketAddr) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        rt.block_on(async move {
            let app = Router::new()
                .route("/", get(index))
                .route("/api/status", get(status))
                .route("/ws", get(ws_handler))
                .with_state(state);

            let listener = tokio::net::TcpListener::bind(bind).await.expect("bind");
            tracing::info!("serving on http://{bind}");
            axum::serve(listener, app).await.expect("serve");
        });
    })
}

struct SpikeApp {
    clients: Arc<AtomicUsize>,
    url: String,
    qr_texture: Option<egui::TextureHandle>,
}

impl SpikeApp {
    fn new(cc: &eframe::CreationContext<'_>, clients: Arc<AtomicUsize>, url: String) -> Self {
        let qr_texture = render_qr_texture(&cc.egui_ctx, &url);
        Self {
            clients,
            url,
            qr_texture: Some(qr_texture),
        }
    }
}

fn render_qr_texture(ctx: &egui::Context, url: &str) -> egui::TextureHandle {
    let code = qrcode::QrCode::new(url).expect("qr encode");
    let dark = egui::Color32::from_rgb(0x0b, 0x1e, 0x3f);
    let light = egui::Color32::WHITE;
    let scale = 8usize;
    let modules: Vec<Vec<bool>> = code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(1, 1)
        .build()
        .lines()
        .map(|l| l.chars().map(|c| c != ' ').collect())
        .collect();
    let h = modules.len();
    let w = modules.first().map(|r| r.len()).unwrap_or(0);
    let img_w = w * scale;
    let img_h = h * scale;
    let mut pixels = Vec::with_capacity(img_w * img_h);
    for y in 0..img_h {
        for x in 0..img_w {
            let b = modules[y / scale][x / scale];
            pixels.push(if b { dark } else { light });
        }
    }
    let color_image = egui::ColorImage {
        size: [img_w, img_h],
        pixels,
    };
    ctx.load_texture("qr", color_image, egui::TextureOptions::NEAREST)
}

impl eframe::App for SpikeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                ui.heading(egui::RichText::new("Skylander Portal — Spike").size(32.0));
                ui.add_space(8.0);
                ui.label(egui::RichText::new(&self.url).size(20.0).monospace());
                ui.add_space(16.0);
                if let Some(tex) = &self.qr_texture {
                    let size = tex.size_vec2();
                    ui.image((tex.id(), size));
                }
                ui.add_space(16.0);
                let n = self.clients.load(Ordering::Relaxed);
                ui.label(egui::RichText::new(format!("Connected clients: {n}")).size(28.0));
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let ip = first_non_loopback_ipv4().unwrap_or(Ipv4Addr::LOCALHOST);
    let port: u16 = 8765;
    let bind = SocketAddr::from((ip, port));
    let url = format!("http://{bind}");

    let clients = Arc::new(AtomicUsize::new(0));
    let state = AppState {
        clients: clients.clone(),
    };

    let _server = start_server(state, bind);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Skylander Portal Controller (spike)")
            .with_inner_size([640.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "skylander-portal-spike",
        native_options,
        Box::new(move |cc| Ok(Box::new(SpikeApp::new(cc, clients, url)))),
    )
}
