import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { WorkspacesProvider } from "./hooks/useWorkspaces";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <WorkspacesProvider>
      <App />
    </WorkspacesProvider>
  </React.StrictMode>
);
