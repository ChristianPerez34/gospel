import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { CanvasReviewerNode, CanvasToolNode } from "../hooks/useConstellation";
import { ConstellationCanvas } from "./ConstellationCanvas";

const reviewer: CanvasReviewerNode = {
  id: "Architecture",
  focus: "Architecture",
  name: "Architecture",
  status: "active",
  progress: 0.4,
  findings: 0,
  suppressed: 0,
  comments: [],
  provider: "anthropic",
  model: "claude-sonnet-4",
};

const fileRead: CanvasToolNode = {
  id: "review-tool:Architecture:detector:1:call-7",
  kind: "read",
  label: "Read",
  target: "src/review/mod.rs",
  status: "running",
  hasDiff: false,
  reviewerId: "Architecture",
};

describe("ConstellationCanvas", () => {
  afterEach(cleanup);

  it("connects reviewer file activity to its reviewer instead of the main agent", () => {
    const { container } = render(
      <ConstellationCanvas
        toolNodes={[fileRead]}
        reviewerNodes={[reviewer]}
        reviewActive
        agentRunning={false}
      />
    );

    expect(screen.getByText("mod.rs")).toBeDefined();
    expect(screen.getByTitle("Review model: anthropic/claude-sonnet-4")).toBeDefined();
    expect(
      container.querySelector(
        'path[data-edge-from="Architecture"][data-edge-to="review-tool:Architecture:detector:1:call-7"]'
      )
    ).not.toBeNull();
    expect(
      container.querySelector(
        'path[data-edge-from="agent"][data-edge-to="review-tool:Architecture:detector:1:call-7"]'
      )
    ).toBeNull();
  });
});
