import "@fontsource-variable/geist";
import { AppShell } from "./components/AppShell";
import { PlanPanel } from "./components/PlanPanel";
import { useWorkspaces } from "./hooks/useWorkspaces";
import { HarnessPrototype } from "./prototype/harness/HarnessPrototype";
import "./styles/global.css";

function isHarnessPrototypeRequest(): boolean {
  if (import.meta.env.PROD) return false;
  return new URLSearchParams(window.location.search).get("prototype") === "harness";
}

function isPlanPanelRequest(): boolean {
  if (import.meta.env.PROD) return false;
  return new URLSearchParams(window.location.search).get("panel") === "plan";
}

function App() {
  if (isHarnessPrototypeRequest()) {
    return <HarnessPrototype />;
  }
  const showPlanPanel = isPlanPanelRequest();
  return (
    <>
      <AppShell />
      {showPlanPanel && <PlanPanelOverlay />}
    </>
  );
}

function PlanPanelOverlay() {
  const { activeWorkspace } = useWorkspaces();
  return <PlanPanel workspacePath={activeWorkspace?.path ?? ""} />;
}

export default App;
