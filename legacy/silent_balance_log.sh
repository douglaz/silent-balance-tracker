#!/usr/bin/env bash
set -euo pipefail

TOKEN="${SILENT_TOKEN:?SILENT_TOKEN must be set}"
LOG_FILE="${SILENT_LOG_FILE:-/home/user/silent_balance_log.csv}"
LOCK_FILE="${SILENT_LOCK_FILE:-/tmp/silent_balance_log.lock}"
BASE_URL="https://silent.link"

exec 9>"$LOCK_FILE"
flock -n 9 || { echo "another run in progress, exiting" >&2; exit 0; }

ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

if [[ ! -f "$LOG_FILE" ]]; then
  echo "timestamp_utc,balance_usd,msisdn,last_update,status,note" > "$LOG_FILE"
fi

write_row() {
  local balance=$1 msisdn=$2 lastupdate=$3 status=$4 note=$5
  printf '%s,%s,%s,%s,%s,%s\n' "$ts" "$balance" "$msisdn" "$lastupdate" "$status" "$note" >> "$LOG_FILE"
}

order=$(curl -sS --max-time 20 -X POST "$BASE_URL/api/v1/order" \
  -H 'Content-Type: application/json' \
  -d "{\"token\":\"$TOKEN\"}") || {
  write_row "" "" "" "error" "order_request_failed"
  exit 1
}

imsi=$(jq -r '.data.imsi // empty' <<<"$order")
if [[ -z "$imsi" ]]; then
  write_row "" "" "" "error" "no_imsi"
  exit 1
fi

bal=$(curl -sS --max-time 20 -X POST "$BASE_URL/api/v1/checkbalance" \
  -H 'Content-Type: application/json' \
  -d "{\"phone\":\"$imsi\",\"token\":\"$TOKEN\"}") || {
  write_row "" "" "" "error" "balance_request_failed"
  exit 1
}

status=$(jq -r '.status // "unknown"' <<<"$bal")
balance=$(jq -r '.data.BALANCE // empty' <<<"$bal")
msisdn=$(jq -r '.data.MSISDN // empty' <<<"$bal")
lastupdate=$(jq -r '.data.LASTUPDATE // empty' <<<"$bal" | tr ',' ' ')

write_row "$balance" "$msisdn" "$lastupdate" "$status" ""
