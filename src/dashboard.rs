use crate::config::{short_token, Config};
use crate::db::Db;
use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
struct AppState {
    db: Db,
    /// token → display name (or short token).
    accounts: HashMap<String, String>,
}

impl AppState {
    fn lookup(&self, token: &str) -> Option<&str> {
        self.accounts.get(token).map(|s| s.as_str())
    }
}

pub async fn serve(cfg: &Config, db: Db) -> Result<()> {
    let mut accounts = HashMap::new();
    for a in &cfg.accounts {
        let token = a.token()?;
        let label = a.display_name(&token);
        accounts.insert(token, label);
    }

    let state = Arc::new(AppState { db, accounts });

    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/:token", get(account_page))
        .route("/:token/api/latest", get(api_latest))
        .route("/:token/api/history", get(api_history))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", cfg.server.host, cfg.server.port).parse()?;
    tracing::info!(%addr, "dashboard listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn index() -> Html<&'static str> {
    Html(LANDING_HTML)
}

async fn account_page(
    State(s): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Html<String>, ApiError> {
    let label = s.lookup(&token).ok_or(ApiError::NotFound)?;
    Ok(Html(render_account_page(label, &token)))
}

#[derive(Serialize)]
struct LatestEntry {
    label: String,
    token_short: String,
    timestamp_utc: String,
    balance_usd: Option<f64>,
    msisdn: Option<String>,
    last_update: Option<String>,
    status: String,
    note: Option<String>,
}

async fn api_latest(
    State(s): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<Option<LatestEntry>>, ApiError> {
    let label = s.lookup(&token).ok_or(ApiError::NotFound)?.to_string();
    let row = s.db.latest_for(&token)?;
    Ok(Json(row.map(|r| LatestEntry {
        label,
        token_short: short_token(&r.account_token),
        timestamp_utc: r.timestamp_utc.to_rfc3339(),
        balance_usd: r.balance_usd,
        msisdn: r.msisdn,
        last_update: r.last_update,
        status: r.status,
        note: r.note,
    })))
}

#[derive(Deserialize)]
struct HistoryParams {
    /// RFC 3339 timestamp; default = 30 days ago.
    since: Option<String>,
    /// Max points; default 5000, capped at 20000.
    limit: Option<usize>,
}

#[derive(Serialize)]
struct HistoryPoint {
    t: String,
    balance_usd: Option<f64>,
    status: String,
}

async fn api_history(
    State(s): State<Arc<AppState>>,
    Path(token): Path<String>,
    Query(p): Query<HistoryParams>,
) -> Result<Json<Vec<HistoryPoint>>, ApiError> {
    s.lookup(&token).ok_or(ApiError::NotFound)?;
    let since = match p.since.as_deref() {
        Some(raw) => Some(
            chrono::DateTime::parse_from_rfc3339(raw)
                .map_err(|e| ApiError::BadRequest(format!("invalid `since`: {e}")))?
                .with_timezone(&chrono::Utc),
        ),
        None => Some(chrono::Utc::now() - chrono::Duration::days(30)),
    };
    let limit = p.limit.unwrap_or(5000).min(20_000);
    let rows = s.db.history(&token, since, limit)?;
    Ok(Json(
        rows.into_iter()
            .map(|r| HistoryPoint {
                t: r.timestamp_utc.to_rfc3339(),
                balance_usd: r.balance_usd,
                status: r.status,
            })
            .collect(),
    ))
}

enum ApiError {
    NotFound,
    BadRequest(String),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found").into_response(),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            ApiError::Internal(e) => {
                tracing::error!(error = %e, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            }
        }
    }
}

const LANDING_HTML: &str = r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>silent-balance-tracker</title>
<style>body{font-family:-apple-system,sans-serif;max-width:40rem;margin:4rem auto;padding:0 1rem;color:#222}code{background:#f0f0f0;padding:0 .25rem;border-radius:3px}</style>
</head><body>
<h1>silent-balance-tracker</h1>
<p>Per-account dashboards live at <code>/&lt;TOKEN&gt;</code>, where <code>&lt;TOKEN&gt;</code> is the silent.link order token.</p>
</body></html>
"##;

fn render_account_page(label: &str, token: &str) -> String {
    // label is interpolated into HTML (HTML-escape it).
    // token goes into a JS string literal (use serde_json to JSON-encode it,
    // which produces a safely-quoted string that's also valid JS).
    let label_e = html_escape(label);
    let token_json = serde_json::to_string(token).unwrap_or_else(|_| "\"\"".to_string());
    ACCOUNT_PAGE_TEMPLATE
        .replace("{{LABEL}}", &label_e)
        .replace("{{TOKEN_JSON}}", &token_json)
}

const ACCOUNT_PAGE_TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>{{LABEL}} — silent-balance-tracker</title>
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <meta name="robots" content="noindex,nofollow" />
  <script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js"></script>
  <script src="https://cdn.jsdelivr.net/npm/chartjs-adapter-date-fns@3.0.0/dist/chartjs-adapter-date-fns.bundle.min.js"></script>
  <style>
    :root { color-scheme: light dark; }
    body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; max-width: 1100px; margin: 2rem auto; padding: 0 1rem; }
    h1 { margin-bottom: 0.25rem; }
    .sub { color: #666; margin-top: 0; font-size: 0.9rem; }
    .stats { display: flex; flex-wrap: wrap; gap: 2rem; margin: 1rem 0 2rem; }
    .stat { min-width: 9rem; }
    .stat .label { color: #888; font-size: 0.8rem; text-transform: uppercase; letter-spacing: 0.05em; }
    .stat .value { font-size: 1.6rem; font-weight: 600; }
    .status-ok { color: #1b8a3a; }
    .status-error { color: #b00020; }
    .controls { margin-bottom: 1rem; }
    .controls label { margin-right: 1rem; font-size: 0.9rem; }
    .chart-wrap { position: relative; height: 420px; }
    .note { background: rgba(176,0,32,0.08); border-left: 3px solid #b00020; padding: 0.5rem 0.75rem; font-size: 0.85rem; margin-top: 1rem; white-space: pre-wrap; word-break: break-word; }
    code { background: rgba(0,0,0,0.06); padding: 0 0.25rem; border-radius: 3px; }
  </style>
</head>
<body>
  <h1>{{LABEL}}</h1>
  <p class="sub">silent.link order <code id="tok"></code></p>

  <div class="stats">
    <div class="stat"><div class="label">balance</div><div class="value" id="balance">…</div></div>
    <div class="stat"><div class="label">status</div><div class="value" id="status">…</div></div>
    <div class="stat"><div class="label">MSISDN</div><div class="value" id="msisdn">…</div></div>
    <div class="stat"><div class="label">silent.link last update</div><div class="value" id="lastupdate">…</div></div>
    <div class="stat"><div class="label">polled (UTC)</div><div class="value" id="polled">…</div></div>
  </div>

  <div id="errnote" class="note" style="display:none"></div>

  <h2>History</h2>
  <div class="controls">
    <label>Range:
      <select id="range">
        <option value="1">last 24h</option>
        <option value="7">last 7d</option>
        <option value="30" selected>last 30d</option>
        <option value="90">last 90d</option>
      </select>
    </label>
  </div>
  <div class="chart-wrap"><canvas id="chart"></canvas></div>

  <script>
    const TOKEN = {{TOKEN_JSON}};
    document.getElementById('tok').textContent = TOKEN.length > 12
      ? TOKEN.slice(0, 6) + '…' + TOKEN.slice(-4)
      : TOKEN;

    async function loadLatest() {
      const res = await fetch(`/${TOKEN}/api/latest`);
      if (!res.ok) return;
      const r = await res.json();
      const setText = (id, v) => document.getElementById(id).textContent = (v == null || v === '') ? '—' : v;
      if (!r) {
        ['balance','status','msisdn','lastupdate','polled'].forEach(id => setText(id, null));
        document.getElementById('balance').textContent = 'no data yet';
        return;
      }
      setText('balance', r.balance_usd != null ? `$${r.balance_usd}` : null);
      const statusEl = document.getElementById('status');
      statusEl.textContent = r.status;
      statusEl.className = 'value status-' + (r.status === 'ok' ? 'ok' : 'error');
      setText('msisdn', r.msisdn);
      setText('lastupdate', r.last_update);
      setText('polled', r.timestamp_utc);
      const note = document.getElementById('errnote');
      if (r.note) { note.textContent = r.note; note.style.display = 'block'; }
      else { note.style.display = 'none'; }
    }

    let chart;
    async function loadChart() {
      const days = parseInt(document.getElementById('range').value, 10);
      const since = new Date(Date.now() - days * 86400000).toISOString();
      const r = await fetch(`/${TOKEN}/api/history?since=${encodeURIComponent(since)}`);
      if (!r.ok) return;
      const points = await r.json();
      const data = points.filter(p => p.balance_usd != null).map(p => ({ x: p.t, y: p.balance_usd }));
      const ctx = document.getElementById('chart');
      if (chart) chart.destroy();
      chart = new Chart(ctx, {
        type: 'line',
        data: { datasets: [{
          label: 'balance (USD)',
          data,
          borderColor: '#3366cc',
          backgroundColor: '#3366cc33',
          tension: 0.15,
          spanGaps: true,
        }] },
        options: {
          responsive: true,
          maintainAspectRatio: false,
          parsing: false,
          interaction: { mode: 'nearest', intersect: false },
          scales: {
            x: { type: 'time', time: { tooltipFormat: 'yyyy-MM-dd HH:mm' } },
            y: { title: { display: true, text: 'balance (USD)' } },
          },
          plugins: { legend: { display: false } },
        },
      });
    }

    document.getElementById('range').addEventListener('change', loadChart);
    loadLatest();
    loadChart();
    setInterval(() => { loadLatest(); loadChart(); }, 60000);
  </script>
</body>
</html>
"#;

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
