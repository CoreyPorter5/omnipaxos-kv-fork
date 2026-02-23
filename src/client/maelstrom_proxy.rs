use async_trait::async_trait;
use maelstrom::protocol::Message;
use maelstrom::{Node, Runtime};
mod http_client;
use http_client::{KvResp,PutBody};

// use http_client::{router, HttpTrigger, KvResp};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Read { key: String },
    Write { key: String, value: String },
}

struct MaelstromProxy {
    client_port: u16,
}

#[async_trait]
impl Node for MaelstromProxy {
    async fn process(&self, runtime: Runtime, req: Message) -> maelstrom::Result<()> {
        let body: Request = req.body.as_obj()?;
        match body {
            Request::Read { key } => {
                let url = format!("http://127.0.0.1:{}/get/{}", self.client_port, key);
                let resp: KvResp = reqwest::get(url).await?.json().await?;
                runtime
                    .reply(
                        req,
                        serde_json::json!({"type": "read_ok", "value": resp.value}),
                    )
                    .await
            }
            Request::Write { key, value } => {
                let url = format!("http://127.0.0.1:{}/put/{}", self.client_port, key);
                reqwest::Client::new()
                    .put(url)
                    .json(&PutBody { value })
                    .send()
                    .await?;
                runtime
                    .reply(req, serde_json::json!({"type": "write_ok"}))
                    .await
            }
        }
    }
}

fn main() {
    let runtime = Runtime::new();
    let handler = MaelstromProxy { client_port: 8001 };
}
