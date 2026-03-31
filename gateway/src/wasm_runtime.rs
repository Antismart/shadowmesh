#[cfg(feature = "wasm")]
mod inner {
    use crate::config::WasmConfig;
    use dashmap::DashMap;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use wasmtime::*;
    use wasmtime_wasi::pipe::{MemoryInputPipe, MemoryOutputPipe};
    use wasmtime_wasi::preview1::WasiP1Ctx;
    use wasmtime_wasi::{WasiCtxBuilder, WasiView};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct WasmHttpRequest {
        pub method: String,
        pub url: String,
        pub headers: Vec<(String, String)>,
        #[serde(with = "base64_bytes")]
        pub body: Vec<u8>,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct WasmHttpResponse {
        pub status: u16,
        pub headers: Vec<(String, String)>,
        #[serde(with = "base64_bytes")]
        pub body: Vec<u8>,
    }

    mod base64_bytes {
        use base64::Engine;
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
            let s = String::deserialize(d)?;
            base64::engine::general_purpose::STANDARD
                .decode(&s)
                .map_err(serde::de::Error::custom)
        }
    }

    struct WasiState {
        preview1: WasiP1Ctx,
    }

    pub struct WasmRuntime {
        engine: Engine,
        module_cache: Arc<DashMap<String, Module>>,
        config: WasmConfig,
    }

    impl WasmRuntime {
        pub fn new(config: WasmConfig) -> Result<Self, String> {
            let mut engine_config = Config::new();
            engine_config.consume_fuel(true);
            engine_config.wasm_memory64(false);

            let engine =
                Engine::new(&engine_config).map_err(|e| format!("Failed to create WASM engine: {}", e))?;

            tracing::info!("WASM runtime initialized (max_memory: {}MB, max_fuel: {})",
                config.max_memory_mb, config.max_fuel);

            Ok(Self {
                engine,
                module_cache: Arc::new(DashMap::new()),
                config,
            })
        }

        pub fn load_module(&self, key: &str, wasm_bytes: &[u8]) -> Result<(), String> {
            let max_bytes = self.config.max_module_size_mb * 1024 * 1024;
            if wasm_bytes.len() as u64 > max_bytes {
                return Err(format!(
                    "WASM module too large: {} bytes (max {}MB)",
                    wasm_bytes.len(),
                    self.config.max_module_size_mb
                ));
            }

            let module = Module::new(&self.engine, wasm_bytes)
                .map_err(|e| format!("Failed to compile WASM module: {}", e))?;

            tracing::info!(key = %key, size = wasm_bytes.len(), "WASM module loaded");
            self.module_cache.insert(key.to_string(), module);
            Ok(())
        }

