import { apiFetch, apiPost, apiDelete } from './client';
import type { NameRecord, NameResolveResponse, NameAssignResponse } from './types';

export const names = {
  list: () => apiFetch<NameRecord[]>('/api/names'),

  resolve: (name: string) =>
    apiFetch<NameResolveResponse>(`/api/names/${encodeURIComponent(name)}`),

  assign: (name: string, cid: string) =>
    apiPost<NameAssignResponse>(
      `/api/names/${encodeURIComponent(name)}/assign`,
      { cid },
    ),

  remove: (name: string) =>
    apiDelete<{ success: boolean }>(`/api/names/${encodeURIComponent(name)}`),
};
