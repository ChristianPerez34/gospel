import { vi, beforeEach, afterEach } from "vitest";

// Shim vi.mocked for environments where it is missing
if (typeof vi !== "undefined" && !vi.mocked) {
  (vi as any).mocked = <T>(fn: T): any => fn;
}

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@tauri-apps/api/path", () => ({
  normalize: vi.fn(async (path: string) => path),
  resolve: vi.fn(async (...parts: string[]) => parts.join("/").replace(/\/+/g, "/")),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn().mockResolvedValue(undefined),
}));

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  vi.restoreAllMocks();
});
