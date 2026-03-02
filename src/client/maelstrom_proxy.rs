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

    fn parse_key(v: &Value) -> String {
        if let Some(s) = v.as_str(){
            s.to_string()
        }else if let Some(n) = v.as_i64() {
            n.to_string()
        } else {
            panic!("Unexpected key format: {:?}", v)
        }
    }

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
                let key = Self::parse_key(&body["key"]);
                let url = format!("http://127.0.0.1:{}/get/{}", port, key);

                match self.http_client
                    .get(&url)
                    .timeout(Duration::from_secs(5))
                    .send()
                    .await
                {
                    Ok(resp) => {
                        match resp.json::<KvResp>().await {
                            Ok(kv) => {
                                match kv.value {
                                    Some(value_str) => {
                                        match value_str.parse::<i64>() {
                                            Ok(parsed) => {
                                                runtime.reply(
                                                    req,
                                                    serde_json::json!({
                                            "type": "read_ok",
                                            "value": parsed
                                        })
                                                ).await
                                            }
                                            Err(_) => runtime.reply(
                                                req,
                                                serde_json::json!({
                                        "type": "error",
                                        "code": 13,
                                        "text": "Invalid integer from Shim"
                                    })
                                            ).await
                                        }
                                    }
                                    None => {
                                        runtime.reply(
                                            req,
                                            serde_json::json!({
                                    "type": "read_ok",
                                    "value": null
                                })
                                        ).await
                                    }
                                }
                            }
                            Err(_) => runtime.reply(
                                req,
                                serde_json::json!({
                        "type": "error",
                        "code": 13,
                        "text": "Bad JSON from Shim"
                    })
                            ).await
                        }
                    }
                    Err(_) => runtime.reply(
                        req,
                        serde_json::json!({
                "type": "error",
                "code": 14,
                "text": "Timeout"
            })
                    ).await
                }
            }
            "write" => {
                let port = self.get_port();
                let key = Self::parse_key(&body["key"]);
                let val_str = body["value"].as_i64().map(|v| v.to_string()).unwrap_or_else(||panic!("Expected integer value for write command, got: {:?}", body["value"]));

                let url = format!("http://127.0.0.1:{}/put/{}", port, key);
                
                match self.http_client.put(&url).json(&PutBody { value: val_str }).timeout(Duration::from_secs(5)).send().await {
                    Ok(_) => runtime.reply(req, serde_json::json!({"type": "write_ok"})).await,
                    Err(_) => runtime.reply(req, serde_json::json!({"type": "error", "code": 14, "text": "Timeout"})).await
                }
            }
            "cas" => {
                let port = self.get_port();
                let key = Self::parse_key(&body["key"]);
                let from = body["from"]
                    .as_i64()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let to = body["to"]
                    .as_i64()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let create = body["create_if_not_exists"]
                    .as_bool()
                    .unwrap_or(false);

                let url = format!("http://127.0.0.1:{}/cas/{}", port, key);

                let body = serde_json::json!({
                    "from": from,
                    "to": to,
                    "create_if_not_exists": create
                });

                match self.http_client
                    .post(&url)
                    .json(&body)
                    .timeout(Duration::from_secs(5))
                    .send()
                    .await
                {
                    Ok(resp) => {
                        match resp.json::<KvResp>().await {
                            Ok(kv) => {
                                if kv.swapped == Some(true) {
                                    runtime.reply(req, serde_json::json!({
                            "type": "cas_ok"
                        })).await
                                } else {
                                    runtime.reply(req, serde_json::json!({
                            "type": "error",
                            "code": 22,
                            "text": "precondition failed"
                        })).await
                                }
                            }
                            Err(_) => runtime.reply(req, serde_json::json!({
                                "type": "error",
                                "code": 13,
                                "text": "Bad JSON from Shim"
                            })).await
                                    }
                                }
                                Err(_) => runtime.reply(req, serde_json::json!({
                                    "type": "error",
                                    "code": 14,
                                    "text": "Timeout"
                                })).await
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