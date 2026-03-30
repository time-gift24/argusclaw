//! Mock OAuth2 server for testing Chrome tool capabilities.
//!
//! Starts two HTTP services:
//!   1. OAuth2 protocol backend (via oauth2-test-server)
//!   2. Interactive HTML frontend (custom axum routes) for browser automation testing
//!
//! Usage:
//!   cargo run -p argus-tool --features mock-oauth2 --bin mock-oauth2 [--port 9090]

use anyhow::Result;
use axum::{
    extract::Query,
    response::{Html, IntoResponse, Redirect},
    routing::get,
    Router,
};
use clap::Parser;
use serde::Deserialize;
use std::net::SocketAddr;
use tokio::net::TcpListener;

/// Mock OAuth2 server configuration.
#[derive(Parser)]
#[command(name = "mock-oauth2", about = "Mock OAuth2 server for Chrome tool testing")]
struct Cli {
    /// Port for the HTML frontend (OAuth2 backend uses port + 1)
    #[arg(short, long, default_value_t = 9090)]
    port: u16,
}

// ---------------------------------------------------------------------------
// HTML page templates
// ---------------------------------------------------------------------------

fn home_page(frontend_base: &str, auth_url: &str, client_id: &str) -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"><title>Mock OAuth2 - Test App</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 680px; margin: 40px auto; padding: 0 20px; color: #333; }}
h1 {{ color: #1a1a2e; }} h2 {{ color: #16213e; }}
a {{ color: #0f3460; }} a:hover {{ color: #e94560; }}
.btn {{ display: inline-block; padding: 12px 28px; background: #0f3460; color: #fff; text-decoration: none; border-radius: 6px; font-size: 16px; margin: 6px; }}
.btn:hover {{ background: #16213e; }}
.btn-danger {{ background: #e94560; }} .btn-danger:hover {{ background: #c81d4e; }}
.btn-success {{ background: #2ecc71; }} .btn-success:hover {{ background: #27ae60; }}
.scenario {{ border: 1px solid #ddd; border-radius: 8px; padding: 16px; margin: 12px 0; }}
.scenario h3 {{ margin-top: 0; }}
code {{ background: #f4f4f4; padding: 2px 6px; border-radius: 3px; }}
.info {{ background: #f0f7ff; border-left: 4px solid #0f3460; padding: 12px 16px; margin: 12px 0; border-radius: 0 6px 6px 0; }}
</style>
</head>
<body>
<h1>Mock OAuth2 Test App</h1>
<p>This server provides interactive pages for testing Chrome automation capabilities.</p>

<div class="info">
  <strong>OAuth2 Backend:</strong> {auth_url}<br>
  <strong>Client ID:</strong> <code>{client_id}</code><br>
  <strong>Callback:</strong> <code>{frontend_base}/callback</code>
</div>

<h2>Test Scenarios</h2>

<div class="scenario">
  <h3>1. Login Form</h3>
  <p>Test: <strong>form filling</strong>, <strong>clicking</strong>, <strong>screenshots</strong></p>
  <a href="/login" class="btn">Go to Login</a>
</div>

<div class="scenario">
  <h3>2. OAuth2 Authorization Flow</h3>
  <p>Test: <strong>navigation</strong>, <strong>redirects</strong>, <strong>cookies</strong></p>
  <a href="/start-auth" class="btn">Start OAuth2 Flow</a>
</div>

<div class="scenario">
  <h3>3. Protected Resource</h3>
  <p>Test: <strong>cookie-based access</strong>, <strong>text extraction</strong></p>
  <a href="/protected" class="btn">View Protected Page</a>
</div>

<div class="scenario">
  <h3>4. Link Collection</h3>
  <p>Test: <strong>link extraction</strong></p>
  <a href="/links" class="btn">Links Page</a>
</div>

<h2>Manual OAuth2 URLs</h2>
<p>Authorize: <code>{auth_url}/authorize?client_id={client_id}&redirect_uri={frontend_base}/callback&response_type=code&scope=openid+profile&state=test-state</code></p>
</body>
</html>"#,
        auth_url = auth_url,
        client_id = client_id,
        frontend_base = frontend_base,
    ))
}

fn login_page(error: Option<&str>) -> Html<String> {
    let error_html = error.map_or(String::new(), |e| {
        format!(
            r#"<div style="color:red; background:#fee; padding:8px; border-radius:4px; margin-bottom:12px;">{e}</div>"#
        )
    });
    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"><title>Login - Mock OAuth2</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 420px; margin: 80px auto; padding: 0 20px; color: #333; }}
h1 {{ text-align: center; color: #1a1a2e; }}
.form-group {{ margin-bottom: 16px; }}
label {{ display: block; margin-bottom: 4px; font-weight: 600; }}
input[type="text"], input[type="password"] {{ width: 100%; padding: 10px; border: 1px solid #ccc; border-radius: 6px; box-sizing: border-box; font-size: 14px; }}
.btn {{ width: 100%; padding: 12px; background: #0f3460; color: #fff; border: none; border-radius: 6px; font-size: 16px; cursor: pointer; }}
.btn:hover {{ background: #16213e; }}
.footer {{ text-align: center; margin-top: 16px; }}
.footer a {{ color: #0f3460; }}
</style>
</head>
<body>
<h1>Sign In</h1>
{error_html}
<form method="GET" action="/do-login">
  <div class="form-group">
    <label for="username">Username</label>
    <input type="text" id="username" name="username" placeholder="Enter any username" required />
  </div>
  <div class="form-group">
    <label for="password">Password</label>
    <input type="password" id="password" name="password" placeholder="Enter any password" required />
  </div>
  <button type="submit" class="btn">Sign In</button>
</form>
<div class="footer">
  <p>Hint: any username/password works</p>
  <a href="/">Back to Home</a>
</div>
</body>
</html>"#,
        error_html = error_html,
    ))
}

fn consent_page(username: &str) -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"><title>Consent - Mock OAuth2</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 480px; margin: 80px auto; padding: 0 20px; color: #333; }}
h1 {{ text-align: center; color: #1a1a2e; }}
.permissions {{ border: 1px solid #ddd; border-radius: 8px; padding: 16px; margin: 16px 0; }}
.permissions li {{ margin: 6px 0; }}
.actions {{ display: flex; gap: 12px; margin-top: 20px; }}
.btn {{ flex: 1; padding: 12px; border: none; border-radius: 6px; font-size: 16px; cursor: pointer; text-align: center; text-decoration: none; color: #fff; }}
.btn-approve {{ background: #2ecc71; }} .btn-approve:hover {{ background: #27ae60; }}
.btn-deny {{ background: #e94560; }} .btn-deny:hover {{ background: #c81d4e; }}
.user {{ text-align: center; color: #666; margin-bottom: 20px; }}
</style>
</head>
<body>
<h1>Authorization Request</h1>
<p class="user">Signed in as <strong>{username}</strong></p>
<p>The application <strong>Test Client</strong> is requesting access to your account:</p>
<ul class="permissions">
  <li>View your profile</li>
  <li>View your email address</li>
  <li>Read your basic info</li>
</ul>
<div class="actions">
  <a href="/approve" class="btn btn-approve">Approve</a>
  <a href="/deny" class="btn btn-deny">Deny</a>
</div>
</body>
</html>"#,
        username = username,
    ))
}

fn callback_page(code: &str, state: &str) -> Html<String> {
    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"><title>OAuth2 Callback - Mock OAuth2</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 560px; margin: 60px auto; padding: 0 20px; color: #333; }}
h1 {{ color: #2ecc71; }}
.success {{ background: #eafaf1; border: 1px solid #2ecc71; border-radius: 8px; padding: 16px; margin: 16px 0; }}
code {{ background: #f4f4f4; padding: 2px 6px; border-radius: 3px; word-break: break-all; }}
.detail {{ margin: 8px 0; }}
a {{ color: #0f3460; }}
</style>
</head>
<body>
<h1>Authorization Successful</h1>
<div class="success">
  <p>The OAuth2 authorization flow completed. The following data was received:</p>
  <div class="detail"><strong>Code:</strong> <code>{code}</code></div>
  <div class="detail"><strong>State:</strong> <code>{state}</code></div>
</div>
<p><a href="/protected">Access Protected Resource</a></p>
<p><a href="/">Back to Home</a></p>
</body>
</html>"#,
        code = html_escape(code),
        state = html_escape(state),
    ))
}

fn protected_page(username: Option<&str>) -> Html<String> {
    let content = match username {
        Some(u) => format!(
            r#"<div class="success">
  <p>Welcome, <strong>{u}</strong>!</p>
  <p>You are viewing a protected resource. This page is only accessible after OAuth2 authentication.</p>
  <h3>Your Profile</h3>
  <table style="width:100%; border-collapse: collapse;">
    <tr><td style="padding:8px; border:1px solid #ddd; font-weight:bold;">Name</td><td style="padding:8px; border:1px solid #ddd;">{u}</td></tr>
    <tr><td style="padding:8px; border:1px solid #ddd; font-weight:bold;">Email</td><td style="padding:8px; border:1px solid #ddd;">{u}@example.com</td></tr>
    <tr><td style="padding:8px; border:1px solid #ddd; font-weight:bold;">Role</td><td style="padding:8px; border:1px solid #ddd;">user</td></tr>
  </table>
</div>"#,
            u = html_escape(u),
        ),
        None => r#"<div class="error">
  <p>You are not authenticated.</p>
  <p>Please <a href="/login">sign in</a> first to access this protected resource.</p>
</div>"#
        .to_string(),
    };
    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"><title>Protected - Mock OAuth2</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 560px; margin: 60px auto; padding: 0 20px; color: #333; }}
h1 {{ color: #1a1a2e; }}
.success {{ background: #eafaf1; border: 1px solid #2ecc71; border-radius: 8px; padding: 16px; margin: 16px 0; }}
.error {{ background: #fee; border: 1px solid #e94560; border-radius: 8px; padding: 16px; margin: 16px 0; }}
a {{ color: #0f3460; }}
</style>
</head>
<body>
<h1>Protected Resource</h1>
{content}
<p><a href="/">Back to Home</a></p>
</body>
</html>"#,
        content = content,
    ))
}

fn links_page() -> Html<String> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"><title>Links - Mock OAuth2</title>
<style>
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 560px; margin: 60px auto; padding: 0 20px; color: #333; }
h1 { color: #1a1a2e; }
ul { list-style: none; padding: 0; }
li { margin: 8px 0; }
a { color: #0f3460; font-size: 16px; }
</style>
</head>
<body>
<h1>Links Page</h1>
<p>This page contains various links for testing link extraction.</p>
<h2>Navigation</h2>
<ul>
  <li><a href="/">Home</a></li>
  <li><a href="/login">Login Page</a></li>
  <li><a href="/protected">Protected Resource</a></li>
  <li><a href="/consent?username=testuser">Consent Page</a></li>
</ul>
<h2>External</h2>
<ul>
  <li><a href="https://example.com">Example.com</a></li>
  <li><a href="https://httpbin.org">HTTPBin</a></li>
</ul>
<h2>Hidden</h2>
<ul>
  <li><a href="/callback?code=hidden-code&state=hidden-state" style="display:none;">Hidden Link</a></li>
</ul>
</body>
</html>"#
            .into(),
    )
}

fn deny_page() -> Html<String> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"><title>Access Denied - Mock OAuth2</title>
<style>
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 480px; margin: 80px auto; padding: 0 20px; color: #333; }
h1 { color: #e94560; text-align: center; }
.error { background: #fee; border: 1px solid #e94560; border-radius: 8px; padding: 16px; margin: 16px 0; text-align: center; }
a { color: #0f3460; }
</style>
</head>
<body>
<h1>Access Denied</h1>
<div class="error">
  <p>You denied the authorization request.</p>
</div>
<p style="text-align:center;"><a href="/">Back to Home</a></p>
</body>
</html>"#
            .into(),
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Query parameter types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LoginQuery {
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
struct ConsentQuery {
    username: Option<String>,
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    auth_base_url: String,
    frontend_base_url: String,
    client_id: String,
    logged_in_user: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

async fn handle_home(state: axum::extract::State<AppState>) -> impl IntoResponse {
    home_page(&state.frontend_base_url, &state.auth_base_url, &state.client_id)
}

async fn handle_login(Query(_params): Query<LoginQuery>) -> impl IntoResponse {
    login_page(None)
}

async fn handle_do_login(
    state: axum::extract::State<AppState>,
    Query(params): Query<LoginQuery>,
) -> axum::response::Response {
    let username = params.username.unwrap_or_default();
    let password = params.password.unwrap_or_default();

    if username.is_empty() || password.is_empty() {
        return login_page(Some("Username and password are required.")).into_response();
    }

    // Any credentials are accepted
    {
        let mut user = state.logged_in_user.lock().unwrap();
        *user = Some(username.clone());
    }

    // Redirect to consent page
    let encoded = url::form_urlencoded::byte_serialize(username.as_bytes()).collect::<String>();
    Redirect::to(&format!("/consent?username={encoded}")).into_response()
}

async fn handle_consent(Query(params): Query<ConsentQuery>) -> impl IntoResponse {
    let username = params.username.as_deref().unwrap_or("anonymous");
    consent_page(username)
}

async fn handle_approve(state: axum::extract::State<AppState>) -> impl IntoResponse {
    // Build the OAuth2 authorize URL and redirect
    let auth_url = format!(
        "{}/authorize?client_id={}&redirect_uri={}/callback&response_type=code&scope=openid+profile&state=test-state",
        state.auth_base_url,
        state.client_id,
        state.frontend_base_url,
    );
    Redirect::to(&auth_url).into_response()
}

async fn handle_deny() -> impl IntoResponse {
    deny_page()
}

async fn handle_callback(Query(params): Query<CallbackQuery>) -> impl IntoResponse {
    let code = params.code.as_deref().unwrap_or("(none)");
    let state = params.state.as_deref().unwrap_or("(none)");
    callback_page(code, state)
}

async fn handle_protected(state: axum::extract::State<AppState>) -> impl IntoResponse {
    let username = state.logged_in_user.lock().unwrap().clone();
    protected_page(username.as_deref())
}

async fn handle_links() -> impl IntoResponse {
    links_page()
}

async fn handle_start_auth(state: axum::extract::State<AppState>) -> impl IntoResponse {
    let auth_url = format!(
        "{}/authorize?client_id={}&redirect_uri={}/callback&response_type=code&scope=openid+profile&state=test-state",
        state.auth_base_url,
        state.client_id,
        state.frontend_base_url,
    );
    Redirect::to(&auth_url).into_response()
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let frontend_port = cli.port;
    let backend_port = cli.port + 1;

    // 1. Start OAuth2 backend
    let oauth_config = oauth2_test_server::IssuerConfig {
        port: backend_port,
        ..Default::default()
    };
    let oauth_server = oauth2_test_server::OAuthTestServer::start_with_config(oauth_config).await;
    let auth_base_url = oauth_server.base_url().as_str().trim_end_matches('/').to_string();

    // 2. Register a test client
    let client = oauth_server
        .register_client(serde_json::json!({
            "scope": "openid profile email",
            "redirect_uris": [format!("http://127.0.0.1:{frontend_port}/callback")],
            "client_name": "test-client",
            "grant_types": ["authorization_code"],
            "response_types": ["code"]
        }))
        .await;
    let client_id = client.client_id.clone();
    let client_secret = client.client_secret.clone().unwrap_or_default();

    // 3. Start HTML frontend
    let frontend_base_url = format!("http://127.0.0.1:{frontend_port}");
    let state = AppState {
        auth_base_url: auth_base_url.clone(),
        frontend_base_url: frontend_base_url.clone(),
        client_id: client_id.clone(),
        logged_in_user: std::sync::Arc::new(std::sync::Mutex::new(None)),
    };

    let app = Router::new()
        .route("/", get(handle_home))
        .route("/login", get(handle_login))
        .route("/do-login", get(handle_do_login))
        .route("/consent", get(handle_consent))
        .route("/approve", get(handle_approve))
        .route("/deny", get(handle_deny))
        .route("/callback", get(handle_callback))
        .route("/protected", get(handle_protected))
        .route("/links", get(handle_links))
        .route("/start-auth", get(handle_start_auth))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], frontend_port));
    let listener = TcpListener::bind(addr).await?;

    // 4. Print info
    println!("Mock OAuth2 Server for Chrome Tool Testing");
    println!("============================================");
    println!("OAuth2 Backend:  {auth_base_url}");
    println!("HTML Frontend:   {frontend_base_url}");
    println!();
    println!("Client ID:     {client_id}");
    println!("Client Secret: {client_secret}");
    println!("Redirect URI:  {frontend_base_url}/callback");
    println!();
    println!("Test URLs:");
    println!("  Home:       {frontend_base_url}/");
    println!("  Login:      {frontend_base_url}/login");
    println!("  Consent:    {frontend_base_url}/consent?username=testuser");
    println!("  Protected:  {frontend_base_url}/protected");
    println!("  Links:      {frontend_base_url}/links");
    println!(
        "  Authorize:  {auth_base_url}/authorize?client_id={client_id}&redirect_uri={frontend_base_url}/callback&response_type=code&scope=openid+profile&state=test-state"
    );
    println!();
    println!("Press Ctrl+C to stop.");

    axum::serve(listener, app).await?;

    Ok(())
}
