export function TitleBar() {
  return (
    <div
      className="h-[var(--titlebar-height)] flex items-center justify-between bg-surface-elevated border-b border-surface-overlay px-3 select-none"
      data-tauri-drag-region
      // @ts-expect-error WebkitAppRegion is a Tauri-specific vendor property
      style={{ WebkitAppRegion: "drag" }}
    >
      <span
        className="font-body text-caption font-medium tracking-[0.02em] text-text-muted"
        data-tauri-drag-region
      >
        Gospel
      </span>
      <div
        className="flex items-center gap-1"
        // @ts-expect-error WebkitAppRegion is a Tauri-specific vendor property
        style={{ WebkitAppRegion: "no-drag" }}
      >
        <button
          type="button"
          className="w-7 h-7 flex items-center justify-center rounded-sm text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary"
          aria-label="Minimize"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
            <rect x="2" y="5.5" width="8" height="1" fill="currentColor" />
          </svg>
        </button>
        <button
          type="button"
          className="w-7 h-7 flex items-center justify-center rounded-sm text-text-muted transition-colors duration-150 ease-out-quart hover:bg-surface-overlay hover:text-text-secondary"
          aria-label="Maximize"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
            <rect
              x="2.5"
              y="2.5"
              width="7"
              height="7"
              stroke="currentColor"
              strokeWidth="1"
              fill="none"
            />
          </svg>
        </button>
        <button
          type="button"
          className="w-7 h-7 flex items-center justify-center rounded-sm text-text-muted transition-colors duration-150 ease-out-quart hover:bg-status-error hover:text-text-inverse"
          aria-label="Close"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
            <path d="M3 3L9 9M9 3L3 9" stroke="currentColor" strokeWidth="1.2" />
          </svg>
        </button>
      </div>
    </div>
  );
}
