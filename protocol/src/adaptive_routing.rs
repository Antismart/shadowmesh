//! Adaptive Routing with Censorship Detection
//!
//! Implements intelligent routing that detects censorship and automatically
//! routes around blocked paths in real-time.
//!
//! # Key Features
//!
//! - **Path Health Monitoring**: Track success/failure rates per route
//! - **Censorship Detection**: Identify patterns indicating blocking
//! - **Automatic Failover**: Switch to alternative paths instantly
//! - **Geographic Diversity**: Ensure routes span multiple jurisdictions
//! - **Latency Optimization**: Balance privacy with performance
//!
//! # Detection Signals
//!
//! - Connection timeouts from specific regions
//! - TCP RST packets (connection resets)
//! - DNS poisoning detection
//! - TLS certificate anomalies
//! - Consistent failures from specific ASNs

use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum history entries per path
const MAX_PATH_HISTORY: usize = 100;

/// Threshold for marking a path as potentially censored
const CENSORSHIP_FAILURE_THRESHOLD: f64 = 0.7;

/// Minimum samples before making censorship determination
const MIN_SAMPLES_FOR_DETECTION: usize = 5;

/// Time window for failure rate calculation
const FAILURE_WINDOW: Duration = Duration::from_secs(300); // 5 minutes

/// Censorship detection result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CensorshipStatus {
    /// No censorship detected
    Clear,
    /// Possible censorship (elevated failures)
    Suspected,
    /// Confirmed censorship pattern
    Confirmed,
    /// Unknown (insufficient data)
    Unknown,
}

/// Type of failure observed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureType {
    /// Connection timeout
    Timeout,
    /// Connection refused
    ConnectionRefused,
    /// Connection reset (TCP RST)
    ConnectionReset,
    /// DNS resolution failed
    DnsFailure,
    /// TLS/SSL handshake failed
    TlsFailure,
    /// HTTP error (4xx, 5xx)
    HttpError(u16),
    /// Content hash mismatch (tampering)
    ContentTampering,
    /// Generic network error
    NetworkError,
    /// Peer not reachable
    PeerUnreachable,
}

impl FailureType {
    /// Check if this failure type is indicative of censorship
    pub fn is_censorship_indicator(&self) -> bool {
        matches!(
            self,
            FailureType::ConnectionReset
                | FailureType::Timeout
                | FailureType::DnsFailure
                | FailureType::TlsFailure
                | FailureType::ContentTampering
        )
    }

    /// Get severity weight for this failure type
    pub fn severity_weight(&self) -> f64 {
        match self {
            FailureType::ConnectionReset => 1.0,    // Strong indicator
            FailureType::ContentTampering => 1.0,   // Strong indicator
            FailureType::TlsFailure => 0.8,         // Often censorship
            FailureType::DnsFailure => 0.8,         // Often censorship
            FailureType::Timeout => 0.6,            // Could be network
            FailureType::ConnectionRefused => 0.4,  // Usually not censorship
            FailureType::HttpError(_) => 0.3,       // Rarely censorship
            FailureType::NetworkError => 0.2,       // Generic
            FailureType::PeerUnreachable => 0.5,    // Could be either
        }
    }
}

/// A single path attempt record
#[derive(Debug, Clone)]
pub struct PathAttempt {
    /// When the attempt was made
    pub timestamp: Instant,
    /// Whether it succeeded
    pub success: bool,
    /// Failure type if failed
    pub failure_type: Option<FailureType>,
    /// Latency in milliseconds
    pub latency_ms: Option<u64>,
    /// Bytes transferred
    pub bytes_transferred: u64,
}

/// Geographic region for diversity routing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeoRegion {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Africa,
    MiddleEast,
    Asia,
    Oceania,
    Unknown,
}

impl GeoRegion {
    /// Get region from ISO 3166-1 alpha-2 country code
    /// Comprehensive mapping covering all recognized countries
    pub fn from_country_code(code: &str) -> Self {
        match code.to_uppercase().as_str() {
            // North America (23 countries)
            "US" | "CA" | "MX" | "GT" | "CU" | "HT" | "DO" | "HN" | "NI" | "SV" 
            | "CR" | "PA" | "JM" | "TT" | "BS" | "BB" | "LC" | "GD" | "VC" | "AG" 
            | "DM" | "KN" | "BZ" => GeoRegion::NorthAmerica,
            
            // South America (12 countries)
            "BR" | "AR" | "CL" | "CO" | "PE" | "VE" | "EC" | "BO" | "PY" | "UY" 
            | "GY" | "SR" => GeoRegion::SouthAmerica,
            
            // Europe (44 countries)
            "GB" | "DE" | "FR" | "NL" | "SE" | "NO" | "FI" | "CH" | "AT" | "BE" 
            | "IT" | "ES" | "PT" | "PL" | "CZ" | "RO" | "UA" | "IE" | "DK" | "HU"
            | "SK" | "HR" | "SI" | "RS" | "BG" | "GR" | "LT" | "LV" | "EE" | "IS"
            | "LU" | "MT" | "CY" | "AL" | "MK" | "BA" | "ME" | "MD" | "BY" | "XK"
            | "AD" | "MC" | "SM" | "LI" => GeoRegion::Europe,
            
            // Africa (54 countries)
            "ZA" | "NG" | "KE" | "EG" | "MA" | "GH" | "ET" | "TZ" | "UG" | "DZ"
            | "SD" | "AO" | "MZ" | "MG" | "CM" | "CI" | "NE" | "BF" | "ML" | "MW"
            | "ZM" | "SN" | "TD" | "SO" | "ZW" | "GN" | "RW" | "BJ" | "TN" | "BI"
            | "SS" | "TG" | "SL" | "LY" | "CG" | "LR" | "CF" | "MR" | "ER" | "NA"
            | "GM" | "BW" | "GA" | "LS" | "GW" | "GQ" | "MU" | "SZ" | "DJ" | "KM"
            | "CV" | "ST" | "SC" | "CD" => GeoRegion::Africa,
            
            // Middle East (17 countries)
            "AE" | "SA" | "IL" | "TR" | "IR" | "IQ" | "JO" | "LB" | "SY" | "YE"
            | "OM" | "KW" | "QA" | "BH" | "PS" | "AM" | "GE" => GeoRegion::MiddleEast,
            
            // Asia (48 countries)
            "CN" | "JP" | "KR" | "IN" | "SG" | "HK" | "TW" | "TH" | "VN" | "ID" 
            | "MY" | "PH" | "PK" | "BD" | "LK" | "NP" | "MM" | "KH" | "LA" | "MN"
            | "KZ" | "UZ" | "TM" | "TJ" | "KG" | "AZ" | "AF" | "BT" | "MV" | "BN"
            | "TL" | "MO" | "KP" => GeoRegion::Asia,
            
            // Oceania (14 countries)
            "AU" | "NZ" | "PG" | "FJ" | "SB" | "VU" | "WS" | "KI" | "TO" | "FM"
            | "PW" | "MH" | "NR" | "TV" => GeoRegion::Oceania,
            
            // Unknown
            _ => GeoRegion::Unknown,
        }
    }
    