        pub fn execute(
            &self,
            key: &str,
            request: WasmHttpRequest,
        ) -> Result<WasmHttpResponse, String> {
            let module = self
                .module_cache
                .get(key)
                .ok_or_else(|| format!("WASM module not found: {}", key))?;

            let request_json =
                serde_json::to_vec(&request).map_err(|e| format!("Failed to serialize request: {}", e))?;

            let stdin = MemoryInputPipe::new(bytes::Bytes::from(request_json));
            let stdout = MemoryOutputPipe::new(65536);
            let stderr = MemoryOutputPipe::new(4096);

            let stdout_clone = stdout.clone();
            let stderr_clone = stderr.clone();

            let wasi_ctx = WasiCtxBuilder::new()
                .stdin(stdin)
                .stdout(stdout)
                .stderr(stderr)
                .build_p1();

            let state = WasiState {
                preview1: wasi_ctx,
            };

            let mut store = Store::new(&self.engine, state);
            store
                .set_fuel(self.config.max_fuel)
                .map_err(|e| format!("Failed to set fuel: {}", e))?;

            let mut linker = Linker::new(&self.engine);
            wasmtime_wasi::preview1::add_to_linker_sync(&mut linker, |s: &mut WasiState| &mut s.preview1)
                .map_err(|e| format!("Failed to add WASI to linker: {}", e))?;

            let instance = linker
                .instantiate(&mut store, &module)
                .map_err(|e| format!("Failed to instantiate module: {}", e))?;

            let start = instance
                .get_typed_func::<(), ()>(&mut store, "_start")
                .map_err(|e| format!("Module missing _start export: {}", e))?;

            match start.call(&mut store, ()) {
                Ok(()) => {}
                Err(e) => {
                    // Check fuel exhaustion first
                    let fuel_left = store.get_fuel().unwrap_or(0);
                    if fuel_left == 0 {
                        return Err("WASM execution exceeded fuel limit (CPU timeout)".into());
                    }
                    // P6: Check for WASI process exit (code 0 is normal)
                    let is_normal_exit = e.downcast_ref::<wasmtime_wasi::I32Exit>()
                        .map_or(false, |exit| exit.0 == 0);
                    if !is_normal_exit {
                        return Err(format!("WASM execution failed: {}", e));
                    }
                }
            }

            let output_bytes = stdout_clone.try_into_inner().unwrap_or_default();

            if output_bytes.is_empty() {
                return Err("WASM module produced no output".into());
            }

            let response: WasmHttpResponse = serde_json::from_slice(&output_bytes)
                .map_err(|e| {
                    let stderr_bytes = stderr_clone.try_into_inner().unwrap_or_default();
                    let stderr_str = String::from_utf8_lossy(&stderr_bytes);
                    format!(
                        "Failed to parse WASM response: {}. Stderr: {}",
                        e,
                        stderr_str.chars().take(500).collect::<String>()
                    )
                })?;

            Ok(response)
        }

        pub fn has_module(&self, key: &str) -> bool {
            self.module_cache.contains_key(key)
        }

        pub fn remove_module(&self, key: &str) {
            self.module_cache.remove(key);
        }

        pub fn module_count(&self) -> usize {
            self.module_cache.len()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn request_response_roundtrip() {
            let req = WasmHttpRequest {
                method: "GET".into(),
                url: "/api/hello".into(),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: b"hello".to_vec(),
            };
            let json = serde_json::to_string(&req).unwrap();
            let parsed: WasmHttpRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.method, "GET");
            assert_eq!(parsed.url, "/api/hello");
            assert_eq!(parsed.body, b"hello");
        }

        #[test]
        fn response_default() {
            let resp = WasmHttpResponse::default();
            assert_eq!(resp.status, 0);
            assert!(resp.headers.is_empty());
            assert!(resp.body.is_empty());
        }

        #[test]
        fn runtime_creation() {
            let config = WasmConfig::default();
            let rt = WasmRuntime::new(config);
            assert!(rt.is_ok());
        }

        #[test]
        fn module_not_found() {
            let rt = WasmRuntime::new(WasmConfig::default()).unwrap();
            let req = WasmHttpRequest {
                method: "GET".into(),
                url: "/".into(),
                headers: vec![],
                body: vec![],
            };
            let result = rt.execute("nonexistent", req);
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("not found"));
        }

        #[test]
        fn oversized_module_rejected() {
            let mut config = WasmConfig::default();
            config.max_module_size_mb = 0; // reject everything
            let rt = WasmRuntime::new(config).unwrap();
            let result = rt.load_module("test", &[0u8; 100]);
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("too large"));
        }

        #[test]
        fn has_module_empty() {
            let rt = WasmRuntime::new(WasmConfig::default()).unwrap();
            assert!(!rt.has_module("test"));
            assert_eq!(rt.module_count(), 0);
        }
    }
}

#[cfg(feature = "wasm")]
pub use inner::*;

// Stub types when wasm feature is disabled
#[cfg(not(feature = "wasm"))]
pub struct WasmRuntime;

#[cfg(not(feature = "wasm"))]
impl WasmRuntime {
    pub fn new(_config: crate::config::WasmConfig) -> Result<Self, String> {
        Err("WASM support not compiled (enable 'wasm' feature)".into())
    }
}

#[cfg(not(feature = "wasm"))]
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WasmHttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[cfg(not(feature = "wasm"))]
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WasmHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
