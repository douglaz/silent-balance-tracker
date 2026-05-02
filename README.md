# silent-balance-tracker

Polls [silent.link](https://silent.link) order balances on a schedule for one or more accounts, stores readings in SQLite, and serves a per-account dashboard at `/<TOKEN>`.

The token in the URL is the same token from the silent.link order URL — knowing the URL is auth.

```
silent.link/order/<TOKEN>
                  ↓ same token ↓
silent.galtland.io/<TOKEN>
```

Replaces `legacy/silent_balance_log.sh` (single token, CSV).

## Configuration

Each account has a `url` (the full silent.link order URL, the token is parsed out) and an optional friendly `name`:

```toml
[database]
path = "./silent_balance.sqlite"

[server]
host = "0.0.0.0"
port = 8080

[poller]
interval_seconds = 300
request_timeout_seconds = 20

[[accounts]]
name = "primary"
url = "https://silent.link/order/AAA..."

[[accounts]]
url = "https://silent.link/order/BBB..."   # name is optional; falls back to a short token
```

For deployments where you don't want secrets in the config file, set `SILENT_ACCOUNTS_JSON` instead — it overrides any `[[accounts]]` from the file:

```
SILENT_ACCOUNTS_JSON='[{"name":"primary","url":"https://silent.link/order/AAA"}]'
```

## Running locally

```bash
nix develop
cp config.example.toml config.toml   # edit URLs
cargo run -- serve --config ./config.toml
# then open http://localhost:8080/<TOKEN>
```

One-shot poll (e.g. for testing):

```bash
cargo run -- poll --config ./config.toml
```

## CLI

- `serve` — runs the HTTP dashboard *and* a background poll loop in the same process. Default mode for k8s.
- `poll` — runs a single poll cycle for every configured account, then exits.

## HTTP routes

- `GET /` — minimal landing page (does not enumerate configured accounts)
- `GET /<TOKEN>` — dashboard HTML for that one account
- `GET /<TOKEN>/api/latest` — latest snapshot
- `GET /<TOKEN>/api/history?since=RFC3339&limit=N` — history points (default `since` = 30 days ago)
- `GET /healthz` — liveness/readiness

Tokens not in the configured list return 404. The token acts as a bearer secret — anyone who knows the silent.link order URL already has full access via silent.link itself, so this site exposes no more than that.

## Deployment (k8s on the galtland.io cluster)

`k8s/silent-balance-tracker.yaml` creates:

- `Namespace` `silent-balance-tracker`
- `Secret` `silent-accounts` — `SILENT_ACCOUNTS_JSON` source (real URLs go here, **don't commit them**)
- `ConfigMap` `silent-balance-tracker-config` — non-sensitive config.toml
- `PersistentVolumeClaim` `silent-balance-tracker-data` (1Gi, RWO, `local-path`) for the SQLite file
- `Deployment` (single replica, `Recreate` strategy)
- `Service` ClusterIP on port 8080
- `Ingress` for `silent.galtland.io` (nginx ingressClassName, cert-manager letsencrypt-prod, TLS secret `silent-balance-tracker-tls`) — same convention as the other apps in the cluster

Image is published by CI to `ghcr.io/douglaz/silent-balance-tracker:<tag>`.

To deploy via ArgoCD:

1. Edit the `silent-accounts` Secret with real URLs (don't commit).
2. Drop the manifest into `~/newmachine/remote-devops/k8s/argo/silent-balance-tracker/silent-balance-tracker.yaml`, commit, push.
3. Add a DNS record for `silent.galtland.io` pointing at the cluster.

## Building

```bash
# Static musl binary
nix build .#default

# Docker image (uses nix-built static binary)
nix build .#dockerImage
docker load < result
docker run --rm -p 8080:8080 \
  -e SILENT_ACCOUNTS_JSON='[{"name":"primary","url":"https://silent.link/order/AAA"}]' \
  -v $PWD/data:/data -e DATABASE_PATH=/data/silent.sqlite \
  silent-balance-tracker:latest
```

## Schema

```sql
CREATE TABLE balance_log (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  account_token TEXT    NOT NULL,
  timestamp_utc TEXT    NOT NULL,   -- RFC3339
  balance_usd   REAL,
  msisdn        TEXT,
  last_update   TEXT,
  status        TEXT    NOT NULL,
  note          TEXT
);
CREATE INDEX idx_balance_log_token_time ON balance_log(account_token, timestamp_utc);
```

A row is written every poll cycle for every account. When the poll fails, `status="error"` and the error message lands in `note`.