    /// Get approximate latitude range for the region (for distance estimation)
    pub fn latitude_range(&self) -> (f64, f64) {
        match self {
            GeoRegion::NorthAmerica => (15.0, 72.0),
            GeoRegion::SouthAmerica => (-56.0, 12.0),
            GeoRegion::Europe => (35.0, 71.0),
            GeoRegion::Africa => (-35.0, 37.0),
            GeoRegion::MiddleEast => (12.0, 42.0),
            GeoRegion::Asia => (-10.0, 77.0),
            GeoRegion::Oceania => (-47.0, -1.0),
            GeoRegion::Unknown => (-90.0, 90.0),
        }
    }
    
    /// Get approximate longitude range for the region
    pub fn longitude_range(&self) -> (f64, f64) {
        match self {
            GeoRegion::NorthAmerica => (-168.0, -52.0),
            GeoRegion::SouthAmerica => (-81.0, -34.0),
            GeoRegion::Europe => (-25.0, 60.0),
            GeoRegion::Africa => (-17.0, 51.0),
            GeoRegion::MiddleEast => (25.0, 63.0),
            GeoRegion::Asia => (60.0, 180.0),
            GeoRegion::Oceania => (110.0, 180.0),
            GeoRegion::Unknown => (-180.0, 180.0),
        }
    }
    
    /// Estimate network distance between two regions (in arbitrary units)
    /// Used for routing optimization
    pub fn distance_to(&self, other: &GeoRegion) -> u32 {
        if self == other {
            return 0;
        }
        
        // Distance matrix based on typical network latencies
        match (self, other) {
            // Adjacent regions have lower distance
            (GeoRegion::NorthAmerica, GeoRegion::SouthAmerica) => 2,
            (GeoRegion::SouthAmerica, GeoRegion::NorthAmerica) => 2,
            
            (GeoRegion::NorthAmerica, GeoRegion::Europe) => 2,
            (GeoRegion::Europe, GeoRegion::NorthAmerica) => 2,
            
            (GeoRegion::Europe, GeoRegion::Africa) => 2,
            (GeoRegion::Africa, GeoRegion::Europe) => 2,
            
            (GeoRegion::Europe, GeoRegion::MiddleEast) => 1,
            (GeoRegion::MiddleEast, GeoRegion::Europe) => 1,
            
            (GeoRegion::MiddleEast, GeoRegion::Asia) => 2,
            (GeoRegion::Asia, GeoRegion::MiddleEast) => 2,
            
            (GeoRegion::MiddleEast, GeoRegion::Africa) => 2,
            (GeoRegion::Africa, GeoRegion::MiddleEast) => 2,
            
            (GeoRegion::Asia, GeoRegion::Oceania) => 2,
            (GeoRegion::Oceania, GeoRegion::Asia) => 2,
            
            (GeoRegion::NorthAmerica, GeoRegion::Asia) => 3,
            (GeoRegion::Asia, GeoRegion::NorthAmerica) => 3,
            
            (GeoRegion::NorthAmerica, GeoRegion::Oceania) => 3,
            (GeoRegion::Oceania, GeoRegion::NorthAmerica) => 3,
            
            (GeoRegion::Europe, GeoRegion::Asia) => 3,
            (GeoRegion::Asia, GeoRegion::Europe) => 3,
            
            // Distant regions
            (GeoRegion::SouthAmerica, GeoRegion::Asia) => 4,
            (GeoRegion::Asia, GeoRegion::SouthAmerica) => 4,
            
            (GeoRegion::SouthAmerica, GeoRegion::Oceania) => 4,
            (GeoRegion::Oceania, GeoRegion::SouthAmerica) => 4,
            
            (GeoRegion::Africa, GeoRegion::Oceania) => 4,
            (GeoRegion::Oceania, GeoRegion::Africa) => 4,
            
            (GeoRegion::SouthAmerica, GeoRegion::Africa) => 3,
            (GeoRegion::Africa, GeoRegion::SouthAmerica) => 3,
            
            // Unknown regions get high distance
            (GeoRegion::Unknown, _) | (_, GeoRegion::Unknown) => 5,
            
            // Default for any other combination
            _ => 3,
        }
    }
    
    /// Get a human-readable name for the region
    pub fn name(&self) -> &'static str {
        match self {
            GeoRegion::NorthAmerica => "North America",
            GeoRegion::SouthAmerica => "South America",
            GeoRegion::Europe => "Europe",
            GeoRegion::Africa => "Africa",
            GeoRegion::MiddleEast => "Middle East",
            GeoRegion::Asia => "Asia",
            GeoRegion::Oceania => "Oceania",
            GeoRegion::Unknown => "Unknown",
        }
    }
    
    /// Get all known regions (excluding Unknown)
    pub fn all() -> &'static [GeoRegion] {
        &[
            GeoRegion::NorthAmerica,
            GeoRegion::SouthAmerica,
            GeoRegion::Europe,
            GeoRegion::Africa,
            GeoRegion::MiddleEast,
            GeoRegion::Asia,
            GeoRegion::Oceania,
        ]
    }
}

