import { useCallback, useState } from 'react';
import type { ToastItem, ToastTone } from '../types/cockpit';

export function useToastQueue() {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  const dismissToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const pushToast = useCallback(
    (tone: ToastTone, title: string, detail?: string) => {
      const id = `toast-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
      setToasts((prev) => [...prev.slice(-3), { id, tone, title, detail }]);
      setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id));
      }, 3400);
    },
    []
  );

  return { toasts, pushToast, dismissToast };
}
