#!/usr/bin/env bash
set -euo pipefail

# Orchestrator end-to-end test script (deployment paths)
# Usage: ./tests/run_orchestrator_test.sh

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ORCH_CONF="/tmp/orch_run.toml"
LOG="/tmp/orch_run.log"
PIDFILE="/tmp/orch_run.pid"

# Cleanup previous artifacts
rm -f /tmp/adcp_fifo /tmp/adcp_*_hb "$LOG" "$PIDFILE" "$ORCH_CONF"
rm -f "$REPO/deployment/tmp/adcp_fifo" "$REPO/deployment/tmp/adcp_*_hb" "$LOG" "$PIDFILE" "$ORCH_CONF"
rm -rf "$REPO/deployment/to_process" "$REPO/deployment/processed" "$REPO/deployment/data" "$REPO/deployment/backup" "$REPO/deployment/tmp"
mkdir -p "$REPO/deployment"

cat > "$ORCH_CONF" <<EOF
service_name = "adcp-orchestrator"
mode = "Orchestrator"
serial_port = "/dev/null"
data_directory = "./deployment/data"
file_stability_seconds = 30
EOF

cd "$REPO"
cargo build --release

# Start orchestrator
./target/release/adcp "$ORCH_CONF" > "$LOG" 2>&1 &
echo $! > "$PIDFILE"
echo "started orchestrator pid $(cat "$PIDFILE")"

# Let it run briefly
sleep 8

# Request graceful shutdown
kill -INT "$(cat "$PIDFILE")" || true
sleep 1
wait "$(cat "$PIDFILE")" || true

# Show results
echo "--- deployment/to_process ---"
[ -d "$REPO/deployment/to_process" ] && ls -R "$REPO/deployment/to_process" || echo "no to_process"

echo "--- deployment/processed ---"
[ -d "$REPO/deployment/processed" ] && ls -R "$REPO/deployment/processed" || echo "no processed"

echo "--- deployment/data ---"
[ -d "$REPO/deployment/data" ] && ls -R "$REPO/deployment/data" || echo "no data"

echo "--- tail of log ---"
[ -f "$LOG" ] && tail -n 500 "$LOG" || echo "no log"

exit 0