/// Information about a relay node for routing decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayInfo {
    /// Peer ID
    pub peer_id: PeerId,
    /// IP address (for geo-routing)
    pub ip_addr: Option<IpAddr>,
    /// Country code
    pub country_code: Option<String>,
    /// Geographic region
    pub region: GeoRegion,
    /// Autonomous System Number
    pub asn: Option<u32>,
    /// Average latency in ms
    pub avg_latency_ms: u64,
    /// Bandwidth capacity in bytes/sec
    pub bandwidth_bps: u64,
    /// Current load (0.0 - 1.0)
    pub load: f64,
    /// Whether this relay supports exit
    pub is_exit: bool,
    /// Whether this relay supports guard (entry)
    pub is_guard: bool,
    /// Last seen timestamp
    pub last_seen: u64,
    /// Reputation score (0.0 - 1.0)
    pub reputation: f64,
}

impl RelayInfo {
    /// Create a new relay info
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            ip_addr: None,
            country_code: None,
            region: GeoRegion::Unknown,
            asn: None,
            avg_latency_ms: 0,
            bandwidth_bps: 0,
            load: 0.0,
            is_exit: false,
            is_guard: false,
            last_seen: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            reputation: 0.5,
        }
    }

    /// Create with geographic info
    pub fn with_geo(peer_id: PeerId, country_code: &str, ip_addr: Option<IpAddr>) -> Self {
        let mut info = Self::new(peer_id);
        info.country_code = Some(country_code.to_string());
        info.region = GeoRegion::from_country_code(country_code);
        info.ip_addr = ip_addr;
        info
    }

    /// Check if relay is healthy
    pub fn is_healthy(&self) -> bool {
        self.reputation > 0.3 && self.load < 0.9
    }

    /// Calculate fitness score for path selection
    pub fn fitness_score(&self, prefer_low_latency: bool) -> f64 {
        let latency_factor = if prefer_low_latency {
            1.0 / (1.0 + self.avg_latency_ms as f64 / 100.0)
        } else {
            0.5
        };

        let load_factor = 1.0 - self.load;
        let reputation_factor = self.reputation;

        latency_factor * 0.3 + load_factor * 0.3 + reputation_factor * 0.4
    }
}

/// Path health tracking
#[derive(Debug)]
pub struct PathHealth {
    /// Source peer
    pub source: PeerId,
    /// Destination peer
    pub destination: PeerId,
    /// Historical attempts
    attempts: VecDeque<PathAttempt>,
    /// Current censorship status
    pub censorship_status: CensorshipStatus,
    /// Consecutive failures
    pub consecutive_failures: usize,
    /// Last successful attempt
    pub last_success: Option<Instant>,
    /// Average latency (moving average)
    pub avg_latency_ms: f64,
    /// Total bytes transferred
    pub total_bytes: u64,
}

impl PathHealth {
    /// Create a new path health tracker
    pub fn new(source: PeerId, destination: PeerId) -> Self {
        Self {
            source,
            destination,
            attempts: VecDeque::with_capacity(MAX_PATH_HISTORY),
            censorship_status: CensorshipStatus::Unknown,
            consecutive_failures: 0,
            last_success: None,
            avg_latency_ms: 0.0,
            total_bytes: 0,
        }
    }

    /// Record a successful attempt
    pub fn record_success(&mut self, latency_ms: u64, bytes: u64) {
        let attempt = PathAttempt {
            timestamp: Instant::now(),
            success: true,
            failure_type: None,
            latency_ms: Some(latency_ms),
            bytes_transferred: bytes,
        };

        self.add_attempt(attempt);
        self.consecutive_failures = 0;
        self.last_success = Some(Instant::now());
        self.total_bytes += bytes;

        // Update moving average latency
        self.avg_latency_ms = self.avg_latency_ms * 0.9 + latency_ms as f64 * 0.1;

        self.update_censorship_status();
    }

    /// Record a failed attempt
    pub fn record_failure(&mut self, failure_type: FailureType) {
        let attempt = PathAttempt {
            timestamp: Instant::now(),
            success: false,
            failure_type: Some(failure_type),
            latency_ms: None,
            bytes_transferred: 0,
        };

        self.add_attempt(attempt);
        self.consecutive_failures += 1;

        self.update_censorship_status();
    }

    /// Add an attempt to history
    fn add_attempt(&mut self, attempt: PathAttempt) {
        if self.attempts.len() >= MAX_PATH_HISTORY {
            self.attempts.pop_front();
        }
        self.attempts.push_back(attempt);
    }

    /// Calculate failure rate within the window
    pub fn failure_rate(&self) -> f64 {
        let now = Instant::now();
        let recent: Vec<&PathAttempt> = self
            .attempts
            .iter()
            .filter(|a| now.duration_since(a.timestamp) < FAILURE_WINDOW)
            .collect();

        if recent.is_empty() {
            return 0.0;
        }

        let failures = recent.iter().filter(|a| !a.success).count();
        failures as f64 / recent.len() as f64
    }

    /// Calculate censorship-indicative failure rate
    pub fn censorship_failure_rate(&self) -> f64 {
        let now = Instant::now();
        let recent: Vec<&PathAttempt> = self
            .attempts
            .iter()
            .filter(|a| now.duration_since(a.timestamp) < FAILURE_WINDOW)
            .collect();

        if recent.is_empty() {
            return 0.0;
        }

        let censorship_failures: f64 = recent
            .iter()
            .filter(|a| !a.success)
            .filter_map(|a| a.failure_type)
            .filter(|ft| ft.is_censorship_indicator())
            .map(|ft| ft.severity_weight())
            .sum();

        censorship_failures / recent.len() as f64
    }

