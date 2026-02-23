use async_trait::async_trait;
use maelstrom::{Node, Runtime,protocol:: Message, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod http_client; 
use http_client::{KvResp, PutBody};

struct ProxyHandler {
    client_port: Mutex<Option<u16>>,
    http_client: reqwest::Client,
}

impl ProxyHandler {
    fn get_port(&self) -> u16 {
        self.client_port.lock().unwrap().expect("Port not initialized yet!")
    }

    fn set_port_from_id(&self, node_id: &str) {
        let id_num: u16 = node_id.replace('n', "").parse().unwrap_or(0);
        let port = 8001 + id_num;
        
        eprintln!("(stderr) Node '{}' initialized. Mapping to Local Port {}", node_id, port);
        *self.client_port.lock().unwrap() = Some(port);
    }
}
// refer to https://github.com/jepsen-io/maelstrom/blob/main/resources/protocol-intro.md for the protocol specifications and the messages handling

// refer to https://github.com/jepsen-io/maelstrom/tree/main/demo/rust for Node implementation using maelstrom
#[async_trait]
impl Node for ProxyHandler {
    async fn process(&self, runtime: Runtime, req: Message) -> Result<()> {
        let body: Value = req.body.clone().as_obj()?;
        
        let type_str = body["type"].as_str().unwrap_or("");

        match type_str {
            "init" => {
                let node_id = body["node_id"].as_str().unwrap_or("n0");
                
                self.set_port_from_id(node_id);

                runtime.reply(req, serde_json::json!({"type": "init_ok"})).await
            }
            "read" => {
                let port = self.get_port(); 
                let key = body["key"].as_str().unwrap_or("0");
                let url = format!("http://127.0.0.1:{}/get/{}", port, key);
                
                match self.http_client.get(&url).timeout(Duration::from_secs(5)).send().await {
                    Ok(resp) => {
                        match resp.json::<KvResp>().await {
                            Ok(kv) => runtime.reply(req, serde_json::json!({"type": "read_ok", "value": kv.value})).await,
                            Err(_) => runtime.reply(req, serde_json::json!({"type": "error", "code": 13, "text": "Bad JSON from Shim"})).await
                        }
                    }
                    Err(_) => runtime.reply(req, serde_json::json!({"type": "error", "code": 14, "text": "Timeout"})).await
                }
            }
            "write" => {
                let port = self.get_port();
                let key = body["key"].as_str().unwrap_or("0");
                let val_str = body["value"].to_string().replace('"', ""); 

                let url = format!("http://127.0.0.1:{}/put/{}", port, key);
                
                match self.http_client.put(&url).json(&PutBody { value: val_str }).timeout(Duration::from_secs(5)).send().await {
                    Ok(_) => runtime.reply(req, serde_json::json!({"type": "write_ok"})).await,
                    Err(_) => runtime.reply(req, serde_json::json!({"type": "error", "code": 14, "text": "Timeout"})).await
                }
            }
            _ => Ok(()) 
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {

    let handler = Arc::new(ProxyHandler { 
        client_port: Mutex::new(None), 
        http_client: reqwest::Client::new(),
    });
    
    Runtime::new().with_handler(handler).run().await
}