export interface Deployment {
  name: string;
  cid: string;
  size: number;
  file_count: number;
  created_at: string;
  source: 'upload' | 'github';
  branch: string | null;
  repo_url: string | null;
  build_status: string;
  build_logs: string | null;
  status: string;
  domain: string | null;
}

export interface DeployResponse {
  success: boolean;
  cid: string;
  url: string;
  ipfs_url: string;
  shadow_url: string;
  file_count: number;
  total_size: number;
}

export interface DeployLogsResponse {
  success: boolean;
  status: string;
  logs: string;
}

export interface GithubUser {
  login: string;
  avatar_url: string | null;
}

export interface GithubStatus {
  connected: boolean;
  user: GithubUser | null;
}

export interface GithubRepo {
  id: number;
  name: string;
  full_name: string;
  html_url: string;
  private: boolean;
  default_branch: string;
}

export interface HealthResponse {
  status: string;
  version: string;
  uptime_seconds: number;
  ipfs_connected: boolean;
}

export interface MetricsResponse {
  uptime_seconds: number;
  requests_total: number;
  requests_success: number;
  requests_error: number;
  bytes_served: number;
  cache: {
    total_entries: number;
    expired_entries: number;
    max_entries: number;
  };
  config: {
    cache_max_size_mb: number;
    cache_ttl_seconds: number;
    rate_limit_enabled: boolean;
    rate_limit_rps: number;
    ipfs_connected: boolean;
  };
}

export interface ApiKeyInfo {
  id: string;
  name: string;
  prefix: string;
  created_at: string;
  expires_at: string | null;
  last_used: string | null;
  enabled: boolean;
  scopes: string[];
  is_expired: boolean;
}

export interface ApiKeyCreateResponse {
  id: string;
  key: string;
  name: string;
  prefix: string;
}

export interface NameRecord {
  name: string;
  name_hash: string;
  owner_pubkey: string;
  records: Array<{ Content?: { cid: string } }>;
  sequence: number;
  created_at: number;
  updated_at: number;
  ttl_seconds: number;
  signature: string;
}

export interface NameResolveResponse {
  found: boolean;
  record?: NameRecord;
}

export interface NameAssignResponse {
  success: boolean;
  name: string;
  name_hash: string;
  cid: string;
}
