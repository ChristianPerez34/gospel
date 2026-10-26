import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Workspace } from "../types";
import { WorkspacePicker } from "./WorkspacePicker";

const workspaces: Workspace[] = [
  { id: "alpha", name: "Gospel", path: "/projects/gospel", sessionCount: 0 },
  { id: "beta", name: "Gospel", path: "/archive/gospel", sessionCount: 2 },
];
afterEach(cleanup);
function Picker({
  items = workspaces,
  onAdd = vi.fn(),
}: {
  items?: Workspace[];
  onAdd?: () => void;
}) {
  const [selected, setSelected] = useState<string | undefined>();
  return (
    <WorkspacePicker workspaces={items} value={selected} onChange={setSelected} onAdd={onAdd} />
  );
}

describe("WorkspacePicker", () => {
  it("opens on click, searches by directory, and selects a workspace", async () => {
    render(<Picker />);
    fireEvent.click(screen.getByRole("combobox", { name: "Workspace" }));
    const search = await screen.findByRole("combobox", { name: "Search workspaces" });
    expect(await screen.findAllByRole("option")).toHaveLength(2);
    fireEvent.change(search, { target: { value: "archive" } });
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(1));
    fireEvent.click(screen.getByRole("option", { name: /archive/ }));
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
    expect(screen.getByRole("combobox", { name: "Workspace" }).textContent).toContain(
      "/archive/gospel"
    );
  });

  it("offers adding a workspace even when the list is empty or search has no matches", async () => {
    const onAdd = vi.fn();
    const { rerender } = render(<Picker onAdd={onAdd} />);
    fireEvent.click(screen.getByRole("combobox", { name: "Workspace" }));
    fireEvent.change(await screen.findByRole("combobox", { name: "Search workspaces" }), {
      target: { value: "missing" },
    });
    expect(await screen.findByText("No workspaces found.")).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Add workspace" }));
    expect(onAdd).toHaveBeenCalledOnce();
    rerender(<Picker items={[]} onAdd={onAdd} />);
    fireEvent.click(screen.getByRole("combobox", { name: "Workspace" }));
    expect(await screen.findByText("Add a project folder to get started.")).toBeDefined();
    expect(screen.getByRole("button", { name: "Add workspace" })).toBeDefined();
  });
});
