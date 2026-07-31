import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { CanvasReviewerNode, CanvasToolNode } from "../hooks/useConstellation";
import { ConstellationCanvas } from "./ConstellationCanvas";
import {
  type CanvasPoint,
  layoutReviewerActivityPosition,
  layoutReviewerPositions,
  REVIEWER_ACTIVITY_BOUNDS,
  REVIEWER_NODE_BOUNDS,
  REVIEWER_NODE_GAP,
} from "./constellationLayout";

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

  it("keeps spawned reviewer nodes separated on the canvas", () => {
    const layouts = [
      layoutReviewerPositions(5, { w: 1000, h: 600 }, { x: 500, y: 290 }),
      layoutReviewerPositions(5, { w: 1000, h: 400 }, { x: 500, y: 190 }),
    ];

    for (const positions of layouts) {
      expect(positions).toHaveLength(5);
      for (let i = 0; i < positions.length; i += 1) {
        for (let j = i + 1; j < positions.length; j += 1) {
          const horizontalClearance = Math.abs(positions[i].x - positions[j].x);
          const verticalClearance = Math.abs(positions[i].y - positions[j].y);
          expect(
            horizontalClearance >= REVIEWER_NODE_BOUNDS.width + REVIEWER_NODE_GAP ||
              verticalClearance >= REVIEWER_NODE_BOUNDS.height + REVIEWER_NODE_GAP
          ).toBe(true);
        }
      }
    }
  });

  it("keeps compact activity nodes clear of reviewers and the agent on crowded canvases", () => {
    for (const canvas of [
      { w: 1000, h: 400 },
      { w: 600, h: 600 },
    ]) {
      const center = { x: canvas.w / 2, y: canvas.h / 2 - 10 };
      const reviewers = layoutReviewerPositions(5, canvas, center);
      const activities: CanvasPoint[] = [];

      reviewers.forEach((reviewerPosition, reviewerIndex) => {
        const activity = layoutReviewerActivityPosition(
          reviewerPosition,
          reviewers.filter((_, index) => index !== reviewerIndex),
          activities,
          canvas,
          center
        );

        expect(activity).not.toBeNull();
        if (!activity) return;
        expect(
          rectanglesOverlap(activity, REVIEWER_ACTIVITY_BOUNDS, center, {
            width: 104,
            height: 104,
          })
        ).toBe(false);
        reviewers.forEach((otherReviewer) => {
          expect(
            rectanglesOverlap(
              activity,
              REVIEWER_ACTIVITY_BOUNDS,
              otherReviewer,
              REVIEWER_NODE_BOUNDS
            )
          ).toBe(false);
        });
        activities.forEach((otherActivity) => {
          expect(
            rectanglesOverlap(
              activity,
              REVIEWER_ACTIVITY_BOUNDS,
              otherActivity,
              REVIEWER_ACTIVITY_BOUNDS
            )
          ).toBe(false);
        });
        activities.push(activity);
      });
    }
  });

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

  it("groups multiple tool calls into one separated reviewer activity node", () => {
    const toolNodes = Array.from({ length: 6 }, (_, index) => ({
      ...fileRead,
      id: `review-tool:Architecture:detector:${index}:call-${index}`,
      target: `src/review/file-${index}.rs`,
    }));
    const { container } = render(
      <ConstellationCanvas
        toolNodes={toolNodes}
        reviewerNodes={[reviewer]}
        reviewActive
        agentRunning={false}
      />
    );

    const activityNode = container.querySelector<HTMLElement>(".constellation-node-cluster");
    const reviewerNode = container.querySelector<HTMLElement>(".constellation-node-reviewer");
    expect(activityNode).not.toBeNull();
    expect(reviewerNode).not.toBeNull();
    expect(container.querySelectorAll(".constellation-node-tool")).toHaveLength(0);
    expect(activityNode?.style.top).toBe(reviewerNode?.style.top);
    expect(
      Math.abs(
        Number.parseFloat(activityNode?.style.left ?? "0") -
          Number.parseFloat(reviewerNode?.style.left ?? "0")
      )
    ).toBeGreaterThanOrEqual(150);
    expect(
      container.querySelector(
        'path[data-edge-from="Architecture"][data-edge-to^="reviewer-cluster:Architecture:"]'
      )
    ).not.toBeNull();

    const trigger = screen.getByRole("button", { name: "6 grouped reviewer tool calls" });
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(trigger);
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("6 grouped tool calls")).toBeDefined();
    expect(screen.getByText("src/review/file-0.rs")).toBeDefined();
    expect(screen.getByText("src/review/file-5.rs")).toBeDefined();
  });
});

function rectanglesOverlap(
  first: CanvasPoint,
  firstBounds: { width: number; height: number },
  second: CanvasPoint,
  secondBounds: { width: number; height: number }
): boolean {
  return (
    Math.abs(first.x - second.x) < (firstBounds.width + secondBounds.width) / 2 &&
    Math.abs(first.y - second.y) < (firstBounds.height + secondBounds.height) / 2
  );
}
