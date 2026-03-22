import {
  useState,
  useEffect,
  useRef,
  useCallback,
  useMemo,
} from 'react';
import { useNavigate } from 'react-router-dom';
import { deployments as deploymentsApi } from '../api/deployments';
import { names as namesApi } from '../api/names';
import type { Deployment, NameRecord } from '../api/types';

/* ------------------------------------------------------------------ */
/*  Types                                                              */
/* ------------------------------------------------------------------ */

interface CommandItem {
  id: string;
  category: 'Pages' | 'Deployments' | 'Domains';
  label: string;
  description?: string;
  icon: string; // SVG path
  to: string;
  shortcut?: string;
}

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
}

/* ------------------------------------------------------------------ */
/*  Static page entries                                                */
/* ------------------------------------------------------------------ */

const PAGE_ITEMS: CommandItem[] = [
  {
    id: 'page-overview',
    category: 'Pages',
    label: 'Overview',
    description: 'Dashboard home',
    icon: 'M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-4 0h4',
    to: '/',
  },
  {
    id: 'page-import',
    category: 'Pages',
    label: 'Import',
    description: 'Deploy a new project',
    icon: 'M12 4v16m8-8H4',
    to: '/new',
  },
  {
    id: 'page-domains',
    category: 'Pages',
    label: 'Domains',
    description: 'Manage name records',
    icon: 'M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9',
    to: '/domains',
  },
  {
    id: 'page-analytics',
    category: 'Pages',
    label: 'Usage',
    description: 'Analytics & metrics',
    icon: 'M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z',
    to: '/analytics',
  },
  {
    id: 'page-settings',
    category: 'Pages',
    label: 'Settings',
    description: 'API keys & configuration',
    icon: 'M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z M15 12a3 3 0 11-6 0 3 3 0 016 0z',
    to: '/settings',
  },
];

/* ------------------------------------------------------------------ */
/*  Helpers                                                            */
/* ------------------------------------------------------------------ */

/** Simple case-insensitive fuzzy match: every character in the query
 *  must appear in order somewhere in the target string. */
function fuzzyMatch(query: string, target: string): boolean {
  const q = query.toLowerCase();
  const t = target.toLowerCase();
  let qi = 0;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) qi++;
  }
  return qi === q.length;
}

/* Deployment icon path (cube) */
const DEPLOY_ICON =
  'M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4';

/* Domain icon path (link) */
const DOMAIN_ICON =
  'M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1';

/* ------------------------------------------------------------------ */
/*  Component                                                          */
/* ------------------------------------------------------------------ */

