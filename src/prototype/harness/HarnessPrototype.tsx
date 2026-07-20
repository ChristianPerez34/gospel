// PROTOTYPE — throwaway. Entry + floating variant switcher for the agentic
// coding harness UI exploration. Mounts one of three structurally different
// variants based on ?variant=A|B|C. Hidden in production builds.
import { useEffect, useState } from "react";
import { VariantA } from "./VariantA";
import { VariantB } from "./VariantB";
import { VariantC } from "./VariantC";
import { VariantD } from "./VariantD";

const VARIANTS = [
  { key: "A", name: "Terminal Workbench" },
  { key: "B", name: "Review Theater" },
  { key: "C", name: "Constellation" },
  { key: "D", name: "Workbench Constellation" },
] as const;

type VariantKey = (typeof VARIANTS)[number]["key"];

function readVariant(): VariantKey {
  const params = new URLSearchParams(window.location.search);
  const v = params.get("variant")?.toUpperCase();
  if (v === "A" || v === "B" || v === "C" || v === "D") return v;
  return "D";
}

export function HarnessPrototype() {
  const [variant, setVariant] = useState<VariantKey>(readVariant);

  useEffect(() => {
    const onPop = () => setVariant(readVariant());
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  const cycle = (dir: 1 | -1) => {
    const idx = VARIANTS.findIndex((v) => v.key === variant);
    const next = VARIANTS[(idx + dir + VARIANTS.length) % VARIANTS.length];
    const params = new URLSearchParams(window.location.search);
    params.set("variant", next.key);
    const url = `${window.location.pathname}?${params.toString()}`;
    window.history.replaceState(null, "", url);
    setVariant(next.key);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable))
        return;
      if (e.key === "ArrowLeft") cycle(-1);
      if (e.key === "ArrowRight") cycle(1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [variant]);

  const current = VARIANTS.find((v) => v.key === variant)!;

  return (
    <>
      {variant === "A" && <VariantA />}
      {variant === "B" && <VariantB />}
      {variant === "C" && <VariantC />}
      {variant === "D" && <VariantD />}
      <Switcher current={current} onPrev={() => cycle(-1)} onNext={() => cycle(1)} />
    </>
  );
}

function Switcher({
  current,
  onPrev,
  onNext,
}: {
  current: { key: string; name: string };
  onPrev: () => void;
  onNext: () => void;
}) {
  return (
    <div style={bar}>
      <button type="button" style={arrow} onClick={onPrev} aria-label="previous variant">
        ←
      </button>
      <div style={label}>
        <span style={labelKey}>{current.key}</span>
        <span style={labelName}>{current.name}</span>
      </div>
      <button type="button" style={arrow} onClick={onNext} aria-label="next variant">
        →
      </button>
    </div>
  );
}

const bar: React.CSSProperties = {
  position: "fixed",
  bottom: 16,
  left: "50%",
  transform: "translateX(-50%)",
  display: "flex",
  alignItems: "center",
  gap: 4,
  padding: "6px 8px",
  background: "var(--surface-overlay)",
  border: "1px solid var(--surface-line)",
  borderRadius: "var(--radius-full)",
  boxShadow: "var(--shadow-floating)",
  zIndex: 1000,
  fontFamily: "var(--font-mono)",
};
const arrow: React.CSSProperties = {
  background: "transparent",
  border: "none",
  color: "var(--text-secondary)",
  fontSize: 14,
  cursor: "pointer",
  width: 28,
  height: 28,
  borderRadius: "50%",
  display: "grid",
  placeItems: "center",
};
const label: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 8px",
};
const labelKey: React.CSSProperties = {
  fontSize: 10,
  textTransform: "uppercase",
  letterSpacing: "0.14em",
  color: "var(--accent-action)",
  border: "1px solid var(--accent-action)",
  borderRadius: "var(--radius-full)",
  padding: "2px 8px",
};
const labelName: React.CSSProperties = { fontSize: 12, color: "var(--text-primary)" };
