import { useState, useEffect, useCallback } from "react";
import { Button } from "@/components/ui/button";

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

const TYPE_STYLES = {
  error: "border-status-error",
  success: "border-status-success",
  info: "border-accent-structure",
};

const ICON_STYLES = {
  error: "text-status-error",
  success: "text-status-success",
  info: "text-accent-structure",
};

export function Toast({ toast, onDismiss }: ToastProps) {
  const hasAction = Boolean(toast.action);
  const hasSecondaryAction = Boolean(toast.secondaryAction);

  useEffect(() => {
    const shouldAutoDismiss =
      toast.autoDismissMs && toast.autoDismissMs > 0 && !hasAction && !hasSecondaryAction;
    if (shouldAutoDismiss) {
      const timer = setTimeout(() => onDismiss(toast.id), toast.autoDismissMs);
      return () => clearTimeout(timer);
    }
  }, [toast.id, toast.autoDismissMs, hasAction, hasSecondaryAction, onDismiss]);

  const handleDismiss = useCallback(() => {
    onDismiss(toast.id);
  }, [toast.id, onDismiss]);

  return (
    <div
      className={`flex items-center gap-2.5 py-2.5 px-3.5 bg-surface-elevated border rounded-md shadow-[var(--shadow-floating)] cursor-pointer pointer-events-auto max-w-[380px] animate-toast-in transition-opacity duration-200 hover:border-text-muted ${TYPE_STYLES[toast.type]}`}
      onClick={handleDismiss}
      role="alert"
    >
      <div className={`shrink-0 flex items-center ${ICON_STYLES[toast.type]}`}>
        {toast.type === "error" && (
          <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
            <circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" strokeWidth="1.2" />
            <line
              x1="7"
              y1="4"
              x2="7"
              y2="7.5"
              stroke="currentColor"
              strokeWidth="1.2"
              strokeLinecap="round"
            />
            <circle cx="7" cy="10" r="0.6" />
          </svg>
        )}
        {toast.type === "success" && (
          <svg
            width="14"
            height="14"
            viewBox="0 0 14 14"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          >
            <path d="M3 7L6 10L11 4" />
          </svg>
        )}
        {toast.type === "info" && (
          <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor">
            <circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" strokeWidth="1.2" />
            <circle cx="7" cy="4.5" r="0.6" />
            <line
              x1="7"
              y1="6.5"
              x2="7"
              y2="10"
              stroke="currentColor"
              strokeWidth="1.2"
              strokeLinecap="round"
            />
          </svg>
        )}
      </div>
      <span className="flex-1 text-[13px] text-text-secondary leading-snug">{toast.message}</span>
      <div className="flex gap-1.5 shrink-0">
        {toast.action && (
          <Button
            variant="secondary"
            size="xs"
            onClick={(e) => {
              e.stopPropagation();
              toast.action!.onClick();
            }}
          >
            {toast.action.label}
          </Button>
        )}
        {toast.secondaryAction && (
          <Button
            variant="outline"
            size="xs"
            onClick={(e) => {
              e.stopPropagation();
              toast.secondaryAction!.onClick();
            }}
          >
            {toast.secondaryAction.label}
          </Button>
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
    <div className="fixed bottom-20 right-4 z-[--z-toast] flex flex-col gap-2 pointer-events-none">
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

  const showError = useCallback(
    (
      message: string,
      actions?:
        | ToastData["action"]
        | { primary: ToastData["action"]; secondary: ToastData["action"] }
    ) => {
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
    },
    [addToast]
  );

  const showSuccess = useCallback(
    (message: string) => {
      addToast({ type: "success", message, autoDismissMs: 3000 });
    },
    [addToast]
  );

  const showInfo = useCallback(
    (message: string) => {
      addToast({ type: "info", message, autoDismissMs: 4000 });
    },
    [addToast]
  );

  return { toasts, dismissToast, addToast, showError, showSuccess, showInfo };
}
