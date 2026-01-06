#!/usr/bin/env bash
set -euo pipefail

# Orchestrator end-to-end test script (deployment paths)
# Usage: ./tests/run_orchestrator_test.sh

REPO="$(cd "$(dirname "$0")/.." && pwd)"
# Use repository-local deployment paths for logs and pidfiles
ORCH_CONF="$REPO/deployment/tmp/orchestrator.toml"
LOG="$REPO/deployment/log/adcp-orchestrator.log"
PIDFILE="$REPO/deployment/tmp/orchestrator.pid"

# Cleanup previous artifacts
rm -f "$REPO/deployment/tmp/adcp_fifo" "$REPO/deployment/tmp/adcp_*_hb" "$LOG" "$PIDFILE" "$ORCH_CONF"
rm -rf "$REPO/deployment/to_process" "$REPO/deployment/processed" "$REPO/deployment/data" "$REPO/deployment/backup" "$REPO/deployment/tmp" "$REPO/deployment/log"
mkdir -p "$REPO/deployment/tmp" "$REPO/deployment/log"

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

# Cleanup any leftover pid files from child services
rm -f "$REPO/deployment/tmp/adcp-"*.pid || true
rm -f "$PIDFILE" || true

# Show results
echo "--- deployment/to_process ---"
[ -d "$REPO/deployment/to_process" ] && ls -R "$REPO/deployment/to_process" || echo "no to_process"

echo "--- deployment/processed ---"
[ -d "$REPO/deployment/processed" ] && ls -R "$REPO/deployment/processed" || echo "no processed"

echo "--- deployment/data ---"
[ -d "$REPO/deployment/data" ] && ls -R "$REPO/deployment/data" || echo "no data"

echo "--- tail of adcp-orchestrator log ---"
[ -f "$LOG" ] && tail -n 500 "$LOG" || echo "no log"

exit 0
