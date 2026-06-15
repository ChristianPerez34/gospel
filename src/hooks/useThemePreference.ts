import { useCallback, useEffect, useMemo, useState } from "react";
import type { ResolvedTheme, ThemePreference } from "../types";

const STORAGE_KEY = "gospel.themePreference";
const THEME_VALUES: ThemePreference[] = ["dark", "light", "system"];

function isThemePreference(value: string | null): value is ThemePreference {
  return Boolean(value && THEME_VALUES.includes(value as ThemePreference));
}

function systemTheme(): ResolvedTheme {
  if (typeof window === "undefined" || !window.matchMedia) {
    return "dark";
  }
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function storedPreference(): ThemePreference {
  if (typeof window === "undefined") {
    return "dark";
  }

  try {
    const value = window.localStorage.getItem(STORAGE_KEY);
    return isThemePreference(value) ? value : "dark";
  } catch {
    return "dark";
  }
}

function persistPreference(value: ThemePreference) {
  if (typeof window === "undefined") return;

  try {
    window.localStorage.setItem(STORAGE_KEY, value);
  } catch {
    // Ignore localStorage write failures in restricted contexts.
  }
}

export function useThemePreference() {
  const [themePreference, setThemePreferenceState] = useState<ThemePreference>(storedPreference);
  const [systemResolvedTheme, setSystemResolvedTheme] = useState<ResolvedTheme>(systemTheme);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;

    const query = window.matchMedia("(prefers-color-scheme: light)");
    const handleChange = () => setSystemResolvedTheme(query.matches ? "light" : "dark");

    handleChange();
    query.addEventListener("change", handleChange);
    return () => query.removeEventListener("change", handleChange);
  }, []);

  const setThemePreference = useCallback((next: ThemePreference) => {
    setThemePreferenceState(next);
    persistPreference(next);
  }, []);

  const resolvedTheme = useMemo<ResolvedTheme>(
    () => (themePreference === "system" ? systemResolvedTheme : themePreference),
    [systemResolvedTheme, themePreference],
  );

  return {
    themePreference,
    resolvedTheme,
    setThemePreference,
  };
}
