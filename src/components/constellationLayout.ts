export interface CanvasPoint {
  x: number;
  y: number;
}

interface NodeBounds {
  width: number;
  height: number;
}

export const REVIEWER_NODE_BOUNDS = { width: 144, height: 88 } as const;
export const REVIEWER_NODE_GAP = 24;
export const REVIEWER_ACTIVITY_BOUNDS = { width: 120, height: 32 } as const;

const CANVAS_EDGE_PADDING = 24;
const AGENT_NODE_BOUNDS = { width: 104, height: 104 } as const;
const REVIEWER_ORBIT_RADIUS = 270;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function overlaps(
  first: CanvasPoint,
  firstBounds: NodeBounds,
  second: CanvasPoint,
  secondBounds: NodeBounds
): boolean {
  return (
    Math.abs(first.x - second.x) < (firstBounds.width + secondBounds.width) / 2 &&
    Math.abs(first.y - second.y) < (firstBounds.height + secondBounds.height) / 2
  );
}

function isInsideCanvas(
  point: CanvasPoint,
  bounds: NodeBounds,
  canvas: { w: number; h: number }
): boolean {
  return (
    point.x - bounds.width / 2 >= CANVAS_EDGE_PADDING &&
    point.x + bounds.width / 2 <= canvas.w - CANVAS_EDGE_PADDING &&
    point.y - bounds.height / 2 >= CANVAS_EDGE_PADDING &&
    point.y + bounds.height / 2 <= canvas.h - CANVAS_EDGE_PADDING
  );
}

/** Places reviewer cards on a separated, right-hand orbit. */
export function layoutReviewerPositions(
  count: number,
  canvas: { w: number; h: number },
  center: CanvasPoint
): CanvasPoint[] {
  if (count <= 0) return [];

  const halfWidth = REVIEWER_NODE_BOUNDS.width / 2;
  const halfHeight = REVIEWER_NODE_BOUNDS.height / 2;
  const minY = halfHeight + CANVAS_EDGE_PADDING;
  const maxY = Math.max(minY, canvas.h - halfHeight - CANVAS_EDGE_PADDING);
  const desiredStep = REVIEWER_NODE_BOUNDS.height + REVIEWER_NODE_GAP;
  const availableSpan = maxY - minY;
  const step = count <= 1 ? 0 : Math.min(desiredStep, availableSpan / (count - 1));
  const span = step * (count - 1);
  const orbitCenterY = clamp(center.y, minY + span / 2, maxY - span / 2);

  const leftEdge = halfWidth + CANVAS_EDGE_PADDING;
  const rightEdge = canvas.w - halfWidth - CANVAS_EDGE_PADDING;
  const maxOrbitOffset = Math.max(0, rightEdge - center.x);
  const outerOffset = Math.min(REVIEWER_ORBIT_RADIUS, maxOrbitOffset);
  const innerOffset = Math.min(
    outerOffset,
    AGENT_NODE_BOUNDS.width / 2 + halfWidth + REVIEWER_NODE_GAP
  );
  const midpoint = (count - 1) / 2;

  // A short canvas cannot provide the desired vertical clearance. Stagger
  // reviewers across as many horizontal lanes as necessary.
  if (count > 1 && step < desiredStep) {
    const horizontalStep = REVIEWER_NODE_BOUNDS.width + REVIEWER_NODE_GAP;
    const requiredLanes = step <= 0 ? count : Math.min(count, Math.ceil(desiredStep / step));
    const availableWidth = Math.max(0, rightEdge - leftEdge);
    const laneCount = Math.min(
      requiredLanes,
      Math.max(1, Math.floor(availableWidth / horizontalStep) + 1)
    );
    const laneSpan = (laneCount - 1) * horizontalStep;
    const preferredStart = center.x + AGENT_NODE_BOUNDS.width / 2 + halfWidth + REVIEWER_NODE_GAP;
    const laneStart = clamp(preferredStart, leftEdge, Math.max(leftEdge, rightEdge - laneSpan));

    return Array.from({ length: count }, (_, index) => ({
      x: laneStart + (laneCount - 1 - (index % laneCount)) * horizontalStep,
      y: orbitCenterY + (index - midpoint) * step,
    }));
  }

  return Array.from({ length: count }, (_, index) => {
    const distanceFromMiddle = midpoint === 0 ? 0 : Math.abs(index - midpoint) / midpoint;
    const orbitCurve = 1 - distanceFromMiddle ** 2;

    return {
      x: center.x + innerOffset + (outerOffset - innerOffset) * orbitCurve,
      y: orbitCenterY + (index - midpoint) * step,
    };
  });
}

