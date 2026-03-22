import { useState, useEffect, useCallback } from 'react';
import { Outlet } from 'react-router-dom';
import Sidebar from './Sidebar';
import CommandPalette from '../components/CommandPalette';

export default function RootLayout() {
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);

  const openPalette = useCallback(() => setCommandPaletteOpen(true), []);
  const closePalette = useCallback(() => setCommandPaletteOpen(false), []);

  /* Global Cmd+K / Ctrl+K listener */
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        setCommandPaletteOpen((prev) => !prev);
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, []);

  return (
    <div className="flex h-screen bg-mesh-bg">
      <Sidebar onSearchClick={openPalette} />
      <main className="flex-1 overflow-y-auto">
        <div className="max-w-content mx-auto px-6 py-8">
          <Outlet />
        </div>
      </main>
      <CommandPalette open={commandPaletteOpen} onClose={closePalette} />
    </div>
  );
}
