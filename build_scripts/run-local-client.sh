#!/bin/bash

client1_id=1
client2_id=2
rust_log="debug"

# Clean up child processes
interrupt() {
    pkill -P $$
}
trap "interrupt" SIGINT

# Clients' output is saved into logs dir
local_experiment_dir="/home/a1bderrahmane/omnipaxos-kv-fork/build_scripts/logs"
mkdir -p "${local_experiment_dir}"

# Run clients
client1_config_path="/home/a1bderrahmane/omnipaxos-kv-fork/build_scripts/client-${client1_id}-config.toml"
client2_config_path="/home/a1bderrahmane/omnipaxos-kv-fork/build_scripts/client-${client2_id}-config.toml"
RUST_LOG=$rust_log CONFIG_FILE="$client1_config_path"  cargo run --manifest-path="/home/a1bderrahmane/omnipaxos-kv-fork/Cargo.toml" --bin client &
RUST_LOG=$rust_log CONFIG_FILE="$client2_config_path"  cargo run --manifest-path="/home/a1bderrahmane/omnipaxos-kv-fork/Cargo.toml" --bin client
