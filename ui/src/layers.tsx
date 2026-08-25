// The shell's two floating layers (design §07): a bottom-right toast stack
// (success | info | error, auto-clears after 3.4s) and a dialog layer any
// surface can push into. Escape closes whichever dialog is open.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

export type ToastTone = "success" | "info" | "error";

interface Toast {
  id: number;
  tone: ToastTone;
  text: string;
}

interface Layers {
  toast: (tone: ToastTone, text: string) => void;
  openDialog: (node: ReactNode) => void;
  closeDialog: () => void;
}

const LayersContext = createContext<Layers | null>(null);

export function useLayers(): Layers {
  const ctx = useContext(LayersContext);
  if (!ctx) throw new Error("useLayers outside LayersProvider");
  return ctx;
}

const TOAST_MS = 3400;

export function LayersProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [dialog, setDialog] = useState<ReactNode | null>(null);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => {
    setToasts((ts) => ts.filter((t) => t.id !== id));
  }, []);

  const toast = useCallback(
    (tone: ToastTone, text: string) => {
      const id = nextId.current++;
      setToasts((ts) => [...ts, { id, tone, text }]);
      window.setTimeout(() => dismiss(id), TOAST_MS);
    },
    [dismiss],
  );

  const openDialog = useCallback((node: ReactNode) => setDialog(node), []);
  const closeDialog = useCallback(() => setDialog(null), []);

  useEffect(() => {
    if (!dialog) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeDialog();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dialog, closeDialog]);

  const value = useMemo(
    () => ({ toast, openDialog, closeDialog }),
    [toast, openDialog, closeDialog],
  );

  return (
    <LayersContext.Provider value={value}>
      {children}
      {dialog && (
        <div
          className="dialog-backdrop"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) closeDialog();
          }}
        >
          <div className="dialog" role="dialog">
            {dialog}
          </div>
        </div>
      )}
      <div className="toast-layer">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.tone}`}>
            <span>{t.text}</span>
            <button className="close" onClick={() => dismiss(t.id)} aria-label="Dismiss">
              ×
            </button>
          </div>
        ))}
      </div>
    </LayersContext.Provider>
  );
}
