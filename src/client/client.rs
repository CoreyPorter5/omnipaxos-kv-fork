use crate::{
    configs::ClientConfig,
    data_collection::ClientData,
    network::Network,
    http_client::{router, HttpTrigger, KvResp},
};
use chrono::Utc;
use log::*;
use omnipaxos_kv::common::{kv::*, messages::*};
use rand::Rng;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::interval;

const NETWORK_BATCH_SIZE: usize = 100;

pub struct Client {
    id: ClientId,
    network: Network,
    client_data: ClientData,
    config: ClientConfig,
    active_server: NodeId,
    final_request_count: Option<usize>,
    next_request_id: usize,
    pending_http_queries: HashMap<usize, oneshot::Sender<String>>,
}

impl Client {
    pub async fn new(config: ClientConfig) -> Self {
        let network = Network::new(
            vec![(config.server_id, config.server_address.clone())],
            NETWORK_BATCH_SIZE,
        )
        .await;
        Client {
            id: config.server_id,
            network,
            client_data: ClientData::new(),
            active_server: config.server_id,
            config,
            final_request_count: None,
            next_request_id: 0,
            pending_http_queries: HashMap::new(),
        }
    }

    pub async fn run(&mut self) {
        // Wait for server to signal start
        info!("{}: Waiting for start signal from server", self.id);
        match self.network.server_messages.recv().await {
            Some(ServerMessage::StartSignal(start_time)) => {
                Self::wait_until_sync_time(&mut self.config, start_time).await;
            }
            _ => panic!("Error waiting for start signal"),
        }

        // Early end
        let intervals = self.config.requests.clone();
        if intervals.is_empty() {
            self.save_results().expect("Failed to save results");
            return;
        }

        // Initialize intervals
        let mut rng = rand::thread_rng();
        let mut intervals = intervals.iter();
        let first_interval = intervals.next().unwrap();
        let mut read_ratio = first_interval.get_read_ratio();
        let mut request_interval = interval(first_interval.get_request_delay());
        let mut next_interval = interval(first_interval.get_interval_duration());
        let _ = next_interval.tick().await;

        // Main event loop
        info!("{}: Starting requests", self.id);
        loop {
            tokio::select! {
                biased;
                Some(msg) = self.network.server_messages.recv() => {
                    self.handle_server_message(msg);
                    if self.run_finished() {
                        break;
                    }
                }
                _ = request_interval.tick(), if self.final_request_count.is_none() => {
                    let is_write = rng.gen::<f64>() > read_ratio;
                    self.send_request(is_write).await;
                },
                _ = next_interval.tick() => {
                    match intervals.next() {
                        Some(new_interval) => {
                            read_ratio = new_interval.read_ratio;
                            next_interval = interval(new_interval.get_interval_duration());
                            next_interval.tick().await;
                            request_interval = interval(new_interval.get_request_delay());
                        },
                        None => {
                            self.final_request_count = Some(self.client_data.request_count());
                            if self.run_finished() {
                                break;
                            }
                        },
                    }
                },
            }
        }

        info!(
            "{}: Client finished: collected {} responses",
            self.id,
            self.client_data.response_count(),
        );
        self.network.shutdown();
        self.save_results().expect("Failed to save results");
    }

    // pub async fn run_2(&mut self) {
    //     info!("{}: Waiting for start signal from server", self.id);
    //     match self.network.server_messages.recv().await {
    //         Some(ServerMessage::StartSignal(start_time)) => {
    //             Self::wait_until_sync_time(&mut self.config, start_time).await;
    //         }
    //         _ => panic!("Error waiting for start signal"),
    //     }

    //     // 1. Change channel type to HttpTrigger to allow two-way communication
    //     let (tx, mut rx) = mpsc::channel::<HttpTrigger>(100);
    //     let my_port = 8000 + (self.id as u16);
    //     let endpoint = HttpEndpoint::new("/trigger", tx, my_port).await;

    //     tokio::spawn(async move {
    //         endpoint.serve().await;
    //     });

    //     info!(
    //         "{}: HTTP server spawned on port {}. Entering event loop.",
    //         self.id, my_port
    //     );

    //     loop {
    //         tokio::select! {
    //             //  Receive the trigger which contains the value AND the response channel
    //             Some(trigger) = rx.recv() => {
    //                 info!("Client received {} from HTTP. Sending request to server...", trigger.value);

    //                 let req_id = self.next_request_id;

    //                 // Store the "callback" channel
    //                 self.pending_http_queries.insert(req_id, trigger.response_tx);

    //                 // Send the request to the server cluster
    //                 self.send_custom_request(trigger.value).await;
    //             }

    //             // Existing network logic
    //             Some(msg) = self.network.server_messages.recv() => {
    //                 // self.handle_server_message(msg);
    //                 self.handle_server_message_with_http(msg);
    //             }
    //         }
    //     }
    // }


