import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { X } from "lucide-react";
import { useCallback, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { LinkSafetyConfig, LinkSafetyModalProps } from "streamdown";
import { Button } from "@/components/ui/button";
import { useFocusTrap } from "@/hooks/useFocusTrap";

function ExternalLinkDialog({ isOpen, onClose, url }: LinkSafetyModalProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [opening, setOpening] = useState(false);
  const close = useCallback(() => {
    setError("");
    setCopied(false);
    onClose();
  }, [onClose]);

  useFocusTrap({ active: isOpen, containerRef: dialogRef, onEscape: close });

  if (!isOpen) return null;

  const open = async () => {
    setError("");
    setOpening(true);
    try {
      const { protocol } = new URL(url);
      if (!["http:", "https:", "mailto:", "tel:"].includes(protocol)) {
        throw new Error("Unsupported link protocol");
      }
      if (isTauri()) {
        await openUrl(url);
      } else {
        window.open(url, "_blank", "noreferrer");
      }
      close();
    } catch {
      setError("Unable to open link. You can copy it instead.");
    } finally {
      setOpening(false);
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      setError("");
    } catch {
      setError("Unable to copy link.");
    }
  };

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/50 backdrop-blur-sm">
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
        className="relative mx-4 flex w-full max-w-md flex-col gap-4 rounded-xl border bg-background p-6 shadow-lg"
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
        <h2 id={titleId} className="pr-8 font-semibold text-lg">
          Open external link?
        </h2>
        <p className="text-muted-foreground text-sm">You're about to visit an external website.</p>
        <div className="max-h-32 overflow-y-auto break-all rounded-md bg-muted p-3 font-mono text-sm">
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

export const linkSafety: LinkSafetyConfig = {
  enabled: true,
  renderModal: (props) => <ExternalLinkDialog {...props} />,
};
