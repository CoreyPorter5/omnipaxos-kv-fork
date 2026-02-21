use omnipaxos_kv::common::kv::KVCommand;
use std::collections::HashMap;

pub struct Database {
    db: HashMap<String, String>,
}

impl Database {
    pub fn new() -> Self {
        let mut db = HashMap::new();
        db.insert("1".to_string(), "2".to_string());
        Self { db }
    }

    pub fn handle_command(&mut self, command: KVCommand) -> Option<Option<String>> {
        match command {
            KVCommand::Put(key, value) => {
                self.db.insert(key, value);
                None
            }
            KVCommand::Delete(key) => {
                self.db.remove(&key);
                None
            }
            KVCommand::Get(key) => Some(self.db.get(&key).map(|v| v.clone())),
            KVCommand::Cas {
                key,
                from,
                to,
                create_if_not_exists,
            } => {
                let swapped = match self.db.get(&key) {
                    Some(current_value) => {
                        if *current_value == from {
                            self.db.insert(key, to);
                            true
                        } else {
                            false
                        }
                    }
                    None => {
                        if create_if_not_exists && from.is_empty() {
                            self.db.insert(key, to);
                            true
                        } else {
                            false
                        }
                    }
                };
                Some(Some(swapped.to_string()))
            }
        }
    }
}
