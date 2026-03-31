use axum::{
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use std::time::Duration;

const PROXY_TIMEOUT: Duration = Duration::from_secs(30);

pub fn extract_client_ip(headers: &HeaderMap, socket_addr: Option<&str>) -> Option<String> {
    if let Some(val) = headers.get("x-real-ip") {
        if let Ok(ip) = val.to_str() {
            let ip = ip.trim();
            if !ip.is_empty() {
                return Some(ip.to_string());
            }
        }
    }
    if let Some(val) = headers.get("x-forwarded-for") {
        if let Ok(forwarded) = val.to_str() {
            if let Some(first) = forwarded.split(',').next() {
                let first = first.trim();
                if !first.is_empty() {
                    return Some(first.to_string());
                }
            }
        }
    }
    socket_addr.map(|addr| {
        if addr.starts_with('[') {
            if let Some(end) = addr.find(']') {
                return addr[1..end].to_string();
            }
        }
        if let Some(pos) = addr.rfind(':') {
            if addr[pos + 1..].parse::<u16>().is_ok() {
                return addr[..pos].to_string();
            }
        }
        addr.to_string()
    })
}

pub async fn proxy_request(
    http_client: &reqwest::Client,
    port: u16,
    method: &str,
    path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: axum::body::Bytes,
    client_ip: Option<&str>,
) -> Response {
    let target_url = match query {
        Some(q) if !q.is_empty() => format!("http://127.0.0.1:{port}{path}?{q}"),
        _ => format!("http://127.0.0.1:{port}{path}"),
    };

    let reqwest_method = match reqwest::Method::from_bytes(method.as_bytes()) {
        Ok(m) => m,
        Err(_) => return bad_gateway(&format!("unsupported HTTP method: {method}")),
    };

    let mut req_builder = http_client
        .request(reqwest_method, &target_url)
        .timeout(PROXY_TIMEOUT);

    let host_value = format!("127.0.0.1:{port}");
    for (name, value) in headers.iter() {
        if name == axum::http::header::HOST
            || name == axum::http::header::CONNECTION
            || name == axum::http::header::TRANSFER_ENCODING
            || name == "keep-alive"
            || name == "upgrade"
        {
            continue;
        }
        req_builder = req_builder.header(name.clone(), value.clone());
    }

    req_builder = req_builder.header(reqwest::header::HOST, &host_value);

    if let Some(ip) = client_ip {
        req_builder = req_builder.header("X-Forwarded-For", ip);
    }
    req_builder = req_builder.header("X-Forwarded-Proto", "https");
    if let Some(original_host) = headers.get(axum::http::header::HOST) {
        req_builder = req_builder.header("X-Forwarded-Host", original_host.clone());
    }

    req_builder = req_builder.body(body);

    let upstream_resp = match req_builder.send().await {
        Ok(resp) => resp,
        Err(e) => {
            let reason = if e.is_timeout() {
                "upstream request timed out"
            } else if e.is_connect() {
                "connection refused by upstream"
            } else {
                "failed to reach upstream"
            };
            tracing::warn!(target_url = %target_url, error = %e, "reverse proxy error: {reason}");
            return bad_gateway(reason);
        }
    };

    let status =
        StatusCode::from_u16(upstream_resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    let mut response_headers = HeaderMap::new();
    for (name, value) in upstream_resp.headers().iter() {
        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(name.as_ref()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            response_headers.append(hn, hv);
        }
    }

    let body = Body::from_stream(upstream_resp.bytes_stream());

    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;

    response
}

fn bad_gateway(reason: &str) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        axum::response::Json(serde_json::json!({
            "error": "Bad Gateway",
            "message": reason,
            "status": 502,
        })),
    )
        .into_response()
}
