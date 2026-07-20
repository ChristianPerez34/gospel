import { AppShell } from "./components/AppShell";
import { HarnessPrototype } from "./prototype/harness/HarnessPrototype";
import "./styles/global.css";

function isHarnessPrototypeRequest(): boolean {
  if (import.meta.env.PROD) return false;
  return new URLSearchParams(window.location.search).get("prototype") === "harness";
}

function App() {
  if (isHarnessPrototypeRequest()) {
    return <HarnessPrototype />;
  }
  return <AppShell />;
}

export default App;
