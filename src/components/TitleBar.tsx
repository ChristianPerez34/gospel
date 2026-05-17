import "./TitleBar.css";

export function TitleBar() {
  return (
    <div className="titlebar" data-tauri-drag-region>
      <span className="titlebar__name" data-tauri-drag-region>
        Gospel
      </span>
      <div className="titlebar__controls">
        <button
          className="titlebar__control titlebar__control--minimize"
          aria-label="Minimize"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
            <rect
              x="2"
              y="5.5"
              width="8"
              height="1"
              fill="currentColor"
            />
          </svg>
        </button>
        <button
          className="titlebar__control titlebar__control--maximize"
          aria-label="Maximize"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
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
          className="titlebar__control titlebar__control--close"
          aria-label="Close"
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
            <path
              d="M3 3L9 9M9 3L3 9"
              stroke="currentColor"
              strokeWidth="1.2"
            />
          </svg>
        </button>
      </div>
    </div>
  );
}