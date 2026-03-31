use shadowmesh_edge::{Request, Response};

fn main() {
    shadowmesh_edge::run(handle);
}

fn handle(req: Request) -> Response {
    match req.path() {
        "/api/hello" => Response::ok().json(&serde_json::json!({
            "message": "Hello from the edge!",
            "method": req.method,
            "path": req.url,
        })),
        "/api/echo" => {
            let body = req.body_string().unwrap_or_default();
            Response::ok()
                .header("Content-Type", "text/plain")
                .body(body.into_bytes())
        }
        _ => Response::not_found().json(&serde_json::json!({
            "error": "Not found",
            "path": req.path(),
        })),
    }
}
