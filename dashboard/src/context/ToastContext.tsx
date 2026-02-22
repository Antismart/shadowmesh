import { createContext, useCallback, useContext, useState, type ReactNode } from 'react';

export type ToastType = 'success' | 'error' | 'info';

interface Toast {
  id: number;
  type: ToastType;
  message: string;
}

interface ToastState {
  toasts: Toast[];
  addToast: (type: ToastType, message: string) => void;
  removeToast: (id: number) => void;
}

const ToastContext = createContext<ToastState>({
  toasts: [],
  addToast: () => {},
  removeToast: () => {},
});

let nextId = 0;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const removeToast = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const addToast = useCallback(
    (type: ToastType, message: string) => {
      const id = nextId++;
      setToasts((prev) => [...prev, { id, type, message }]);
      setTimeout(() => removeToast(id), 4000);
    },
    [removeToast],
  );

  return (
    <ToastContext.Provider value={{ toasts, addToast, removeToast }}>
      {children}
      {/* Toast container */}
      <div className="fixed bottom-4 right-4 z-50 flex flex-col gap-2">
        {toasts.map((toast) => (
          <div
            key={toast.id}
            className={`bg-mesh-surface border px-4 py-3 rounded-lg min-w-[300px] flex items-center gap-3 ${
              toast.type === 'error'
                ? 'border-[#ee0000]/30'
                : toast.type === 'success'
                  ? 'border-mesh-accent/30'
                  : 'border-mesh-border'
            }`}
          >
            <span
              className={`text-sm ${
                toast.type === 'error'
                  ? 'text-[#ee0000]'
                  : toast.type === 'success'
                    ? 'text-mesh-accent'
                    : 'text-mesh-muted'
              }`}
            >
              {toast.type === 'error' ? '\u2717' : toast.type === 'success' ? '\u2713' : '\u2139'}
            </span>
            <span className="text-sm text-mesh-text flex-1">{toast.message}</span>
            <button
              onClick={() => removeToast(toast.id)}
              className="text-mesh-muted hover:text-mesh-text text-sm"
            >
              {'\u2715'}
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast() {
  return useContext(ToastContext);
}
