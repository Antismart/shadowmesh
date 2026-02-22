import { apiFetch, apiPost, apiDelete } from './client';
import type { ApiKeyInfo, ApiKeyCreateResponse } from './types';

export const keys = {
  list: () => apiFetch<ApiKeyInfo[]>('/api/keys/'),

  create: (name: string, scopes?: string[]) =>
    apiPost<ApiKeyCreateResponse>('/api/keys/', { name, scopes }),

  revoke: (id: string) =>
    apiPost<{ success: boolean }>(`/api/keys/${id}/revoke`, {}),

  rotate: (id: string) =>
    apiPost<ApiKeyCreateResponse>(`/api/keys/${id}/rotate`, {}),

  remove: (id: string) =>
    apiDelete<{ success: boolean }>(`/api/keys/${id}`),
};