    /// Update censorship status based on recent history
    fn update_censorship_status(&mut self) {
        let recent_count = self
            .attempts
            .iter()
            .filter(|a| Instant::now().duration_since(a.timestamp) < FAILURE_WINDOW)
            .count();

        if recent_count < MIN_SAMPLES_FOR_DETECTION {
            self.censorship_status = CensorshipStatus::Unknown;
            return;
        }

        let censorship_rate = self.censorship_failure_rate();
        let failure_rate = self.failure_rate();

        self.censorship_status = if censorship_rate > CENSORSHIP_FAILURE_THRESHOLD {
            CensorshipStatus::Confirmed
        } else if censorship_rate > 0.4 || failure_rate > 0.6 {
            CensorshipStatus::Suspected
        } else if failure_rate < 0.2 {
            CensorshipStatus::Clear
        } else {
            CensorshipStatus::Unknown
        };
    }

    /// Check if this path should be avoided
    pub fn should_avoid(&self) -> bool {
        matches!(
            self.censorship_status,
            CensorshipStatus::Confirmed | CensorshipStatus::Suspected
        ) || self.consecutive_failures >= 3
    }

    /// Get health score (0.0 = unhealthy, 1.0 = healthy)
    pub fn health_score(&self) -> f64 {
        let failure_penalty = self.failure_rate();
        let censorship_penalty = self.censorship_failure_rate() * 0.5;
        let recency_bonus = if let Some(last) = self.last_success {
            let age = Instant::now().duration_since(last).as_secs() as f64;
            (1.0 / (1.0 + age / 60.0)).min(0.2) // Bonus for recent success
        } else {
            0.0
        };

        (1.0 - failure_penalty - censorship_penalty + recency_bonus).max(0.0).min(1.0)
    }
}

/// Route selection strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteStrategy {
    /// Prioritize speed
    LowLatency,
    /// Prioritize privacy (more hops, diverse regions)
    HighPrivacy,
    /// Prioritize reliability
    HighReliability,
    /// Balanced approach
    Balanced,
    /// Avoid specific regions (censorship evasion)
    AvoidRegions,
}

/// Configuration for adaptive routing
#[derive(Debug, Clone)]
pub struct AdaptiveRoutingConfig {
    /// Default routing strategy
    pub strategy: RouteStrategy,
    /// Minimum number of hops
    pub min_hops: usize,
    /// Maximum number of hops
    pub max_hops: usize,
    /// Require geographic diversity
    pub require_geo_diversity: bool,
    /// Regions to avoid
    pub avoid_regions: HashSet<GeoRegion>,
    /// ASNs to avoid
    pub avoid_asns: HashSet<u32>,
    /// Failover timeout
    pub failover_timeout: Duration,
    /// Maximum retry attempts
    pub max_retries: usize,
    /// Enable automatic failover
    pub auto_failover: bool,
    /// Path health check interval
    pub health_check_interval: Duration,
}

impl Default for AdaptiveRoutingConfig {
    fn default() -> Self {
        Self {
            strategy: RouteStrategy::Balanced,
            min_hops: 3,
            max_hops: 5,
            require_geo_diversity: true,
            avoid_regions: HashSet::new(),
            avoid_asns: HashSet::new(),
            failover_timeout: Duration::from_secs(10),
            max_retries: 3,
            auto_failover: true,
            health_check_interval: Duration::from_secs(60),
        }
    }
}

/// A computed route through the network
#[derive(Debug, Clone)]
pub struct ComputedRoute {
    /// Route ID
    pub id: [u8; 16],
    /// Ordered list of relay peers
    pub relays: Vec<PeerId>,
    /// Regions covered by this route
    pub regions: Vec<GeoRegion>,
    /// Estimated latency
    pub estimated_latency_ms: u64,
    /// Route health score
    pub health_score: f64,
    /// When route was computed
    pub created_at: Instant,
    /// Number of times used
    pub use_count: usize,
    /// Is this a backup route
    pub is_backup: bool,
}

impl ComputedRoute {
    /// Check if route is still valid
    pub fn is_valid(&self, max_age: Duration) -> bool {
        self.created_at.elapsed() < max_age && self.health_score > 0.3
    }
}

/// Adaptive routing manager
pub struct AdaptiveRouter {
    /// Known relays
    relays: Arc<RwLock<HashMap<PeerId, RelayInfo>>>,
    /// Path health tracking
    path_health: Arc<RwLock<HashMap<(PeerId, PeerId), PathHealth>>>,
    /// Active routes
    active_routes: Arc<RwLock<HashMap<[u8; 16], ComputedRoute>>>,
    /// Backup routes per primary route
    backup_routes: Arc<RwLock<HashMap<[u8; 16], Vec<ComputedRoute>>>>,
    /// Blocked paths (confirmed censorship)
    blocked_paths: Arc<RwLock<HashSet<(PeerId, PeerId)>>>,
    /// Configuration
    config: AdaptiveRoutingConfig,
    /// Statistics
    stats: Arc<RwLock<RoutingStats>>,
}

/// Routing statistics
#[derive(Debug, Clone, Default)]
pub struct RoutingStats {
    /// Total routes computed
    pub routes_computed: u64,
    /// Successful deliveries
    pub successful_deliveries: u64,
    /// Failed deliveries
    pub failed_deliveries: u64,
    /// Failovers triggered
    pub failovers_triggered: u64,
    /// Censorship events detected
    pub censorship_events: u64,
    /// Paths currently blocked
    pub blocked_paths: usize,
    /// Average route computation time (ms)
    pub avg_route_time_ms: f64,
}

