use crate::{Request, Response};
use std::io::{self, Read, Write};

pub fn read_request() -> Result<Request, String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("Failed to read stdin: {}", e))?;
    serde_json::from_str(&input).map_err(|e| format!("Failed to parse request: {}", e))
}

pub fn write_response(response: &Response) -> Result<(), String> {
    let json = serde_json::to_vec(response).map_err(|e| format!("Failed to serialize response: {}", e))?;
    io::stdout()
        .write_all(&json)
        .map_err(|e| format!("Failed to write stdout: {}", e))?;
    io::stdout()
        .flush()
        .map_err(|e| format!("Failed to flush stdout: {}", e))?;
    Ok(())
}

/// Run an edge function handler. Reads a request from stdin,
/// passes it to your handler, writes the response to stdout.
pub fn run<F: Fn(Request) -> Response>(handler: F) {
    let request = match read_request() {
        Ok(req) => req,
        Err(e) => {
            let resp = Response::error(&format!("Failed to read request: {}", e));
            let _ = write_response(&resp);
            return;
        }
    };

    let response = handler(request);

    if let Err(e) = write_response(&response) {
        eprintln!("Failed to write response: {}", e);
    }
}