    pub async fn run_2(&mut self) {
        // Channel for HTTP -> client loop
        let (tx, mut rx) = mpsc::channel::<HttpTrigger>(100);

        // Spawn HTTP server for THIS client
        let my_port = 8000 + (self.id as u16);
        let app = router(tx);
        let addr = format!("0.0.0.0:{}", my_port);
        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
            axum::serve(listener, app.into_make_service()).await.unwrap();
        });


        info!("{}: HTTP server spawned on port {}. Entering event loop.", self.id, my_port);

        // Map: command_id -> oneshot sender waiting to answer the HTTP request
        let mut pending: HashMap<CommandId, oneshot::Sender<KvResp>> = HashMap::new();

        loop {
            tokio::select! {
                // 1) HTTP request arrives -> turn into ClientMessage::Append
                Some(trigger) = rx.recv() => {
                    let cmd_id: CommandId = self.next_request_id as CommandId;
                    self.next_request_id += 1;

                    // Store how to reply to this HTTP call
                    pending.insert(cmd_id, trigger.response_tx);

                    // Send to server using your existing client networking
                    let request = ClientMessage::Append(cmd_id, trigger.cmd);
                    debug!("HTTP -> sending {request:?}");
                    self.network.send(self.active_server, request).await;
                }

                // 2) Server reply arrives -> complete the matching HTTP call
                Some(msg) = self.network.server_messages.recv() => {
                    debug!("Received {msg:?}");

                    match msg {
                        ServerMessage::Write(id) => {
                            if let Some(tx) = pending.remove(&id) {
                                let _ = tx.send(KvResp {
                                    ok: true,
                                    value: None,
                                    swapped: None,
                                    error: None,
                                });
                            }
                        }

                        ServerMessage::Read(id, value_opt) => {
                            if let Some(tx) = pending.remove(&id) {
                                let _ = tx.send(KvResp {
                                    ok: true,
                                    value: value_opt,
                                    swapped: None,
                                    error: None,
                                });
                            }
                        }

                        // If you add CAS, you'll likely want a response type for it.
                        // If you encode CAS as Write + extra info, adjust here.
                        ServerMessage::StartSignal(_) => {}
                    }
                }
            }
        }
    }

    fn handle_server_message(&mut self, msg: ServerMessage) {
        debug!("Recieved {msg:?}");
        match msg {
            ServerMessage::StartSignal(_) => (),
            server_response => {
                let cmd_id = server_response.command_id();
                self.client_data.new_response(cmd_id);
            }
        }
    }
    // In src/client/client.rs
    fn handle_server_message_with_http(&mut self, msg: ServerMessage) {
        let cmd_id = msg.command_id();

        // Check if this ID belongs to a waiting HTTP request
        if let Some(response_tx) = self.pending_http_queries.remove(&cmd_id) {
            println!("{:?}", msg);
            let response_data = format!("Server replied: {:?}", msg);
            match msg {
                ServerMessage::Read(id, Some(value)) => {
                    info!("Read Success (ID: {}): Found value '{}'", id, value);
                    let _ = response_tx.send(value);
                }
                _ => {
                    error!("Unexpected message type received: {:?}", msg);
                }
            }
        }

        // Continue with existing data collection logic
        self.client_data.new_response(cmd_id);
    }
    async fn send_custom_request(&mut self, value: u32) {
        let key = value.to_string();
        let cmd = KVCommand::Get(key);

        let request = ClientMessage::Append(self.next_request_id, cmd);
        debug!("Sending {request:?}");
        self.network.send(self.active_server, request).await;
        self.client_data.new_request(false);
        self.next_request_id += 1;
    }
    async fn send_request(&mut self, is_write: bool) {
        let key = self.next_request_id.to_string();
        let cmd = match is_write {
            true => KVCommand::Put(key.clone(), key),
            false => KVCommand::Get(key),
        };
        let request = ClientMessage::Append(self.next_request_id, cmd);
        debug!("Sending {request:?}");
        self.network.send(self.active_server, request).await;
        self.client_data.new_request(is_write);
        self.next_request_id += 1;
    }

    fn run_finished(&self) -> bool {
        if let Some(count) = self.final_request_count {
            if self.client_data.request_count() >= count {
                return true;
            }
        }
        return false;
    }

    // Wait until the scheduled start time to synchronize client starts.
    // If start time has already passed, start immediately.
    async fn wait_until_sync_time(config: &mut ClientConfig, scheduled_start_utc_ms: i64) {
        // // Desync the clients a bit
        // let mut rng = rand::thread_rng();
        // let scheduled_start_utc_ms = scheduled_start_utc_ms + rng.gen_range(1..100);
        let now = Utc::now();
        let milliseconds_until_sync = scheduled_start_utc_ms - now.timestamp_millis();
        config.sync_time = Some(milliseconds_until_sync);
        if milliseconds_until_sync > 0 {
            tokio::time::sleep(Duration::from_millis(milliseconds_until_sync as u64)).await;
        } else {
            warn!("Started after synchronization point!");
        }
    }

    fn save_results(&self) -> Result<(), std::io::Error> {
        self.client_data.save_summary(self.config.clone())?;
        self.client_data
            .to_csv(self.config.output_filepath.clone())?;
        Ok(())
    }
}