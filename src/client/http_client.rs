use axum::{
    extract::{Path, State},
    routing::get,
    Router,
};
use tokio::sync::mpsc;

#[derive(Clone)]
struct AppState {
    tx: mpsc::Sender<u32>,
}

// The handler now extracts the 'value' from the URL AND the 'tx' from State
async fn handler(Path(value): Path<u32>, State(state): State<AppState>) -> &'static str {
    println!("HTTP received value: {}", value);

    // Send the value to the Client loop
    let _ = state.tx.send(value).await;

    "Value sent to Client!"
}
pub struct HttpEndpoint {
    pub base_path: String,
    pub app: Router,
    listener: tokio::net::TcpListener,
}

impl HttpEndpoint {
    pub async fn new(path: &str, tx: mpsc::Sender<u32>,port:u16) -> Self {
        let state = AppState { tx };
        let app = Router::new()
            .route(&format!("{}/:value", path), get(handler))
            .with_state(state);
        let addr = format!("0.0.0.0:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr).await.expect("Port already in use");
        // let listener: tokio::net::TcpListener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
        HttpEndpoint {
            base_path: path.to_string(),
            app,
            listener,
        }
    }
    pub async fn serve(self) {
        println!(
            "Listening on: http://{}",
            self.listener.local_addr().unwrap()
        );
        axum::serve(self.listener, self.app).await.unwrap();
    }
}