impl AdaptiveRouter {
    /// Create a new adaptive router
    pub fn new() -> Self {
        Self::with_config(AdaptiveRoutingConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: AdaptiveRoutingConfig) -> Self {
        Self {
            relays: Arc::new(RwLock::new(HashMap::new())),
            path_health: Arc::new(RwLock::new(HashMap::new())),
            active_routes: Arc::new(RwLock::new(HashMap::new())),
            backup_routes: Arc::new(RwLock::new(HashMap::new())),
            blocked_paths: Arc::new(RwLock::new(HashSet::new())),
            config,
            stats: Arc::new(RwLock::new(RoutingStats::default())),
        }
    }

    /// Register a relay node
    pub fn register_relay(&self, info: RelayInfo) {
        let mut relays = self.relays.write().unwrap();
        relays.insert(info.peer_id, info);
    }

    /// Remove a relay node
    pub fn remove_relay(&self, peer_id: &PeerId) {
        let mut relays = self.relays.write().unwrap();
        relays.remove(peer_id);
    }

    /// Get relay info
    pub fn get_relay(&self, peer_id: &PeerId) -> Option<RelayInfo> {
        let relays = self.relays.read().unwrap();
        relays.get(peer_id).cloned()
    }

    /// Compute a route to a destination
    pub fn compute_route(&self, destination: Option<PeerId>) -> Result<ComputedRoute, RoutingError> {
        let start = Instant::now();
        let relays = self.relays.read().unwrap();
        let blocked = self.blocked_paths.read().unwrap();
        let path_health = self.path_health.read().unwrap();

        // Filter available relays
        let available: Vec<&RelayInfo> = relays
            .values()
            .filter(|r| r.is_healthy())
            .filter(|r| !self.config.avoid_regions.contains(&r.region))
            .filter(|r| r.asn.map_or(true, |asn| !self.config.avoid_asns.contains(&asn)))
            .collect();

        if available.len() < self.config.min_hops {
            return Err(RoutingError::InsufficientRelays {
                required: self.config.min_hops,
                available: available.len(),
            });
        }

        // Select relays based on strategy
        let selected = self.select_relays(&available, &blocked, &path_health, destination)?;

        // Compute health score
        let health_score = self.compute_route_health(&selected, &path_health);

        // Estimate latency
        let estimated_latency: u64 = selected
            .iter()
            .filter_map(|p| relays.get(p))
            .map(|r| r.avg_latency_ms)
            .sum();

        // Generate route ID
        let mut id = [0u8; 16];
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        for peer in &selected {
            hasher.update(peer.to_bytes().as_slice());
        }
        hasher.update(&rand::random::<[u8; 8]>());
        let hash = hasher.finalize();
        id.copy_from_slice(&hash.as_bytes()[..16]);

        // Get regions
        let regions: Vec<GeoRegion> = selected
            .iter()
            .filter_map(|p| relays.get(p).map(|r| r.region))
            .collect();

        let route = ComputedRoute {
            id,
            relays: selected,
            regions,
            estimated_latency_ms: estimated_latency,
            health_score,
            created_at: Instant::now(),
            use_count: 0,
            is_backup: false,
        };

        // Update stats
        if let Ok(mut stats) = self.stats.write() {
            stats.routes_computed += 1;
            let elapsed = start.elapsed().as_millis() as f64;
            stats.avg_route_time_ms = stats.avg_route_time_ms * 0.9 + elapsed * 0.1;
        }

        // Store route
        self.active_routes.write().unwrap().insert(route.id, route.clone());

        // Generate backup routes
        self.generate_backup_routes(&route)?;

        Ok(route)
    }

    /// Select relays for a route based on strategy
    fn select_relays(
        &self,
        available: &[&RelayInfo],
        blocked: &HashSet<(PeerId, PeerId)>,
        path_health: &HashMap<(PeerId, PeerId), PathHealth>,
        _destination: Option<PeerId>,
    ) -> Result<Vec<PeerId>, RoutingError> {
        let num_hops = match self.config.strategy {
            RouteStrategy::HighPrivacy => self.config.max_hops,
            RouteStrategy::LowLatency => self.config.min_hops,
            _ => (self.config.min_hops + self.config.max_hops) / 2,
        };

        // Score and sort relays
        let prefer_low_latency = matches!(self.config.strategy, RouteStrategy::LowLatency);
        let mut scored: Vec<(&RelayInfo, f64)> = available
            .iter()
            .map(|r| (*r, r.fitness_score(prefer_low_latency)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut selected: Vec<PeerId> = Vec::with_capacity(num_hops);
        let mut used_regions: HashSet<GeoRegion> = HashSet::new();
        let mut used_asns: HashSet<u32> = HashSet::new();

        // Select guard (entry) node first
        for (relay, _) in &scored {
            if relay.is_guard && self.is_path_ok(&selected, relay.peer_id, blocked, path_health) {
                if !self.config.require_geo_diversity || !used_regions.contains(&relay.region) {
                    selected.push(relay.peer_id);
                    used_regions.insert(relay.region);
                    if let Some(asn) = relay.asn {
                        used_asns.insert(asn);
                    }
                    break;
                }
            }
        }

        // If no guard found, use any relay
        if selected.is_empty() {
            if let Some((relay, _)) = scored.first() {
                selected.push(relay.peer_id);
                used_regions.insert(relay.region);
                if let Some(asn) = relay.asn {
                    used_asns.insert(asn);
                }
            }
        }

        // Select middle nodes
        for (relay, _) in &scored {
            if selected.len() >= num_hops - 1 {
                break;
            }

            if selected.contains(&relay.peer_id) {
                continue;
            }

            // Check path health
            if !self.is_path_ok(&selected, relay.peer_id, blocked, path_health) {
                continue;
            }

            // Geographic diversity check
            if self.config.require_geo_diversity {
                if used_regions.contains(&relay.region) {
                    continue;
                }
                // ASN diversity
                if let Some(asn) = relay.asn {
                    if used_asns.contains(&asn) {
                        continue;
                    }
                }
            }

            selected.push(relay.peer_id);
            used_regions.insert(relay.region);
            if let Some(asn) = relay.asn {
                used_asns.insert(asn);
            }
        }

        // Select exit node last
        for (relay, _) in &scored {
            if selected.len() >= num_hops {
                break;
            }

            if selected.contains(&relay.peer_id) {
                continue;
            }

            if relay.is_exit && self.is_path_ok(&selected, relay.peer_id, blocked, path_health) {
                selected.push(relay.peer_id);
                break;
            }
        }

        // Fill remaining with any available
        for (relay, _) in &scored {
            if selected.len() >= num_hops {
                break;
            }

            if !selected.contains(&relay.peer_id)
                && self.is_path_ok(&selected, relay.peer_id, blocked, path_health)
            {
                selected.push(relay.peer_id);
            }
        }

        if selected.len() < self.config.min_hops {
            return Err(RoutingError::InsufficientRelays {
                required: self.config.min_hops,
                available: selected.len(),
            });
        }

        Ok(selected)
    }

    /// Check if path between last selected and new peer is OK
    fn is_path_ok(
        &self,
        selected: &[PeerId],
        new_peer: PeerId,
        blocked: &HashSet<(PeerId, PeerId)>,
        path_health: &HashMap<(PeerId, PeerId), PathHealth>,
    ) -> bool {
        if let Some(last) = selected.last() {
            // Check if path is blocked
            if blocked.contains(&(*last, new_peer)) {
                return false;
            }

            // Check path health
            if let Some(health) = path_health.get(&(*last, new_peer)) {
                if health.should_avoid() {
                    return false;
                }
            }
        }

        true
    }

    /// Compute overall route health score
    fn compute_route_health(
        &self,
        route: &[PeerId],
        path_health: &HashMap<(PeerId, PeerId), PathHealth>,
    ) -> f64 {
        if route.len() < 2 {
            return 1.0;
        }

        let mut total_score = 0.0;
        let mut segments = 0;

        for window in route.windows(2) {
            let (from, to) = (window[0], window[1]);
            if let Some(health) = path_health.get(&(from, to)) {
                total_score += health.health_score();
            } else {
                total_score += 0.5; // Unknown path gets neutral score
            }
            segments += 1;
        }

        if segments == 0 {
            1.0
        } else {
            total_score / segments as f64
        }
    }

    /// Generate backup routes for a primary route
    fn generate_backup_routes(&self, primary: &ComputedRoute) -> Result<(), RoutingError> {
        let mut backups = Vec::new();

        // Try to generate 2 backup routes
        for i in 0..2 {
            // Temporarily mark primary relays as "to avoid" for diversity
            let relays = self.relays.read().unwrap();
            let blocked = self.blocked_paths.read().unwrap();
            let path_health = self.path_health.read().unwrap();

            let available: Vec<&RelayInfo> = relays
                .values()
                .filter(|r| r.is_healthy())
                .filter(|r| !primary.relays.contains(&r.peer_id)) // Avoid primary route relays
                .filter(|r| !self.config.avoid_regions.contains(&r.region))
                .collect();

            if available.len() >= self.config.min_hops {
                if let Ok(selected) = self.select_relays(&available, &blocked, &path_health, None) {
                    let health_score = self.compute_route_health(&selected, &path_health);
                    let estimated_latency: u64 = selected
                        .iter()
                        .filter_map(|p| relays.get(p))
                        .map(|r| r.avg_latency_ms)
                        .sum();

                    let regions: Vec<GeoRegion> = selected
                        .iter()
                        .filter_map(|p| relays.get(p).map(|r| r.region))
                        .collect();

                    let mut id = [0u8; 16];
                    id[0] = (i + 1) as u8; // Backup ID
                    id[1..].copy_from_slice(&primary.id[1..]);

                    let backup = ComputedRoute {
                        id,
                        relays: selected,
                        regions,
                        estimated_latency_ms: estimated_latency,
                        health_score,
                        created_at: Instant::now(),
                        use_count: 0,
                        is_backup: true,
                    };

                    backups.push(backup);
                }
            }
        }

        self.backup_routes.write().unwrap().insert(primary.id, backups);
        Ok(())
    }

    /// Record a successful delivery
    pub fn record_success(&self, route_id: &[u8; 16], latency_ms: u64, bytes: u64) {
        // Update route stats
        if let Some(mut route) = self.active_routes.write().unwrap().get_mut(route_id) {
            route.use_count += 1;
        }

        // Update global stats
        if let Ok(mut stats) = self.stats.write() {
            stats.successful_deliveries += 1;
        }
    }

    /// Record a failed delivery and potentially trigger failover
    pub fn record_failure(
        &self,
        route_id: &[u8; 16],
        hop_index: usize,
        failure_type: FailureType,
    ) -> Option<ComputedRoute> {
        let route = {
            let routes = self.active_routes.read().unwrap();
            routes.get(route_id).cloned()
        };

        let route = match route {
            Some(r) => r,
            None => return None,
        };

        // Record failure for the specific path segment
        if hop_index < route.relays.len() - 1 {
            let from = route.relays[hop_index];
            let to = route.relays[hop_index + 1];

            let mut path_health = self.path_health.write().unwrap();
            let health = path_health
                .entry((from, to))
                .or_insert_with(|| PathHealth::new(from, to));
            health.record_failure(failure_type);

            // Check if this path should now be blocked
            if health.censorship_status == CensorshipStatus::Confirmed {
                self.blocked_paths.write().unwrap().insert((from, to));

                if let Ok(mut stats) = self.stats.write() {
                    stats.censorship_events += 1;
                    stats.blocked_paths = self.blocked_paths.read().unwrap().len();
                }
            }
        }

        // Update global stats
        if let Ok(mut stats) = self.stats.write() {
            stats.failed_deliveries += 1;
        }

        // Trigger failover if enabled
        if self.config.auto_failover {
            return self.failover(route_id);
        }

        None
    }

    /// Switch to a backup route
    pub fn failover(&self, route_id: &[u8; 16]) -> Option<ComputedRoute> {
        let backups = self.backup_routes.read().unwrap();

        if let Some(backup_list) = backups.get(route_id) {
            // Find best available backup
            let best = backup_list
                .iter()
                .filter(|b| b.is_valid(Duration::from_secs(300)))
                .max_by(|a, b| a.health_score.partial_cmp(&b.health_score).unwrap());

            if let Some(backup) = best {
                // Update stats
                if let Ok(mut stats) = self.stats.write() {
                    stats.failovers_triggered += 1;
                }

                // Promote backup to active
                self.active_routes.write().unwrap().insert(backup.id, backup.clone());

                return Some(backup.clone());
            }
        }

        // No backup available, compute new route
        if let Ok(new_route) = self.compute_route(None) {
            if let Ok(mut stats) = self.stats.write() {
                stats.failovers_triggered += 1;
            }
            return Some(new_route);
        }

        None
    }

    /// Get routing statistics
    pub fn stats(&self) -> RoutingStats {
        self.stats.read().unwrap().clone()
    }

    /// Get blocked paths
    pub fn blocked_paths(&self) -> Vec<(PeerId, PeerId)> {
        self.blocked_paths.read().unwrap().iter().copied().collect()
    }

    /// Manually block a path
    pub fn block_path(&self, from: PeerId, to: PeerId) {
        self.blocked_paths.write().unwrap().insert((from, to));
    }

    /// Clear a blocked path (for retry)
    pub fn unblock_path(&self, from: PeerId, to: PeerId) {
        self.blocked_paths.write().unwrap().remove(&(from, to));
    }

    /// Get path health
    pub fn get_path_health(&self, from: PeerId, to: PeerId) -> Option<(f64, CensorshipStatus)> {
        let path_health = self.path_health.read().unwrap();
        path_health.get(&(from, to)).map(|h| (h.health_score(), h.censorship_status))
    }

    /// Get all relays with censorship issues
    pub fn get_censored_paths(&self) -> Vec<(PeerId, PeerId, CensorshipStatus)> {
        let path_health = self.path_health.read().unwrap();
        path_health
            .iter()
            .filter(|(_, h)| matches!(h.censorship_status, CensorshipStatus::Confirmed | CensorshipStatus::Suspected))
            .map(|((from, to), h)| (*from, *to, h.censorship_status))
            .collect()
    }

    /// Get number of registered relays
    pub fn relay_count(&self) -> usize {
        self.relays.read().unwrap().len()
    }

    /// Get number of active routes
    pub fn active_route_count(&self) -> usize {
        self.active_routes.read().unwrap().len()
    }
}

impl Default for AdaptiveRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Routing error types
#[derive(Debug, Clone)]
pub enum RoutingError {
    /// Not enough relays available
    InsufficientRelays { required: usize, available: usize },
    /// No path found to destination
    NoPathFound,
    /// All paths are blocked
    AllPathsBlocked,
    /// Route expired
    RouteExpired,
    /// Failover failed
    FailoverFailed,
}

impl std::fmt::Display for RoutingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoutingError::InsufficientRelays { required, available } => {
                write!(f, "Need {} relays, only {} available", required, available)
            }
            RoutingError::NoPathFound => write!(f, "No path found to destination"),
            RoutingError::AllPathsBlocked => write!(f, "All paths are blocked"),
            RoutingError::RouteExpired => write!(f, "Route has expired"),
            RoutingError::FailoverFailed => write!(f, "Failover failed"),
        }
    }
}

