import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { useModelPreferences } from "./useModelPreferences";

const STORAGE_KEY = "gospel.modelPreferences";

// happy-dom does not ship localStorage; stub a minimal in-memory version.
function stubLocalStorage() {
  const store = new Map<string, string>();
  Object.defineProperty(window, "localStorage", {
    value: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => {
        store.set(key, value);
      },
      removeItem: (key: string) => {
        store.delete(key);
      },
      clear: () => store.clear(),
    },
    writable: true,
    configurable: true,
  });
  return store;
}

describe("useModelPreferences", () => {
  let store: Map<string, string>;

  beforeEach(() => {
    store = stubLocalStorage();
  });

  it("starts with empty favorites and hidden sets", () => {
    const { result } = renderHook(() => useModelPreferences());

    expect(result.current.favorites.size).toBe(0);
    expect(result.current.hidden.size).toBe(0);
  });

  it("toggles a model in and out of favorites", () => {
    const { result } = renderHook(() => useModelPreferences());

    act(() => result.current.onToggleFavorite("openai::gpt-5.5"));
    expect(result.current.favorites.has("openai::gpt-5.5")).toBe(true);

    act(() => result.current.onToggleFavorite("openai::gpt-5.5"));
    expect(result.current.favorites.has("openai::gpt-5.5")).toBe(false);
  });

  it("toggles a model in and out of hidden", () => {
    const { result } = renderHook(() => useModelPreferences());

    act(() => result.current.onToggleHidden("openai::gpt-5.5"));
    expect(result.current.hidden.has("openai::gpt-5.5")).toBe(true);

    act(() => result.current.onToggleHidden("openai::gpt-5.5"));
    expect(result.current.hidden.has("openai::gpt-5.5")).toBe(false);
  });

  it("persists preferences to localStorage", () => {
    const { result } = renderHook(() => useModelPreferences());

    act(() => result.current.onToggleFavorite("anthropic::claude-sonnet-4"));
    act(() => result.current.onToggleHidden("openai::gpt-5.5"));

    const stored = JSON.parse(store.get(STORAGE_KEY) ?? "{}");
    expect(stored.favorites).toEqual(["anthropic::claude-sonnet-4"]);
    expect(stored.hidden).toEqual(["openai::gpt-5.5"]);
  });

  it("rehydrates preferences from localStorage", () => {
    store.set(
      STORAGE_KEY,
      JSON.stringify({ favorites: ["openai::gpt-5.5"], hidden: ["openai::gpt-4"] })
    );

    const { result } = renderHook(() => useModelPreferences());

    expect(result.current.favorites.has("openai::gpt-5.5")).toBe(true);
    expect(result.current.hidden.has("openai::gpt-4")).toBe(true);
  });

  it("ignores corrupt stored values", () => {
    store.set(STORAGE_KEY, "not-json");

    const { result } = renderHook(() => useModelPreferences());

    expect(result.current.favorites.size).toBe(0);
    expect(result.current.hidden.size).toBe(0);
  });
});
