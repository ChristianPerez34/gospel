import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { X } from "lucide-react";
import { type RefObject, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { LinkSafetyConfig, LinkSafetyModalProps } from "streamdown";
import { Button } from "@/components/ui/button";
import { useFocusTrap } from "@/hooks/useFocusTrap";

const ALLOWED_PROTOCOLS = new Set(["http:", "https:", "mailto:", "tel:"]);

function ExternalLinkDialogContent({
  dialogRef,
  onClose,
  url,
}: {
  dialogRef: RefObject<HTMLDivElement>;
  onClose: () => void;
  url: string;
}) {
  const activeRef = useRef(true);
  const titleId = useId();
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [opening, setOpening] = useState(false);

  useEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
    };
  }, []);

  const close = () => {
    activeRef.current = false;
    onClose();
  };

  const open = async () => {
    if (opening) return;
    setError("");
    setOpening(true);
    try {
      const { protocol } = new URL(url);
      if (!ALLOWED_PROTOCOLS.has(protocol)) {
        throw new Error("Unsupported link protocol");
      }
      if (isTauri()) {
        await openUrl(url);
      } else {
        window.open(url, "_blank", "noreferrer");
      }
      if (activeRef.current) {
        close();
      }
    } catch {
      if (activeRef.current) {
        setError("Unable to open link. You can copy it instead.");
      }
    } finally {
      if (activeRef.current) {
        setOpening(false);
      }
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(url);
      if (activeRef.current) {
        setCopied(true);
        setError("");
      }
    } catch {
      if (activeRef.current) {
        setError("Unable to copy link.");
      }
    }
  };

  return createPortal(
    <div className="fixed inset-0 z-[var(--z-dialog)] flex items-center justify-center bg-scrim">
      <button
        type="button"
        aria-label="Close link dialog"
        tabIndex={-1}
        className="absolute inset-0 cursor-default"
        onClick={close}
      />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className="relative mx-4 flex w-full max-w-md flex-col gap-4 rounded-xl border border-[var(--surface-line)] bg-surface-elevated p-6 text-text-primary shadow-[var(--shadow-dialog)]"
      >
        <Button
          variant="ghost"
          size="icon-sm"
          className="absolute right-4 top-4"
          aria-label="Close"
          onClick={close}
        >
          <X />
        </Button>
        <h2 id={titleId} className="pr-8 font-semibold text-lg text-text-primary">
          Open external link?
        </h2>
        <p className="text-sm text-text-secondary">You're about to visit an external website.</p>
        <div className="max-h-32 overflow-y-auto break-all rounded-md border border-[var(--surface-line)] bg-surface-overlay p-3 font-mono text-sm text-text-primary">
          {url}
        </div>
        {error && (
          <p role="alert" className="text-sm text-status-error">
            {error}
          </p>
        )}
        <div className="flex gap-2">
          <Button variant="outline" className="flex-1" onClick={copy}>
            {copied ? "Copied" : "Copy link"}
          </Button>
          <Button className="flex-1" onClick={open} disabled={opening}>
            Open link
          </Button>
        </div>
      </div>
    </div>,
    document.body
  );
}

function ExternalLinkDialog({ isOpen, onClose, url }: LinkSafetyModalProps) {
  const dialogRef = useRef<HTMLDivElement>(null);

  useFocusTrap({ active: isOpen, containerRef: dialogRef, onEscape: onClose });

  if (!isOpen) return null;

  return <ExternalLinkDialogContent key={url} dialogRef={dialogRef} onClose={onClose} url={url} />;
}

export const linkSafety: LinkSafetyConfig = {
  enabled: true,
  renderModal: (props) => <ExternalLinkDialog {...props} />,
};
