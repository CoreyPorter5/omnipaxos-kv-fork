use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use omnipaxos_kv::common::kv::KVCommand;

pub struct HttpTrigger {
    pub cmd: KVCommand,
    pub response_tx: oneshot::Sender<KvResp>,
}

#[derive(Clone)]
pub struct AppState {
    pub tx: mpsc::Sender<HttpTrigger>,
}

#[derive(Serialize, Debug)]
pub struct KvResp {
    pub ok: bool,
    pub value: Option<String>,
    pub swapped: Option<bool>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct PutBody {
    pub value: String,
}

#[derive(Deserialize)]
pub struct CasBody {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub create_if_not_exists: bool,
}

async fn dispatch(state: AppState, cmd: KVCommand) -> (StatusCode, Json<KvResp>) {
    let (response_tx, response_rx) = oneshot::channel();

    if state
        .tx
        .send(HttpTrigger { cmd, response_tx })
        .await
        .is_err()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(KvResp {
                ok: false,
                value: None,
                swapped: None,
                error: Some("Client loop not running".into()),
            }),
        );
    }

    match response_rx.await {
        Ok(resp) => (StatusCode::OK, Json(resp)),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(KvResp {
                ok: false,
                value: None,
                swapped: None,
                error: Some("Client loop dropped response".into()),
            }),
        ),
    }
}

// --- Separate endpoints, but minimal code in each ---

async fn get_endpoint(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> (StatusCode, Json<KvResp>) {
    dispatch(state, KVCommand::Get(key)).await
}

async fn put_endpoint(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<PutBody>,
) -> (StatusCode, Json<KvResp>) {
    dispatch(state, KVCommand::Put(key, body.value)).await
}

async fn cas_endpoint(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<CasBody>,
) -> (StatusCode, Json<KvResp>) {
    dispatch(
        state,
        KVCommand::Cas {
            key,
            from: body.from,
            to: body.to,
            create_if_not_exists: body.create_if_not_exists,
        },
    )
        .await
}

pub fn router(tx: mpsc::Sender<HttpTrigger>) -> Router {
    let state = AppState { tx };

    Router::new()
        .route("/get/:key", get(get_endpoint))
        .route("/put/:key", put(put_endpoint))
        .route("/cas/:key", post(cas_endpoint))
        .with_state(state)
}
