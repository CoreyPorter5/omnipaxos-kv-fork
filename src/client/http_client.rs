use axum::{
    extract::{Path, State},
    routing::get,
    Router,
};
use tokio::sync::{mpsc, oneshot}; // Import oneshot

// Define a message structure to carry the value and the return channel
pub struct HttpTrigger {
    pub value: u32,
    pub response_tx: oneshot::Sender<String>,
}

#[derive(Clone)]
struct AppState {
    tx: mpsc::Sender<HttpTrigger>, // Changed to send HttpTrigger
}

async fn handler(Path(value): Path<u32>, State(state): State<AppState>) -> String {
    println!("HTTP received value: {}", value);

    // Create a one-shot channel for the response
    let (response_tx, response_rx) = oneshot::channel();

    let trigger = HttpTrigger {
        value,
        response_tx,
    };

    // Send to client loop
    if state.tx.send(trigger).await.is_err() {
        return "Internal Error: Client loop disconnected".to_string();
    }

    // Wait for the response from the client loop
    match response_rx.await {
        Ok(msg) => msg,
        Err(_) => "Error: Main loop failed to respond".to_string(),
    }
}

pub struct HttpEndpoint {
    pub base_path: String,
    pub app: Router,
    listener: tokio::net::TcpListener,
}

impl HttpEndpoint {
    pub async fn new(path: &str, tx: mpsc::Sender<HttpTrigger>, port: u16) -> Self {
        let state = AppState { tx };
        let app = Router::new()
            .route(&format!("{}/:value", path), get(handler))
            .with_state(state);
        let addr = format!("0.0.0.0:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr).await.expect("Port already in use");
        
        HttpEndpoint {
            base_path: path.to_string(),
            app,
            listener,
        }
    }

    pub async fn serve(self) {
        axum::serve(self.listener, self.app).await.unwrap();
    }
}