import { useState, useEffect, useCallback } from "react";
import "./Toast.css";

export interface ToastData {
  id: string;
  type: "error" | "success" | "info";
  message: string;
  action?: {
    label: string;
    onClick: () => void;
  };
  secondaryAction?: {
    label: string;
    onClick: () => void;
  };
  autoDismissMs?: number;
}

interface ToastProps {
  toast: ToastData;
  onDismiss: (id: string) => void;
}

export function Toast({ toast, onDismiss }: ToastProps) {
  useEffect(() => {
    const shouldAutoDismiss = toast.autoDismissMs && toast.autoDismissMs > 0 && !toast.action && !toast.secondaryAction;
    if (shouldAutoDismiss) {
      const timer = setTimeout(() => onDismiss(toast.id), toast.autoDismissMs);
      return () => clearTimeout(timer);
    }
  }, [toast, onDismiss]);

  const handleDismiss = useCallback(() => {
    onDismiss(toast.id);
  }, [toast.id, onDismiss]);

  return (
    <div
      className={`toast toast--${toast.type}`}
      onClick={handleDismiss}
      role="alert"
    >
      <div className="toast__icon">
        {toast.type === "error" && (
          <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
            <circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" strokeWidth="1.2" />
            <line x1="7" y1="4" x2="7" y2="7.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            <circle cx="7" cy="10" r="0.6" />
          </svg>
        )}
        {toast.type === "success" && (
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
            <path d="M3 7L6 10L11 4" />
          </svg>
        )}
        {toast.type === "info" && (
          <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
            <circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" strokeWidth="1.2" />
            <circle cx="7" cy="4.5" r="0.6" />
            <line x1="7" y1="6.5" x2="7" y2="10" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
        )}
      </div>
      <span className="toast__message">{toast.message}</span>
      <div className="toast__actions">
        {toast.action && (
          <button
            className="toast__action-btn"
            onClick={(e) => { e.stopPropagation(); toast.action!.onClick(); }}
          >
            {toast.action.label}
          </button>
        )}
        {toast.secondaryAction && (
          <button
            className="toast__action-btn toast__action-btn--secondary"
            onClick={(e) => { e.stopPropagation(); toast.secondaryAction!.onClick(); }}
          >
            {toast.secondaryAction.label}
          </button>
        )}
      </div>
    </div>
  );
}

interface ToastContainerProps {
  toasts: ToastData[];
  onDismiss: (id: string) => void;
}

export function ToastContainer({ toasts, onDismiss }: ToastContainerProps) {
  if (toasts.length === 0) return null;

  return (
    <div className="toast-container">
      {toasts.map((toast) => (
        <Toast key={toast.id} toast={toast} onDismiss={onDismiss} />
      ))}
    </div>
  );
}

let toastIdCounter = 0;
const SESSION_ID = Math.random().toString(36).slice(2, 9);

export function useToasts() {
  const [toasts, setToasts] = useState<ToastData[]>([]);

  const addToast = useCallback((data: Omit<ToastData, "id">) => {
    const id = `toast-${SESSION_ID}-${++toastIdCounter}`;
    setToasts((prev) => [...prev, { ...data, id }]);
  }, []);

  const dismissToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const showError = useCallback((message: string, actions?: ToastData["action"] | { primary: ToastData["action"]; secondary: ToastData["action"] }) => {
    const toastData: Omit<ToastData, "id"> = {
      type: "error",
      message,
      autoDismissMs: 5000,
    };
    if (actions && "primary" in actions) {
      toastData.action = actions.primary;
      toastData.secondaryAction = actions.secondary;
    } else if (actions) {
      toastData.action = actions;
    }
    addToast(toastData);
  }, [addToast]);

  const showSuccess = useCallback((message: string) => {
    addToast({ type: "success", message, autoDismissMs: 3000 });
  }, [addToast]);

  const showInfo = useCallback((message: string) => {
    addToast({ type: "info", message, autoDismissMs: 4000 });
  }, [addToast]);

  return { toasts, dismissToast, addToast, showError, showSuccess, showInfo };
}
