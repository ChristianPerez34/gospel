import { openUrl } from "@tauri-apps/plugin-opener";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { MessageBlock } from "./MessageBlock";

afterEach(() => {
  cleanup();
});

describe("MessageBlock external links", () => {
  it("opens confirmed links through Tauri's opener", async () => {
    render(
      <MessageBlock
        message={{
          id: "standalone",
          role: "agent",
          content: "[Read docs](https://example.com/docs)",
          timestamp: new Date(),
        }}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "Read docs" }));
    expect(openUrl).not.toHaveBeenCalled();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Open link" })));
    expect(openUrl).toHaveBeenCalledWith("https://example.com/docs");
  });
});
