use base64::Engine;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    #[serde(deserialize_with = "decode_base64")]
    pub body: Vec<u8>,
}

fn decode_base64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    let s = String::deserialize(d)?;
    base64::engine::general_purpose::STANDARD
        .decode(&s)
        .map_err(serde::de::Error::custom)
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        let lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn body_string(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.body.clone())
    }

    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    pub fn path(&self) -> &str {
        self.url.split('?').next().unwrap_or(&self.url)
    }

    pub fn query(&self) -> Option<&str> {
        self.url.split_once('?').map(|(_, q)| q)
    }
}
