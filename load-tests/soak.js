/**
 * ShadowMesh Gateway - Soak Test
 *
 * Tests system stability over an extended period to detect memory leaks,
 * resource exhaustion, or gradual performance degradation.
 *
 * Run with: k6 run soak.js --env BASE_URL=http://localhost:8081
 *
 * Note: This test runs for 4+ hours by default. Adjust stages for shorter runs.
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

// Custom metrics for long-running analysis
const errorRate = new Rate('errors');
const latencyP50 = new Trend('latency_p50');
const latencyP95 = new Trend('latency_p95');
const latencyP99 = new Trend('latency_p99');

export const options = {
  stages: [
    { duration: '5m', target: 30 },     // Ramp up
    { duration: '4h', target: 30 },     // 4 hour soak at moderate load
    { duration: '5m', target: 0 },      // Ramp down
  ],
  thresholds: {
    http_req_duration: ['p(99)<1000'],   // 99% < 1s over long period
    errors: ['rate<0.001'],               // Very low error rate for soak
    http_req_failed: ['rate<0.001'],      // Almost zero failures
  },
};

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8081';

// Endpoints to test
const endpoints = [
  { path: '/health', weight: 40, name: 'health' },
  { path: '/metrics', weight: 20, name: 'metrics' },
  { path: '/metrics/prometheus', weight: 20, name: 'prometheus' },
  { path: '/dashboard', weight: 15, name: 'dashboard' },
  { path: '/', weight: 5, name: 'index' },
];

function selectEndpoint() {
  const rand = Math.random() * 100;
  let cumulative = 0;

  for (const ep of endpoints) {
    cumulative += ep.weight;
    if (rand < cumulative) {
      return ep;
    }
  }
  return endpoints[0];
}

export default function() {
  const endpoint = selectEndpoint();
  const res = http.get(`${BASE_URL}${endpoint.path}`);

  const ok = check(res, {
    'status 200': (r) => r.status === 200,
    'response time OK': (r) => r.timings.duration < 1000,
  });

  errorRate.add(!ok);

  // Track latency trends over time
  latencyP50.add(res.timings.duration);
  latencyP95.add(res.timings.duration);
  latencyP99.add(res.timings.duration);

  // Consistent sleep for steady load
  sleep(2);
}

// Periodic status logging during soak test
export function setup() {
  console.log('Starting soak test...');
  console.log(`Target URL: ${BASE_URL}`);
  console.log('This test will run for approximately 4 hours and 10 minutes.');
  console.log('Monitor system metrics (CPU, memory, connections) externally.');
  return {};
}

export function teardown(data) {
  console.log('\nSoak test completed.');
  console.log('Review results for gradual performance degradation.');
}

export function handleSummary(data) {
  const { metrics } = data;

  const summary = {
    timestamp: new Date().toISOString(),
    duration_seconds: data.state.testRunDurationMs / 1000,
    total_requests: metrics.http_reqs.values.count,
    requests_per_second: metrics.http_reqs.values.rate,
    latency: {
      avg: metrics.http_req_duration.values.avg,
      min: metrics.http_req_duration.values.min,
      max: metrics.http_req_duration.values.max,
      p50: metrics.http_req_duration.values['p(50)'],
      p90: metrics.http_req_duration.values['p(90)'],
      p95: metrics.http_req_duration.values['p(95)'],
      p99: metrics.http_req_duration.values['p(99)'],
    },
    error_rate: metrics.errors.values.rate,
    passed: metrics.errors.values.rate < 0.001 &&
            metrics.http_req_duration.values['p(99)'] < 1000,
  };

  console.log('\n========== SOAK TEST SUMMARY ==========');
  console.log(`Duration: ${(summary.duration_seconds / 3600).toFixed(2)} hours`);
  console.log(`Total Requests: ${summary.total_requests}`);
  console.log(`Avg Request Rate: ${summary.requests_per_second.toFixed(2)}/s`);
  console.log(`Latency P50: ${summary.latency.p50.toFixed(2)}ms`);
  console.log(`Latency P95: ${summary.latency.p95.toFixed(2)}ms`);
  console.log(`Latency P99: ${summary.latency.p99.toFixed(2)}ms`);
  console.log(`Max Latency: ${summary.latency.max.toFixed(2)}ms`);
  console.log(`Error Rate: ${(summary.error_rate * 100).toFixed(4)}%`);
  console.log(`Result: ${summary.passed ? 'PASSED' : 'FAILED'}`);
  console.log('========================================\n');

  return {
    'soak-results.json': JSON.stringify(summary, null, 2),
  };
}