impl std::error::Error for RoutingError {}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;

    fn create_peer_id() -> PeerId {
        identity::Keypair::generate_ed25519().public().to_peer_id()
    }

    fn create_relay_info(country: &str, is_guard: bool, is_exit: bool) -> RelayInfo {
        let mut info = RelayInfo::with_geo(create_peer_id(), country, None);
        info.is_guard = is_guard;
        info.is_exit = is_exit;
        info.reputation = 0.8;
        info.avg_latency_ms = 50;
        info
    }

    #[test]
    fn test_geo_region_mapping() {
        assert_eq!(GeoRegion::from_country_code("US"), GeoRegion::NorthAmerica);
        assert_eq!(GeoRegion::from_country_code("DE"), GeoRegion::Europe);
        assert_eq!(GeoRegion::from_country_code("JP"), GeoRegion::Asia);
        assert_eq!(GeoRegion::from_country_code("AU"), GeoRegion::Oceania);
        assert_eq!(GeoRegion::from_country_code("BR"), GeoRegion::SouthAmerica);
        assert_eq!(GeoRegion::from_country_code("XX"), GeoRegion::Unknown);
    }

    #[test]
    fn test_failure_type_indicators() {
        assert!(FailureType::ConnectionReset.is_censorship_indicator());
        assert!(FailureType::ContentTampering.is_censorship_indicator());
        assert!(FailureType::DnsFailure.is_censorship_indicator());
        assert!(!FailureType::ConnectionRefused.is_censorship_indicator());
        assert!(!FailureType::HttpError(404).is_censorship_indicator());
    }

    #[test]
    fn test_path_health_tracking() {
        let from = create_peer_id();
        let to = create_peer_id();
        let mut health = PathHealth::new(from, to);

        // Record some successes
        health.record_success(50, 1000);
        health.record_success(60, 2000);
        assert_eq!(health.consecutive_failures, 0);
        assert!(health.last_success.is_some());

        // Record failure
        health.record_failure(FailureType::Timeout);
        assert_eq!(health.consecutive_failures, 1);

        // More successes reset counter
        health.record_success(55, 1500);
        assert_eq!(health.consecutive_failures, 0);
    }

    #[test]
    fn test_censorship_detection() {
        let from = create_peer_id();
        let to = create_peer_id();
        let mut health = PathHealth::new(from, to);

        // Record many censorship-indicative failures
        for _ in 0..10 {
            health.record_failure(FailureType::ConnectionReset);
        }

        assert_eq!(health.censorship_status, CensorshipStatus::Confirmed);
        assert!(health.should_avoid());
    }

    #[test]
    fn test_router_creation() {
        let router = AdaptiveRouter::new();
        assert_eq!(router.relay_count(), 0);
        assert_eq!(router.active_route_count(), 0);
    }

    #[test]
    fn test_relay_registration() {
        let router = AdaptiveRouter::new();

        let relay = create_relay_info("US", true, false);
        let peer_id = relay.peer_id;
        router.register_relay(relay);

        assert_eq!(router.relay_count(), 1);
        assert!(router.get_relay(&peer_id).is_some());

        router.remove_relay(&peer_id);
        assert_eq!(router.relay_count(), 0);
    }

    #[test]
    fn test_route_computation() {
        let config = AdaptiveRoutingConfig {
            min_hops: 2,
            max_hops: 3,
            require_geo_diversity: false,
            ..Default::default()
        };
        let router = AdaptiveRouter::with_config(config);

        // Register relays
        for country in &["US", "DE", "JP", "AU"] {
            let relay = create_relay_info(country, true, true);
            router.register_relay(relay);
        }

        let route = router.compute_route(None).unwrap();
        assert!(route.relays.len() >= 2);
        assert!(route.health_score > 0.0);
    }

    #[test]
    fn test_insufficient_relays() {
        let config = AdaptiveRoutingConfig {
            min_hops: 5,
            ..Default::default()
        };
        let router = AdaptiveRouter::with_config(config);

        // Only register 2 relays
        router.register_relay(create_relay_info("US", true, false));
        router.register_relay(create_relay_info("DE", false, true));

        let result = router.compute_route(None);
        assert!(matches!(result, Err(RoutingError::InsufficientRelays { .. })));
    }

    #[test]
    fn test_record_success() {
        let config = AdaptiveRoutingConfig {
            min_hops: 2,
            require_geo_diversity: false,
            ..Default::default()
        };
        let router = AdaptiveRouter::with_config(config);

        for country in &["US", "DE", "JP"] {
            router.register_relay(create_relay_info(country, true, true));
        }

        let route = router.compute_route(None).unwrap();
        router.record_success(&route.id, 100, 5000);

        let stats = router.stats();
        assert_eq!(stats.successful_deliveries, 1);
    }

    #[test]
    fn test_failover() {
        let config = AdaptiveRoutingConfig {
            min_hops: 2,
            require_geo_diversity: false,
            auto_failover: true,
            ..Default::default()
        };
        let router = AdaptiveRouter::with_config(config);

        // Register enough relays for backup routes
        for country in &["US", "DE", "JP", "AU", "GB", "FR"] {
            router.register_relay(create_relay_info(country, true, true));
        }

        let route = router.compute_route(None).unwrap();

        // Record failures to trigger failover
        let backup = router.record_failure(&route.id, 0, FailureType::ConnectionReset);

        let stats = router.stats();
        assert!(stats.failovers_triggered > 0 || backup.is_some());
    }

    #[test]
    fn test_blocked_path() {
        let router = AdaptiveRouter::new();

        let from = create_peer_id();
        let to = create_peer_id();

        assert!(router.blocked_paths().is_empty());

        // Manually block path
        router.blocked_paths.write().unwrap().insert((from, to));

        assert_eq!(router.blocked_paths().len(), 1);

        // Unblock
        router.unblock_path(from, to);
        assert!(router.blocked_paths().is_empty());
    }

    #[test]
    fn test_relay_fitness_score() {
        let mut relay = RelayInfo::new(create_peer_id());
        relay.avg_latency_ms = 50;
        relay.load = 0.3;
        relay.reputation = 0.9;

        let score = relay.fitness_score(true);
        assert!(score > 0.5);

        // High load should reduce score
        relay.load = 0.95;
        let low_score = relay.fitness_score(true);
        assert!(low_score < score);
    }

    #[test]
    fn test_routing_strategies() {
        let low_latency = AdaptiveRoutingConfig {
            strategy: RouteStrategy::LowLatency,
            min_hops: 2,
            max_hops: 5,
            require_geo_diversity: false,
            ..Default::default()
        };

        let high_privacy = AdaptiveRoutingConfig {
            strategy: RouteStrategy::HighPrivacy,
            min_hops: 2,
            max_hops: 5,
            require_geo_diversity: false,
            ..Default::default()
        };

        let router_fast = AdaptiveRouter::with_config(low_latency);
        let router_private = AdaptiveRouter::with_config(high_privacy);

        for country in &["US", "DE", "JP", "AU", "GB"] {
            router_fast.register_relay(create_relay_info(country, true, true));
            router_private.register_relay(create_relay_info(country, true, true));
        }

        let fast_route = router_fast.compute_route(None).unwrap();
        let private_route = router_private.compute_route(None).unwrap();

        // High privacy should have more hops
        assert!(private_route.relays.len() >= fast_route.relays.len());
    }

    #[test]
    fn test_config_defaults() {
        let config = AdaptiveRoutingConfig::default();
        assert_eq!(config.strategy, RouteStrategy::Balanced);
        assert_eq!(config.min_hops, 3);
        assert!(config.auto_failover);
    }

    #[test]
    fn test_route_validity() {
        let route = ComputedRoute {
            id: [0u8; 16],
            relays: vec![create_peer_id(), create_peer_id()],
            regions: vec![GeoRegion::NorthAmerica, GeoRegion::Europe],
            estimated_latency_ms: 100,
            health_score: 0.8,
            created_at: Instant::now(),
            use_count: 0,
            is_backup: false,
        };

        assert!(route.is_valid(Duration::from_secs(300)));

        // Low health score route is invalid
        let mut unhealthy = route.clone();
        unhealthy.health_score = 0.1;
        assert!(!unhealthy.is_valid(Duration::from_secs(300)));
    }
}
