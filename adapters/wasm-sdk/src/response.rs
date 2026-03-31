use base64::Engine;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    #[serde(serialize_with = "encode_base64")]
    pub body: Vec<u8>,
}

fn encode_base64<S: serde::Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
}

impl Response {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: vec![],
            body: vec![],
        }
    }

    pub fn ok() -> Self {
        Self::new(200)
    }

    pub fn not_found() -> Self {
        Self::new(404)
    }

    pub fn error(msg: &str) -> Self {
        Self::new(500)
            .header("Content-Type", "application/json")
            .body(serde_json::json!({"error": msg}).to_string().into_bytes())
    }

    pub fn redirect(url: &str) -> Self {
        Self::new(302).header("Location", url)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn body(mut self, data: Vec<u8>) -> Self {
        self.body = data;
        self
    }

    pub fn text(self, text: &str) -> Self {
        self.header("Content-Type", "text/plain")
            .body(text.as_bytes().to_vec())
    }

    pub fn html(self, html: &str) -> Self {
        self.header("Content-Type", "text/html")
            .body(html.as_bytes().to_vec())
    }

    pub fn json<T: Serialize>(self, value: &T) -> Self {
        let bytes = serde_json::to_vec(value).unwrap_or_default();
        self.header("Content-Type", "application/json").body(bytes)
    }
}
