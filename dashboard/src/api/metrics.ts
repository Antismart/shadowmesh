import { apiFetch } from './client';
import type { HealthResponse, MetricsResponse } from './types';

export const metrics = {
  health: () => apiFetch<HealthResponse>('/health'),

  stats: () => apiFetch<MetricsResponse>('/metrics'),
};
