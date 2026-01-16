# Adaptive Routing & Censorship Detection

ShadowMesh's adaptive routing system automatically detects censorship and routes around blocked paths in real-time.

## Overview

The adaptive routing module provides:

- **Path Health Monitoring** - Track success/failure rates for every network segment
- **Censorship Detection** - Identify blocking patterns (RST, DNS poisoning, TLS manipulation)
- **Automatic Failover** - Switch to backup routes instantly when paths fail
- **Geographic Diversity** - Ensure routes span multiple jurisdictions
- **Latency Optimization** - Balance privacy with performance

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Adaptive Router                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │   Relay     │  │    Path     │  │  Censorship │             │
│  │  Registry   │  │   Health    │  │  Detector   │             │
│  │             │  │   Tracker   │  │             │             │
│  │ • Geo info  │  │ • Success/  │  │ • Timeout   │             │
│  │ • Latency   │  │   Failure   │  │ • RST       │             │
│  │ • Capacity  │  │ • Latency   │  │ • DNS       │             │
│  │ • ASN       │  │ • Bytes     │  │ • TLS       │             │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
│         │                │                │                     │
│         ▼                ▼                ▼                     │
│  ┌───────────────────────────────────────────────────────┐     │
│  │                   Route Selector                       │     │
│  │                                                         │     │
│  │  Strategy: LowLatency | HighPrivacy | Balanced         │     │
│  │  Constraints: Geo diversity, ASN diversity, Min hops    │     │
│  └───────────────────────────────────────────────────────┘     │
│         │                                                       │
│         ▼                                                       │
│  ┌───────────────────────────────────────────────────────┐     │
│  │              Active Routes + Backups                    │     │
│  │                                                         │     │
│  │  Primary: US → DE → JP → Content                       │     │
│  │  Backup1: CA → FR → AU → Content                       │     │
│  │  Backup2: GB → NL → SG → Content                       │     │
│  └───────────────────────────────────────────────────────┘     │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Censorship Detection

### Detection Signals

| Signal | Severity | Description |
|--------|----------|-------------|
| TCP RST | High (1.0) | Connection reset packets |
| Content Tampering | High (1.0) | Hash mismatch in response |
| TLS Failure | High (0.8) | Certificate injection/MITM |
| DNS Failure | High (0.8) | DNS poisoning/blocking |
| Timeout | Medium (0.6) | Connection timeout |
| Connection Refused | Low (0.4) | Usually server-side |
| HTTP Error | Low (0.3) | Rarely censorship |

### Status Progression

```
Unknown → Suspected → Confirmed
   │          │           │
   │          │           └── > 70% censorship failures
   │          └────────────── > 40% censorship failures
   └───────────────────────── < 5 samples
```

### Example Detection

```rust
use shadowmesh_protocol::{AdaptiveRouter, FailureType, CensorshipStatus};

let router = AdaptiveRouter::new();

// After multiple connection resets from same path
router.record_failure(&route_id, 0, FailureType::ConnectionReset);
router.record_failure(&route_id, 0, FailureType::ConnectionReset);
router.record_failure(&route_id, 0, FailureType::ConnectionReset);

// Check path status
if let Some((health, status)) = router.get_path_health(from, to) {
    match status {
        CensorshipStatus::Confirmed => {
            println!("Path is blocked, using backup route");
        }
        CensorshipStatus::Suspected => {
            println!("Path may be blocked, monitoring closely");
        }
        _ => {}
    }
}
```

## Routing Strategies

### Available Strategies

| Strategy | Hops | Use Case |
|----------|------|----------|
| `LowLatency` | min_hops | Gaming, real-time apps |
| `HighPrivacy` | max_hops | Maximum anonymity |
| `HighReliability` | Adaptive | Critical content |
| `Balanced` | Average | Default, most users |
| `AvoidRegions` | Variable | Censorship circumvention |

### Configuration

```rust
use shadowmesh_protocol::{AdaptiveRouter, AdaptiveRoutingConfig, RouteStrategy, GeoRegion};
use std::collections::HashSet;
use std::time::Duration;

let mut avoid_regions = HashSet::new();
avoid_regions.insert(GeoRegion::Asia);  // Avoid CN/JP/KR/etc

let config = AdaptiveRoutingConfig {
    strategy: RouteStrategy::HighPrivacy,
    min_hops: 3,
    max_hops: 5,
    require_geo_diversity: true,
    avoid_regions,
    avoid_asns: HashSet::new(),
    failover_timeout: Duration::from_secs(10),
    max_retries: 3,
    auto_failover: true,
    health_check_interval: Duration::from_secs(60),
};

let router = AdaptiveRouter::with_config(config);
```

## Geographic Diversity

Routes can be configured to span multiple jurisdictions:

```
┌─────────┐     ┌─────────┐     ┌─────────┐     ┌─────────┐
│  North  │────▶│  Europe │────▶│  Asia   │────▶│ Oceania │
│ America │     │         │     │         │     │         │
│  (US)   │     │  (DE)   │     │  (SG)   │     │  (AU)   │
└─────────┘     └─────────┘     └─────────┘     └─────────┘
```

### Supported Regions

- **NorthAmerica** - US, CA, MX
- **SouthAmerica** - BR, AR, CL, CO, PE, VE
- **Europe** - GB, DE, FR, NL, SE, NO, FI, CH, AT, BE, IT, ES, PT, PL, CZ, RO, UA, IE
- **Africa** - ZA, NG, KE, EG, MA, GH
- **MiddleEast** - AE, SA, IL, TR, IR, IQ
- **Asia** - CN, JP, KR, IN, SG, HK, TW, TH, VN, ID, MY, PH, PK, BD
- **Oceania** - AU, NZ

