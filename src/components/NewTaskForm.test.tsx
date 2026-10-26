import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NewTaskForm } from "./NewTaskForm";

const props = {
  workspaces: [
    { id: "alpha", name: "Alpha", path: "/projects/alpha", sessionCount: 0 },
    { id: "beta", name: "Beta", path: "/projects/beta", sessionCount: 0 },
  ],
  activeWorkspaceId: "alpha",
  models: [{ id: "openai::test", name: "Test model", provider: "openai", model: "test" }],
  selectedModelId: "openai::test",
  selectedVariant: null,
  onModelChange: vi.fn(),
  onVariantChange: vi.fn(),
  onStart: vi.fn(),
  onCancel: vi.fn(),
  onManageWorkspaces: vi.fn(),
  onOpenSettings: vi.fn(),
  creating: false,
  error: null,
};
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("NewTaskForm", () => {
  it("makes the execution workspace explicit and requires a name and instruction", () => {
    render(<NewTaskForm {...props} />);
    const start = screen.getByRole("button", {
      name: "Create and start task",
    }) as HTMLButtonElement;
    expect(screen.getByText("/projects/alpha")).toBeDefined();
    expect(start.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Workspace"), { target: { value: "beta" } });
    expect(screen.getByText("/projects/beta")).toBeDefined();
    expect(props.onStart).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Task name"), { target: { value: " Billing " } });
    expect(start.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Instruction"), {
      target: { value: " Build the billing page without changing checkout. " },
    });
    fireEvent.click(start);
    expect(props.onStart).toHaveBeenCalledWith(
      "Billing",
      "beta",
      "Build the billing page without changing checkout."
    );
  });

  it("keeps the brief after failure and blocks creation without a valid workspace or model", () => {
    const { rerender } = render(<NewTaskForm {...props} />);
    fireEvent.change(screen.getByLabelText("Task name"), { target: { value: "My task" } });
    fireEvent.change(screen.getByLabelText("Instruction"), { target: { value: "Do the work" } });
    rerender(<NewTaskForm {...props} error="Storage unavailable" />);
    expect(screen.getByRole("alert").textContent).toBe("Storage unavailable");
    expect((screen.getByLabelText("Instruction") as HTMLTextAreaElement).value).toBe("Do the work");
    rerender(<NewTaskForm {...props} workspaces={[]} models={[]} />);
    expect(
      (screen.getByRole("button", { name: "Create and start task" }) as HTMLButtonElement).disabled
    ).toBe(true);
    expect(screen.getByRole("button", { name: "Add a workspace" })).toBeDefined();
    expect(screen.getByRole("button", { name: "Model settings" })).toBeDefined();
  });
});
