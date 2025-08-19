use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::Response,
    routing::post,
    Router,
};
use clap::Parser;
use radix_trie::{Trie, TrieCommon};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::{net::TcpListener, sync::RwLock};

#[derive(Debug, PartialEq, Eq, Clone)]
enum InterceptAction {
    Block,
}

#[derive(Clone)]
struct AppState {
    intercept_rules: Arc<RwLock<Trie<String, InterceptAction>>>,
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
        intercept_rules: Arc::new(RwLock::new(Trie::new())),
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
                let mut rules = app_state.intercept_rules.write().await;
                rules.insert(route.clone(), InterceptAction::Block);
                println!("Blocking route {route}");
                Response::builder()
                    .status(StatusCode::OK)
                    .body(axum::body::Body::from(format!("Route '{}' blocked", route)))
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
            } else {
                Err(StatusCode::BAD_REQUEST)
            }
        }
        "unblock" => {
            if let Some(route) = params.get("route") {
                let mut rules = app_state.intercept_rules.write().await;
                if rules.remove(&route[..]).is_some() {
                    println!("Unblocked route {route}");
                } else {
                    println!("No-op unblock for route {route}");
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .body(axum::body::Body::from(format!(
                        "Route '{}' unblocked",
                        route
                    )))
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
    let query_str = uri.query();
    let headers = request.headers().clone();

    let client = Client::new();

    let should_block = {
        let rules = app_state.intercept_rules.read().await;
        rules.get_ancestor(path).map_or(false, |route_rules| {
            route_rules.value() == Some(&InterceptAction::Block)
        })
    };

    if should_block {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let mut url = format!("{}{}", app_state.target_url, path);

    if let Some(query) = query_str {
        url = format!("{}?{}", url, query);
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
