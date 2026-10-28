import { useCallback, useMemo, useState } from "react";

const STORAGE_KEY = "gospel.modelPreferences";

interface StoredModelPreferences {
  favorites: string[];
  hidden: string[];
}

export interface ModelPreferences {
  favorites: ReadonlySet<string>;
  hidden: ReadonlySet<string>;
  onToggleFavorite: (modelId: string) => void;
  onToggleHidden: (modelId: string) => void;
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string");
}

function storedPreferences(): StoredModelPreferences {
  const empty = { favorites: [], hidden: [] };
  if (typeof window === "undefined") {
    return empty;
  }

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return empty;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return empty;
    const { favorites, hidden } = parsed as Partial<StoredModelPreferences>;
    return {
      favorites: isStringArray(favorites) ? favorites : [],
      hidden: isStringArray(hidden) ? hidden : [],
    };
  } catch {
    return empty;
  }
}

function persistPreferences(value: StoredModelPreferences) {
  if (typeof window === "undefined") return;

  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
  } catch {
    // Ignore localStorage write failures in restricted contexts.
  }
}

function toggleId(ids: string[], id: string): string[] {
  return ids.includes(id) ? ids.filter((existing) => existing !== id) : [...ids, id];
}

export function useModelPreferences(): ModelPreferences {
  const [stored, setStored] = useState<StoredModelPreferences>(storedPreferences);

  const onToggleFavorite = useCallback((modelId: string) => {
    setStored((current) => {
      const next = { ...current, favorites: toggleId(current.favorites, modelId) };
      persistPreferences(next);
      return next;
    });
  }, []);

  const onToggleHidden = useCallback((modelId: string) => {
    setStored((current) => {
      const next = { ...current, hidden: toggleId(current.hidden, modelId) };
      persistPreferences(next);
      return next;
    });
  }, []);

  const favorites = useMemo(() => new Set(stored.favorites), [stored.favorites]);
  const hidden = useMemo(() => new Set(stored.hidden), [stored.hidden]);

  return useMemo(
    () => ({ favorites, hidden, onToggleFavorite, onToggleHidden }),
    [favorites, hidden, onToggleFavorite, onToggleHidden]
  );
}