/** Places one compact activity node without covering the agent or reviewer cards. */
export function layoutReviewerActivityPosition(
  reviewer: CanvasPoint,
  otherReviewers: CanvasPoint[],
  placedActivities: CanvasPoint[],
  canvas: { w: number; h: number },
  center: CanvasPoint
): CanvasPoint | null {
  const horizontalOffset =
    REVIEWER_NODE_BOUNDS.width / 2 + REVIEWER_ACTIVITY_BOUNDS.width / 2 + REVIEWER_NODE_GAP;
  const inward = reviewer.x >= center.x ? -1 : 1;
  const baseAngle = inward < 0 ? Math.PI : 0;
  const angleOffsets = [
    0,
    -Math.PI / 6,
    Math.PI / 6,
    -Math.PI / 4,
    Math.PI / 4,
    -Math.PI / 3,
    Math.PI / 3,
    -Math.PI / 2,
    Math.PI / 2,
    Math.PI,
  ];
  const candidates: CanvasPoint[] = [];
  for (const radius of [horizontalOffset, horizontalOffset + 40, horizontalOffset + 80]) {
    for (const offset of angleOffsets) {
      candidates.push({
        x: reviewer.x + Math.cos(baseAngle + offset) * radius,
        y: reviewer.y + Math.sin(baseAngle + offset) * radius,
      });
    }
  }
  const obstacles = [
    { point: center, bounds: AGENT_NODE_BOUNDS },
    { point: reviewer, bounds: REVIEWER_NODE_BOUNDS },
    ...otherReviewers.map((point) => ({ point, bounds: REVIEWER_NODE_BOUNDS })),
    ...placedActivities.map((point) => ({ point, bounds: REVIEWER_ACTIVITY_BOUNDS })),
  ];
  const isAvailable = (candidate: CanvasPoint) =>
    isInsideCanvas(candidate, REVIEWER_ACTIVITY_BOUNDS, canvas) &&
    obstacles.every(
      (obstacle) => !overlaps(candidate, REVIEWER_ACTIVITY_BOUNDS, obstacle.point, obstacle.bounds)
    );

  const radialPosition = candidates.find(isAvailable);
  if (radialPosition) return radialPosition;

  const minX = REVIEWER_ACTIVITY_BOUNDS.width / 2 + CANVAS_EDGE_PADDING;
  const maxX = canvas.w - REVIEWER_ACTIVITY_BOUNDS.width / 2 - CANVAS_EDGE_PADDING;
  const minY = REVIEWER_ACTIVITY_BOUNDS.height / 2 + CANVAS_EDGE_PADDING;
  const maxY = canvas.h - REVIEWER_ACTIVITY_BOUNDS.height / 2 - CANVAS_EDGE_PADDING;
  const gridCandidates: CanvasPoint[] = [];
  for (let y = minY; y <= maxY; y += 16) {
    for (let x = minX; x <= maxX; x += 16) gridCandidates.push({ x, y });
  }
  gridCandidates.sort(
    (first, second) =>
      (first.x - reviewer.x) ** 2 +
      (first.y - reviewer.y) ** 2 -
      ((second.x - reviewer.x) ** 2 + (second.y - reviewer.y) ** 2)
  );

  return gridCandidates.find(isAvailable) ?? null;
}