## API Reference

### AdaptiveRouter

```rust
use shadowmesh_protocol::{AdaptiveRouter, RelayInfo, ComputedRoute};

// Create router
let router = AdaptiveRouter::new();

// Register relays
let relay = RelayInfo::with_geo(peer_id, "US", Some(ip_addr));
router.register_relay(relay);

// Compute a route
let route: ComputedRoute = router.compute_route(None)?;
println!("Route: {:?}", route.relays);
println!("Health: {}", route.health_score);
println!("Latency: {}ms", route.estimated_latency_ms);

// Record success
router.record_success(&route.id, latency_ms, bytes);

// Record failure (may trigger failover)
if let Some(backup) = router.record_failure(&route.id, hop_index, FailureType::ConnectionReset) {
    println!("Switched to backup route");
}

// Manual failover
if let Some(backup) = router.failover(&route.id) {
    println!("Failover successful");
}

// Check statistics
let stats = router.stats();
println!("Routes computed: {}", stats.routes_computed);
println!("Censorship events: {}", stats.censorship_events);
```

### RelayInfo

```rust
use shadowmesh_protocol::{RelayInfo, GeoRegion};
use std::net::IpAddr;

let mut relay = RelayInfo::new(peer_id);
relay.country_code = Some("US".to_string());
relay.region = GeoRegion::NorthAmerica;
relay.ip_addr = Some("1.2.3.4".parse().unwrap());
relay.asn = Some(13335);  // Cloudflare
relay.avg_latency_ms = 50;
relay.bandwidth_bps = 100_000_000;  // 100 Mbps
relay.load = 0.3;  // 30% utilized
relay.is_guard = true;   // Can be entry node
relay.is_exit = true;    // Can be exit node
relay.reputation = 0.9;  // High reputation

// Check health
if relay.is_healthy() {
    router.register_relay(relay);
}

// Get fitness score
let score = relay.fitness_score(prefer_low_latency);
```

### PathHealth

```rust
use shadowmesh_protocol::{PathHealth, FailureType, CensorshipStatus};

let mut health = PathHealth::new(from_peer, to_peer);

// Record attempts
health.record_success(latency_ms, bytes);
health.record_failure(FailureType::Timeout);

// Check metrics
let failure_rate = health.failure_rate();
let should_avoid = health.should_avoid();
let status = health.censorship_status;

match status {
    CensorshipStatus::Clear => println!("Path is healthy"),
    CensorshipStatus::Suspected => println!("Monitoring path closely"),
    CensorshipStatus::Confirmed => println!("Path is blocked"),
    CensorshipStatus::Unknown => println!("Insufficient data"),
}
```

## Automatic Failover

When a path fails, the router automatically:

1. Records the failure type
2. Updates path health metrics
3. Checks if censorship threshold is exceeded
4. Blocks confirmed-censored paths
5. Switches to pre-computed backup route
6. If no backup, computes new route avoiding blocked paths

```rust
// Automatic failover is enabled by default
let config = AdaptiveRoutingConfig {
    auto_failover: true,  // Enable automatic failover
    failover_timeout: Duration::from_secs(10),
    max_retries: 3,
    ..Default::default()
};
```

## Failure Types

```rust
pub enum FailureType {
    Timeout,           // Connection timeout
    ConnectionRefused, // Port closed
    ConnectionReset,   // TCP RST (strong censorship indicator)
    DnsFailure,        // DNS poisoning
    TlsFailure,        // Certificate issues
    HttpError(u16),    // HTTP 4xx/5xx
    ContentTampering,  // Hash mismatch
    NetworkError,      // Generic
    PeerUnreachable,   // Peer offline
}
```

## Statistics

```rust
let stats = router.stats();

println!("Routes computed: {}", stats.routes_computed);
println!("Successful deliveries: {}", stats.successful_deliveries);
println!("Failed deliveries: {}", stats.failed_deliveries);
println!("Failovers triggered: {}", stats.failovers_triggered);
println!("Censorship events: {}", stats.censorship_events);
println!("Blocked paths: {}", stats.blocked_paths);
println!("Avg route time: {}ms", stats.avg_route_time_ms);
```

## Integration with ZK Relay

The adaptive router works with the ZK relay for complete privacy:

```rust
use shadowmesh_protocol::{ZkRelayClient, AdaptiveRouter, KEY_SIZE};

// 1. Compute optimal route
let router = AdaptiveRouter::new();
let route = router.compute_route(None)?;

// 2. Build ZK circuit through the route
let mut client = ZkRelayClient::new([0u8; KEY_SIZE]);
let circuit_id = client.build_circuit(&route.relays)?;

// 3. Use circuit for requests
let cell = client.wrap_request(&circuit_id, request)?;

// 4. If failure, router triggers failover
if delivery_failed {
    if let Some(backup) = router.record_failure(&route.id, hop, failure_type) {
        // Build new circuit through backup route
        let new_circuit = client.build_circuit(&backup.relays)?;
    }
}
```

## Testing

```bash
# Run adaptive routing tests
cargo test -p protocol adaptive_routing

# Run integration tests
cargo test -p protocol --test integration_tests
```

## Future Enhancements

- [ ] Machine learning-based censorship prediction
- [ ] Real-time path probing
- [ ] BGP anomaly detection
- [ ] Steganographic fallback channels
- [ ] Tor bridge integration
- [ ] Domain fronting support
