use axum::{
    Router,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::Response,
    routing::post,
};
use clap::Parser;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;

#[derive(Clone)]
enum InterceptAction {
    Block,
}

#[derive(Clone)]
struct AppState {
    intercept_rules: Arc<RwLock<HashMap<String, InterceptAction>>>,
    target_url: String,
}

#[derive(Parser)]
#[command(name = "kaiyote")]
#[command(about = "HTTP proxy middleware with route interception capabilities")]
struct Args {
    #[arg(short, long, default_value = "http://127.0.0.1:8080")]
    #[arg(help = "Target URL to proxy requests to")]
    target: String,

    #[arg(short, long, default_value = "127.0.0.1:3000")]
    #[arg(help = "Address to bind the proxy server to")]
    bind: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let app_state = AppState {
        intercept_rules: Arc::new(RwLock::new(HashMap::new())),
        target_url: args.target.clone(),
    };

    let app = Router::new()
        .route("/control/{command}", post(control_handler))
        .fallback(proxy_handler)
        .with_state(app_state);

    let listener = TcpListener::bind(&args.bind).await.unwrap();
    println!("Proxy server running on http://{}", args.bind);
    println!("Proxying requests to: {}", args.target);

    axum::serve(listener, app).await.unwrap();
}

async fn control_handler(
    Path(command): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(app_state): State<AppState>,
) -> Result<Response, StatusCode> {
    match command.as_str() {
        "block" => {
            if let Some(route) = params.get("route") {
                let mut rules = app_state.intercept_rules.write().unwrap();
                rules.insert(route.clone(), InterceptAction::Block);
                Response::builder()
                    .status(StatusCode::OK)
                    .body(axum::body::Body::from(format!("Route '{}' blocked", route)))
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
            } else {
                Err(StatusCode::BAD_REQUEST)
            }
        }
        _ => Err(StatusCode::NOT_FOUND),
    }
}

#[axum::debug_handler]
async fn proxy_handler(
    State(app_state): State<AppState>,
    request: Request,
) -> Result<Response, StatusCode> {
    let method = request.method().clone();
    let uri = request.uri();
    let path = uri.path();
    let query_str = uri.query().unwrap_or("");
    let headers = request.headers().clone();

    let query: HashMap<String, String> = if query_str.is_empty() {
        HashMap::new()
    } else {
        query_str
            .split('&')
            .filter_map(|pair| {
                let mut split = pair.split('=');
                match (split.next(), split.next()) {
                    (Some(key), Some(value)) => Some((key.to_string(), value.to_string())),
                    _ => None,
                }
            })
            .collect()
    };

    let client = Client::new();

    let should_block = {
        let rules = app_state.intercept_rules.read().unwrap();
        rules.get(path).is_some()
    };

    if should_block {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let mut url = format!("{}{}", app_state.target_url, path);

    if !query.is_empty() {
        let query_string: Vec<String> = query.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        url = format!("{}?{}", url, query_string.join("&"));
    }

    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut req_builder = client.request(method, &url);

    for (name, value) in headers.iter() {
        if name != "host" && name != "content-length" {
            req_builder = req_builder.header(name, value);
        }
    }

    if !body.is_empty() {
        req_builder = req_builder.body(body);
    }

    let response = req_builder
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut resp_builder = Response::builder().status(status);

    for (name, value) in headers.iter() {
        if name != "content-length" && name != "transfer-encoding" {
            resp_builder = resp_builder.header(name, value);
        }
    }

    resp_builder
        .body(axum::body::Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
