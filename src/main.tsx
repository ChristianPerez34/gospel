import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { WorkspacesProvider } from "./hooks/useWorkspaces";
import { installClipboardPolyfill } from "./lib/clipboard";

// Streamdown's code-block copy button calls navigator.clipboard.writeText
// directly with no fallback, which fails in the Tauri WebView. Patch it
// once at startup so code blocks (and every other caller) copy reliably.
installClipboardPolyfill();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <WorkspacesProvider>
      <App />
    </WorkspacesProvider>
  </React.StrictMode>
);
