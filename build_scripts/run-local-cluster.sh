#!/bin/bash

usage="Usage: run-local-cluster.sh"
cluster_size=3
rust_log="info"

# Clean up child processes
interrupt() {
    pkill -P $$
}
trap "interrupt" SIGINT

# Servers' output is saved into logs dir
local_experiment_dir="/home/a1bderrahmane/omnipaxos-kv-fork/build_scripts/logs"
mkdir -p "${local_experiment_dir}"

# Run servers
cluster_config_path="/home/a1bderrahmane/omnipaxos-kv-fork/build_scripts/cluster-config.toml"
for ((i = 1; i <= cluster_size; i++)); do
    server_config_path="/home/a1bderrahmane/omnipaxos-kv-fork/build_scripts/server-${i}-config.toml"
    RUST_LOG=$rust_log SERVER_CONFIG_FILE=$server_config_path CLUSTER_CONFIG_FILE=$cluster_config_path cargo run --manifest-path="/home/a1bderrahmane/omnipaxos-kv-fork/Cargo.toml" --bin server &
done
wait