export default function CommandPalette({ open, onClose }: CommandPaletteProps) {
  const navigate = useNavigate();
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);

  /* ----- Data cache (fetched once per open, cached for session) ----- */
  const cachedDeployments = useRef<Deployment[] | null>(null);
  const cachedNames = useRef<NameRecord[] | null>(null);
  const [deploymentItems, setDeploymentItems] = useState<CommandItem[]>([]);
  const [domainItems, setDomainItems] = useState<CommandItem[]>([]);

  /* Fetch data when palette opens */
  useEffect(() => {
    if (!open) return;

    let cancelled = false;

    const loadData = async () => {
      // Deployments
      if (cachedDeployments.current === null) {
        try {
          const list = await deploymentsApi.list();
          if (!cancelled) cachedDeployments.current = list;
        } catch {
          /* silently skip if API unavailable */
        }
      }
      if (!cancelled && cachedDeployments.current) {
        setDeploymentItems(
          cachedDeployments.current.map((d) => ({
            id: `deploy-${d.cid}`,
            category: 'Deployments' as const,
            label: d.name,
            description: d.cid.slice(0, 12) + '...',
            icon: DEPLOY_ICON,
            to: `/deployments/${d.cid}`,
          })),
        );
      }

      // Names / Domains
      if (cachedNames.current === null) {
        try {
          const list = await namesApi.list();
          if (!cancelled) cachedNames.current = list;
        } catch {
          /* silently skip */
        }
      }
      if (!cancelled && cachedNames.current) {
        setDomainItems(
          cachedNames.current.map((n) => ({
            id: `domain-${n.name_hash}`,
            category: 'Domains' as const,
            label: n.name,
            description: n.records?.[0]?.Content?.cid
              ? n.records[0].Content.cid.slice(0, 12) + '...'
              : 'No record',
            icon: DOMAIN_ICON,
            to: '/domains',
          })),
        );
      }
    };

    loadData();
    return () => {
      cancelled = true;
    };
  }, [open]);

  /* ----- Reset state on open ----- */
  useEffect(() => {
    if (open) {
      setQuery('');
      setActiveIndex(0);
      requestAnimationFrame(() => inputRef.current?.focus());
      document.body.style.overflow = 'hidden';
    } else {
      document.body.style.overflow = '';
    }
  }, [open]);

  /* ----- Filtered & grouped results ----- */
  const allItems = useMemo(
    () => [...PAGE_ITEMS, ...deploymentItems, ...domainItems],
    [deploymentItems, domainItems],
  );

  const filtered = useMemo(() => {
    if (!query.trim()) return allItems;
    return allItems.filter(
      (item) =>
        fuzzyMatch(query, item.label) ||
        (item.description && fuzzyMatch(query, item.description)),
    );
  }, [query, allItems]);

  /* Group by category, preserving order */
  const grouped = useMemo(() => {
    const categories: Array<{ name: string; items: CommandItem[] }> = [];
    const seen = new Set<string>();
    for (const item of filtered) {
      if (!seen.has(item.category)) {
        seen.add(item.category);
        categories.push({
          name: item.category,
          items: filtered.filter((i) => i.category === item.category),
        });
      }
    }
    return categories;
  }, [filtered]);

  /* Clamp active index when results change */
  useEffect(() => {
    setActiveIndex((prev) => Math.min(prev, Math.max(0, filtered.length - 1)));
  }, [filtered.length]);

  /* ----- Navigation helpers ----- */
  const selectItem = useCallback(
    (item: CommandItem) => {
      onClose();
      navigate(item.to);
    },
    [navigate, onClose],
  );

  /* Scroll the active item into view */
  useEffect(() => {
    if (!listRef.current) return;
    const active = listRef.current.querySelector('[data-active="true"]');
    active?.scrollIntoView({ block: 'nearest' });
  }, [activeIndex]);

  /* ----- Keyboard handler ----- */
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          setActiveIndex((i) => (i + 1) % Math.max(1, filtered.length));
          break;
        case 'ArrowUp':
          e.preventDefault();
          setActiveIndex(
            (i) => (i - 1 + filtered.length) % Math.max(1, filtered.length),
          );
          break;
        case 'Enter':
          e.preventDefault();
          if (filtered[activeIndex]) selectItem(filtered[activeIndex]);
          break;
        case 'Escape':
          e.preventDefault();
          onClose();
          break;
      }
    },
    [filtered, activeIndex, selectItem, onClose],
  );

  if (!open) return null;

  /* ----- Flat index counter for mapping group rows to global index ----- */
  let flatIndex = 0;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[15vh]"
      onKeyDown={handleKeyDown}
    >
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/70"
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Card */}
      <div className="relative z-10 w-full max-w-lg mx-4 bg-mesh-surface border border-mesh-border rounded-lg shadow-2xl overflow-hidden">
        {/* Search input */}
        <div className="flex items-center gap-3 px-4 border-b border-mesh-border">
          <svg
            className="w-4 h-4 text-mesh-muted flex-shrink-0"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            strokeWidth={2}
            aria-hidden="true"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
            />
          </svg>
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setActiveIndex(0);
            }}
            placeholder="Search pages, deployments, domains..."
            className="flex-1 bg-transparent py-3 text-sm text-mesh-text placeholder-mesh-muted outline-none"
            aria-label="Command palette search"
          />
          <kbd className="text-[10px] text-mesh-muted border border-mesh-border rounded px-1.5 py-0.5 leading-none select-none">
            ESC
          </kbd>
        </div>

        {/* Results */}
        <div
          ref={listRef}
          className="max-h-72 overflow-y-auto py-2"
          role="listbox"
          aria-label="Command palette results"
        >
          {filtered.length === 0 ? (
            <div className="px-4 py-8 text-center text-sm text-mesh-muted">
              No results found.
            </div>
          ) : (
            grouped.map((group) => (
              <div key={group.name}>
                <div className="px-4 pt-2 pb-1 text-[11px] font-medium uppercase tracking-wider text-mesh-muted select-none">
                  {group.name}
                </div>
                {group.items.map((item) => {
                  const idx = flatIndex++;
                  const isActive = idx === activeIndex;
                  return (
                    <button
                      key={item.id}
                      role="option"
                      aria-selected={isActive}
                      data-active={isActive}
                      onClick={() => selectItem(item)}
                      onMouseEnter={() => setActiveIndex(idx)}
                      className={`flex items-center gap-3 w-full px-4 py-2 text-left text-sm transition-colors duration-75 ${
                        isActive
                          ? 'bg-mesh-border/50 text-mesh-text'
                          : 'text-mesh-muted hover:text-mesh-text'
                      }`}
                    >
                      <svg
                        className="w-4 h-4 flex-shrink-0"
                        fill="none"
                        viewBox="0 0 24 24"
                        stroke="currentColor"
                        strokeWidth={1.5}
                        aria-hidden="true"
                      >
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          d={item.icon}
                        />
                      </svg>
                      <span className="flex-1 truncate">{item.label}</span>
                      {item.description && (
                        <span className="text-xs text-mesh-muted truncate max-w-[140px]">
                          {item.description}
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center gap-4 px-4 py-2 border-t border-mesh-border text-[11px] text-mesh-muted select-none">
          <span className="flex items-center gap-1">
            <kbd className="border border-mesh-border rounded px-1 py-0.5 leading-none">&uarr;</kbd>
            <kbd className="border border-mesh-border rounded px-1 py-0.5 leading-none">&darr;</kbd>
            navigate
          </span>
          <span className="flex items-center gap-1">
            <kbd className="border border-mesh-border rounded px-1 py-0.5 leading-none">&crarr;</kbd>
            select
          </span>
          <span className="flex items-center gap-1">
            <kbd className="border border-mesh-border rounded px-1 py-0.5 leading-none">esc</kbd>
            close
          </span>
        </div>
      </div>
    </div>
  );
}
