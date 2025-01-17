use std::sync::Arc;

use crate::message::SlaveMesage;
use axum::http::StatusCode;
use tracing::debug;

struct ServerState {
    tx: tokio::sync::mpsc::Sender<SlaveMesage>,
}

const LISTEN_IP: &str = "0.0.0.0:33000";

pub async fn run(tx: tokio::sync::mpsc::Sender<SlaveMesage>) {
    debug!(addr = LISTEN_IP, "starting slave server...");

    let app_state = Arc::new(ServerState { tx });

    let app = axum::Router::new()
        .route("/api/ping", axum::routing::post(ping_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(LISTEN_IP)
        .await
        .expect("failed to create tcp listener");

    axum::serve(listener, app)
        .await
        .expect("server retruned an error");
}

async fn ping_handler(
    axum::extract::State(state): axum::extract::State<Arc<ServerState>>,
) -> StatusCode {
    state
        .tx
        .send(SlaveMesage::new())
        .await
        .expect("to send message");

    StatusCode::OK
}
