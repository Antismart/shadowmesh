//! Performance benchmarks for ShadowMesh
//!
//! Compares WebRTC vs HTTP gateway performance and measures core operations.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Duration;

// Simulated content sizes for benchmarking
const SIZES: &[usize] = &[
    1024,        // 1 KB
    10 * 1024,   // 10 KB
    100 * 1024,  // 100 KB
    1024 * 1024, // 1 MB
];

// ============================================
// Content Hashing Benchmarks
// ============================================

fn bench_content_hashing(c: &mut Criterion) {
    let mut group = c.benchmark_group("content_hashing");
    group.measurement_time(Duration::from_secs(5));

    for size in SIZES {
        let data = vec![0u8; *size];

        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::new("blake3", size), &data, |b, data| {
            b.iter(|| {
                let hash = blake3::hash(black_box(data));
                black_box(hash)
            })
        });
    }

    group.finish();
}

// ============================================
// Content Fragmentation Benchmarks
// ============================================

fn bench_fragmentation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragmentation");
    group.measurement_time(Duration::from_secs(5));

    const CHUNK_SIZE: usize = 256 * 1024; // 256 KB chunks

    for size in SIZES {
        let data = vec![0u8; *size];

        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::new("fragment", size), &data, |b, data| {
            b.iter(|| {
                let chunks: Vec<&[u8]> = data.chunks(CHUNK_SIZE).collect();
                black_box(chunks)
            })
        });

        group.bench_with_input(
            BenchmarkId::new("fragment_with_hash", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let chunks: Vec<([u8; 32], &[u8])> = data
                        .chunks(CHUNK_SIZE)
                        .map(|chunk| {
                            let hash = blake3::hash(chunk);
                            (*hash.as_bytes(), chunk)
                        })
                        .collect();
                    black_box(chunks)
                })
            },
        );
    }

    group.finish();
}

// ============================================
// Encryption Benchmarks
// ============================================

fn bench_encryption(c: &mut Criterion) {
    use chacha20poly1305::{
        aead::{Aead, KeyInit},
        ChaCha20Poly1305, Nonce,
    };

    let mut group = c.benchmark_group("encryption");
    group.measurement_time(Duration::from_secs(5));

    let key = [0u8; 32];
    let nonce_bytes = [0u8; 12];

    for size in SIZES {
        let data = vec![0u8; *size];
        let cipher = ChaCha20Poly1305::new(&key.into());
        let nonce = Nonce::from_slice(&nonce_bytes);

        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(
            BenchmarkId::new("chacha20_encrypt", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let encrypted = cipher.encrypt(nonce, black_box(data.as_slice())).unwrap();
                    black_box(encrypted)
                })
            },
        );

        // Prepare encrypted data for decryption benchmark
        let encrypted = cipher.encrypt(nonce, data.as_slice()).unwrap();

        group.bench_with_input(
            BenchmarkId::new("chacha20_decrypt", size),
            &encrypted,
            |b, encrypted| {
                b.iter(|| {
                    let decrypted = cipher
                        .decrypt(nonce, black_box(encrypted.as_slice()))
                        .unwrap();
                    black_box(decrypted)
                })
            },
        );
    }

    group.finish();
}

// ============================================
// Serialization Benchmarks
// ============================================

fn bench_serialization(c: &mut Criterion) {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct SignalingMessage {
        msg_type: String,
        peer_id: String,
        session_id: String,
        payload: String,
    }

    let mut group = c.benchmark_group("serialization");

    let message = SignalingMessage {
        msg_type: "offer".to_string(),
        peer_id: "peer-abc123def456".to_string(),
        session_id: "session-789xyz".to_string(),
        payload: "v=0\r\no=- 123456 2 IN IP4 127.0.0.1\r\n...".to_string(),
    };

    group.bench_function("json_serialize", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&message)).unwrap();
            black_box(json)
        })
    });

    let json_str = serde_json::to_string(&message).unwrap();

    group.bench_function("json_deserialize", |b| {
        b.iter(|| {
            let msg: SignalingMessage = serde_json::from_str(black_box(&json_str)).unwrap();
            black_box(msg)
        })
    });

    group.finish();
}

// ============================================
// HTTP vs WebRTC Latency Simulation
// ============================================

fn bench_transport_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("transport_overhead");

    // Simulate HTTP request overhead (headers, TLS, etc.)
    fn simulate_http_overhead(payload_size: usize) -> usize {
        const HTTP_HEADER_SIZE: usize = 500; // Typical HTTP headers
        const TLS_OVERHEAD: usize = 50; // TLS record overhead
        payload_size + HTTP_HEADER_SIZE + TLS_OVERHEAD
    }

    // Simulate WebRTC DataChannel overhead
    fn simulate_webrtc_overhead(payload_size: usize) -> usize {
        const SCTP_HEADER: usize = 28; // SCTP header
        const DTLS_OVERHEAD: usize = 29; // DTLS record overhead
        payload_size + SCTP_HEADER + DTLS_OVERHEAD
    }

    for size in SIZES {
        let data = vec![0u8; *size];

        group.bench_with_input(BenchmarkId::new("http_overhead", size), &data, |b, data| {
            b.iter(|| {
                let total = simulate_http_overhead(black_box(data.len()));
                black_box(total)
            })
        });

        group.bench_with_input(
            BenchmarkId::new("webrtc_overhead", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let total = simulate_webrtc_overhead(black_box(data.len()));
                    black_box(total)
                })
            },
        );
    }

    group.finish();
}

// ============================================
// Concurrent Connection Handling
// ============================================

fn bench_connection_tracking(c: &mut Criterion) {
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("connection_tracking");

    group.bench_function("hashmap_insert", |b| {
        b.iter(|| {
            let mut map: HashMap<String, u64> = HashMap::new();
            for i in 0..1000 {
                map.insert(format!("peer-{}", i), i);
            }
            black_box(map)
        })
    });

    group.bench_function("hashmap_lookup", |b| {
        let mut map: HashMap<String, u64> = HashMap::new();
        for i in 0..1000 {
            map.insert(format!("peer-{}", i), i);
        }

        b.iter(|| {
            for i in 0..1000 {
                let _ = map.get(&format!("peer-{}", i));
            }
        })
    });

    group.bench_function("rwlock_read", |b| {
        let map: Arc<RwLock<HashMap<String, u64>>> = Arc::new(RwLock::new(HashMap::new()));

        rt.block_on(async {
            let mut guard = map.write().await;
            for i in 0..1000 {
                guard.insert(format!("peer-{}", i), i);
            }
        });

        b.iter(|| {
            rt.block_on(async {
                let guard = map.read().await;
                for i in 0..100 {
                    let _ = guard.get(&format!("peer-{}", i));
                }
            })
        })
    });

    group.finish();
}

// ============================================
// Session ID Generation
// ============================================

fn bench_session_id_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("session_id");

    group.bench_function("uuid_v4", |b| {
        b.iter(|| {
            let id = uuid::Uuid::new_v4().to_string();
            black_box(id)
        })
    });

    group.bench_function("blake3_random", |b| {
        b.iter(|| {
            let random_bytes: [u8; 16] = rand::random();
            let hash = blake3::hash(&random_bytes);
            let id = hex::encode(&hash.as_bytes()[..16]);
            black_box(id)
        })
    });

    group.finish();
}

// ============================================
// Main Benchmark Groups
// ============================================

criterion_group!(
    benches,
    bench_content_hashing,
    bench_fragmentation,
    bench_encryption,
    bench_serialization,
    bench_transport_overhead,
    bench_connection_tracking,
    bench_session_id_generation,
);

criterion_main!(benches);
