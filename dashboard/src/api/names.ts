import { apiFetch, apiPost } from './client';
import type { NameResolveResponse } from './types';

export const names = {
  resolve: (name: string) =>
    apiFetch<NameResolveResponse>(`/api/names/${encodeURIComponent(name)}`),

  register: (name: string, records: unknown[]) =>
    apiPost<{ success: boolean }>(`/api/names/${encodeURIComponent(name)}`, {
      records,
    }),
};
