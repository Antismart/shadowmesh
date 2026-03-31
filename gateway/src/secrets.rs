use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use dashmap::DashMap;
use std::collections::HashMap;

struct EncryptedSecret {
    ciphertext: Vec<u8>,
    nonce: [u8; 12],
}

pub struct SecretsManager {
    secrets: DashMap<String, DashMap<String, EncryptedSecret>>,
    master_key: [u8; 32],
}

impl SecretsManager {
    pub fn new() -> Self {
        let master_key = match std::env::var("SHADOWMESH_SECRETS_KEY") {
            Ok(hex_key) => {
                let mut key = [0u8; 32];
                let hash = blake3::hash(hex_key.as_bytes());
                key.copy_from_slice(hash.as_bytes());
                tracing::info!("Secrets manager initialized with persistent key");
                key
            }
            Err(_) => {
                let mut key = [0u8; 32];
                use rand::RngCore;
                rand::thread_rng().fill_bytes(&mut key);
                tracing::warn!(
                    "SHADOWMESH_SECRETS_KEY not set — using ephemeral key (secrets won't persist across restarts)"
                );
                key
            }
        };

        Self {
            secrets: DashMap::new(),
            master_key,
        }
    }

    pub fn set_secret(&self, namespace: &str, name: &str, value: &[u8]) -> Result<(), String> {
        let cipher = ChaCha20Poly1305::new((&self.master_key).into());

        let mut nonce_bytes = [0u8; 12];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, value)
            .map_err(|e| format!("Encryption failed: {}", e))?;

        let ns = self
            .secrets
            .entry(namespace.to_string())
            .or_insert_with(DashMap::new);

        ns.insert(
            name.to_string(),
            EncryptedSecret {
                ciphertext,
                nonce: nonce_bytes,
            },
        );

        Ok(())
    }

    pub fn get_secret(&self, namespace: &str, name: &str) -> Result<Vec<u8>, String> {
        let ns = self
            .secrets
            .get(namespace)
            .ok_or_else(|| format!("Namespace '{}' not found", namespace))?;

        let entry = ns
            .get(name)
            .ok_or_else(|| format!("Secret '{}' not found", name))?;

        let cipher = ChaCha20Poly1305::new((&self.master_key).into());
        let nonce = Nonce::from_slice(&entry.nonce);

        cipher
            .decrypt(nonce, entry.ciphertext.as_ref())
            .map_err(|e| format!("Decryption failed: {}", e))
    }

    pub fn delete_secret(&self, namespace: &str, name: &str) -> bool {
        self.secrets
            .get(namespace)
            .map_or(false, |ns| ns.remove(name).is_some())
    }

    pub fn list_secrets(&self, namespace: &str) -> Vec<String> {
        self.secrets
            .get(namespace)
            .map_or(vec![], |ns| ns.iter().map(|e| e.key().clone()).collect())
    }

    pub fn get_env_map(&self, namespace: &str) -> HashMap<String, String> {
        let Some(ns) = self.secrets.get(namespace) else {
            return HashMap::new();
        };

        let cipher = ChaCha20Poly1305::new((&self.master_key).into());
        let mut map = HashMap::new();

        for entry in ns.iter() {
            let nonce = Nonce::from_slice(&entry.value().nonce);
            if let Ok(plaintext) = cipher.decrypt(nonce, entry.value().ciphertext.as_ref()) {
                if let Ok(val) = String::from_utf8(plaintext) {
                    map.insert(entry.key().clone(), val);
                }
            }
        }

        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_roundtrip() {
        let mgr = SecretsManager::new();
        mgr.set_secret("app1", "API_KEY", b"sk-12345").unwrap();
        let val = mgr.get_secret("app1", "API_KEY").unwrap();
        assert_eq!(val, b"sk-12345");
    }

    #[test]
    fn get_missing_secret() {
        let mgr = SecretsManager::new();
        assert!(mgr.get_secret("app1", "MISSING").is_err());
    }

    #[test]
    fn delete_secret() {
        let mgr = SecretsManager::new();
        mgr.set_secret("app1", "KEY", b"val").unwrap();
        assert!(mgr.delete_secret("app1", "KEY"));
        assert!(mgr.get_secret("app1", "KEY").is_err());
    }

    #[test]
    fn list_returns_names_only() {
        let mgr = SecretsManager::new();
        mgr.set_secret("app1", "SECRET_A", b"aaa").unwrap();
        mgr.set_secret("app1", "SECRET_B", b"bbb").unwrap();

        let mut names = mgr.list_secrets("app1");
        names.sort();
        assert_eq!(names, vec!["SECRET_A", "SECRET_B"]);
    }

    #[test]
    fn namespace_isolation() {
        let mgr = SecretsManager::new();
        mgr.set_secret("app1", "KEY", b"one").unwrap();
        mgr.set_secret("app2", "KEY", b"two").unwrap();

        assert_eq!(mgr.get_secret("app1", "KEY").unwrap(), b"one");
        assert_eq!(mgr.get_secret("app2", "KEY").unwrap(), b"two");
    }

    #[test]
    fn env_map() {
        let mgr = SecretsManager::new();
        mgr.set_secret("app1", "DB_URL", b"postgres://localhost").unwrap();
        mgr.set_secret("app1", "API_KEY", b"sk-test").unwrap();

        let env = mgr.get_env_map("app1");
        assert_eq!(env.get("DB_URL").unwrap(), "postgres://localhost");
        assert_eq!(env.get("API_KEY").unwrap(), "sk-test");
    }
}
