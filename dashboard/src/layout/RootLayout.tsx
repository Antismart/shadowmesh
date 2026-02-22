import { Outlet } from 'react-router-dom';
import Sidebar from './Sidebar';

export default function RootLayout() {
  return (
    <div className="flex h-screen bg-mesh-bg">
      <Sidebar />
      <main className="flex-1 overflow-y-auto">
        <div className="max-w-content mx-auto px-6 py-8">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
