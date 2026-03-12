import { useEffect, useState, useMemo } from 'react';
import { github } from '../api/github';
import type { TreeEntry } from '../api/github';

interface FolderBrowserProps {
  repo: string;
  branch: string;
  value: string;
  onChange: (path: string) => void;
}

interface TreeNode {
  name: string;
  path: string;
  children: TreeNode[];
}

function buildTree(entries: TreeEntry[]): TreeNode[] {
  const dirs = entries.filter((e) => e.type === 'tree');
  const root: TreeNode[] = [];
  const map = new Map<string, TreeNode>();

  dirs.sort((a, b) => a.path.localeCompare(b.path));

  for (const dir of dirs) {
    const name = dir.path.split('/').pop()!;
    const node: TreeNode = { name, path: dir.path, children: [] };
    map.set(dir.path, node);

    const lastSlash = dir.path.lastIndexOf('/');
    const parentPath = lastSlash > -1 ? dir.path.substring(0, lastSlash) : '';

    if (parentPath && map.has(parentPath)) {
      map.get(parentPath)!.children.push(node);
    } else if (!parentPath) {
      root.push(node);
    }
  }

  return root;
}

function FolderRow({
  node,
  depth,
  selected,
  onSelect,
}: {
  node: TreeNode;
  depth: number;
  selected: string;
  onSelect: (path: string) => void;
}) {
  const [expanded, setExpanded] = useState(depth === 0);
  const isSelected = selected === node.path;
  const hasChildren = node.children.length > 0;

  return (
    <>
      <button
        type="button"
        onClick={() => onSelect(node.path)}
        className={`flex items-center gap-1.5 w-full text-left px-2 py-1 text-sm rounded transition-colors ${
          isSelected
            ? 'bg-mesh-accent/10 text-mesh-accent'
            : 'text-mesh-text hover:bg-mesh-surface-hover'
        }`}
        style={{ paddingLeft: `${depth * 16 + 8}px` }}
      >
        {hasChildren ? (
          <span
            onClick={(e) => {
              e.stopPropagation();
              setExpanded(!expanded);
            }}
            className="w-4 h-4 flex items-center justify-center text-mesh-muted cursor-pointer"
          >
            {expanded ? '▾' : '▸'}
          </span>
        ) : (
          <span className="w-4" />
        )}
        <svg className="w-4 h-4 text-mesh-muted flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
        </svg>
        <span className="truncate">{node.name}</span>
      </button>
      {expanded &&
        hasChildren &&
        node.children.map((child) => (
          <FolderRow
            key={child.path}
            node={child}
            depth={depth + 1}
            selected={selected}
            onSelect={onSelect}
          />
        ))}
    </>
  );
}

export default function FolderBrowser({ repo, branch, value, onChange }: FolderBrowserProps) {
  const [entries, setEntries] = useState<TreeEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!repo || !branch) return;
    setLoading(true);
    setError('');
    github
      .tree(repo, branch)
      .then((res) => setEntries(res.entries))
      .catch(() => setError('Failed to load folder structure'))
      .finally(() => setLoading(false));
  }, [repo, branch]);

  const tree = useMemo(() => buildTree(entries), [entries]);

  if (loading) {
    return (
      <div className="border border-mesh-border rounded-lg p-3 max-h-60">
        <div className="flex items-center gap-2 text-sm text-mesh-muted">
          <div className="w-4 h-4 border-2 border-mesh-accent border-t-transparent rounded-full animate-spin" />
          Loading folder structure...
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="border border-mesh-border rounded-lg p-3">
        <p className="text-sm text-mesh-muted">{error}</p>
      </div>
    );
  }

  return (
    <div className="border border-mesh-border rounded-lg overflow-hidden max-h-60 overflow-y-auto">
      <button
        type="button"
        onClick={() => onChange('')}
        className={`flex items-center gap-1.5 w-full text-left px-2 py-1.5 text-sm border-b border-mesh-border transition-colors ${
          !value
            ? 'bg-mesh-accent/10 text-mesh-accent'
            : 'text-mesh-text hover:bg-mesh-surface-hover'
        }`}
      >
        <span className="w-4" />
        <svg className="w-4 h-4 text-mesh-muted flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
        </svg>
        <span>./ (repository root)</span>
      </button>
      <div className="py-1">
        {tree.map((node) => (
          <FolderRow
            key={node.path}
            node={node}
            depth={0}
            selected={value}
            onSelect={onChange}
          />
        ))}
      </div>
    </div>
  );
}
