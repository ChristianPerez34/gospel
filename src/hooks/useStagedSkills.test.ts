import { invoke } from "@tauri-apps/api/core";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useStagedSkills } from "./useStagedSkills";

describe("useStagedSkills", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("loads staged skills for the active workspace", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_staged_skills") {
        return [{ name: "tdd", staged: true, has_rejected: false }];
      }
      return undefined;
    });

    const { result } = renderHook(() => useStagedSkills("/tmp/gospel"));

    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });
    expect(result.current.stagedSkills).toEqual([
      { name: "tdd", staged: true, has_rejected: false },
    ]);
    expect(invoke).toHaveBeenCalledWith("list_staged_skills");
  });

  it("adopts a staged skill and reloads the list", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_staged_skills") {
        return [{ name: "tdd", staged: true, has_rejected: false }];
      }
      if (cmd === "adopt_staged_skill") {
        return {
          name: "tdd",
          description: "Test-driven development",
          source: "Workspace",
          scripts: [],
          user_invocable: true,
        };
      }
      return undefined;
    });

    const { result } = renderHook(() => useStagedSkills("/tmp/gospel"));
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.adopt("tdd");
    });

    expect(invoke).toHaveBeenCalledWith("adopt_staged_skill", { skillName: "tdd" });
    expect(invoke).toHaveBeenCalledWith("list_staged_skills");
  });

  it("optimizes a named skill then reloads staged results", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_staged_skills") {
        return [{ name: "tdd", staged: true, has_rejected: false }];
      }
      if (cmd === "optimize_skill_from_sessions") {
        return { verdict: "accept", staged_path: ".gospel/skill-opt/tdd/SKILL.md" };
      }
      return undefined;
    });

    const { result } = renderHook(() => useStagedSkills("/tmp/gospel"));
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.optimize("tdd", { provider: "openai", model: "gpt-5.5" });
    });

    expect(invoke).toHaveBeenCalledWith("optimize_skill_from_sessions", {
      skillName: "tdd",
      provider: "openai",
      model: "gpt-5.5",
      variant: null,
    });
    expect(invoke).toHaveBeenCalledWith("list_staged_skills");
  });

  it("marks optimize as in flight until the command settles", async () => {
    let resolveOptimize: ((value: unknown) => void) | undefined;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_staged_skills") {
        return [{ name: "tdd", staged: true, has_rejected: false }];
      }
      if (cmd === "optimize_skill_from_sessions") {
        return new Promise((resolve) => {
          resolveOptimize = resolve;
        });
      }
      return undefined;
    });

    const { result } = renderHook(() => useStagedSkills("/tmp/gospel"));
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    let pending: Promise<void> | undefined;
    act(() => {
      pending = result.current.optimize("tdd", { provider: "openai", model: "gpt-5.5" });
    });
    await waitFor(() => {
      expect(result.current.optimizing).toBe(true);
    });

    await act(async () => {
      resolveOptimize?.({ verdict: "accept" });
      await pending;
    });
    expect(result.current.optimizing).toBe(false);
  });

  it("surfaces optimizer errors without clearing staged skills", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_staged_skills") {
        return [{ name: "tdd", staged: true, has_rejected: false }];
      }
      if (cmd === "optimize_skill_from_sessions") {
        throw new Error("selection split is empty");
      }
      return undefined;
    });

    const { result } = renderHook(() => useStagedSkills("/tmp/gospel"));
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await expect(
        result.current.optimize("tdd", { provider: "openai", model: "gpt-5.5" }),
      ).rejects.toThrow();
    });

    expect(result.current.error).toContain("selection split is empty");
    expect(result.current.stagedSkills).toEqual([
      { name: "tdd", staged: true, has_rejected: false },
    ]);
  });

  it("previews harvest train and selection counts for a skill", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "list_staged_skills") return [];
      if (cmd === "harvest_skill_optimization_tasks") {
        return {
          skill: "tdd",
          train: [{ prompt: "a" }, { prompt: "b" }],
          selection: [{ prompt: "c" }],
        };
      }
      return undefined;
    });

    const { result } = renderHook(() => useStagedSkills("/tmp/gospel"));
    await waitFor(() => {
      expect(result.current.loading).toBe(false);
    });

    await act(async () => {
      await result.current.previewHarvest("tdd");
    });

    expect(invoke).toHaveBeenCalledWith("harvest_skill_optimization_tasks", {
      skillName: "tdd",
    });
    expect(result.current.harvest).toEqual({
      skill: "tdd",
      trainCount: 2,
      selectionCount: 1,
    });
  });
});
