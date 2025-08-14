use axum::{
    extract::{Path, Query, Request},
    http::{HeaderMap, Method, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use reqwest::Client;
use std::collections::HashMap;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/{*path}", get(proxy_handler))
        .route("/{*path}", post(proxy_handler))
        .route("/", get(proxy_handler))
        .route("/", post(proxy_handler));

    let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("Proxy server running on http://127.0.0.1:3000");
    println!("Set TARGET_URL environment variable to specify the target server");
    
    axum::serve(listener, app).await.unwrap();
}

async fn proxy_handler(
    method: Method,
    opt_path: Option<Path<String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    request: Request,
) -> Result<Response, StatusCode> {
    let target_url = std::env::var("TARGET_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());

    let client = Client::new();
    
    let full_path = if let Some(Path(path)) = opt_path {
        format!("/{path}")
    } else {
        "/".to_string()
    };
    let mut url = format!("{}{}", target_url, full_path);
    
    if !query.is_empty() {
        let query_string: Vec<String> = query
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
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
    let body = response.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

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
