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
// Keep in sync with .constellation-reviewer-tool-card in src/styles/global.css.
export const REVIEWER_ACTIVITY_BOUNDS = { width: 120, height: 32 } as const;

const CANVAS_EDGE_PADDING = 24;
export const AGENT_NODE_BOUNDS = { width: 104, height: 104 } as const;
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

function overlapArea(
  first: CanvasPoint,
  firstBounds: NodeBounds,
  second: CanvasPoint,
  secondBounds: NodeBounds
): number {
  const width = Math.max(
    0,
    (firstBounds.width + secondBounds.width) / 2 - Math.abs(first.x - second.x)
  );
  const height = Math.max(
    0,
    (firstBounds.height + secondBounds.height) / 2 - Math.abs(first.y - second.y)
  );
  return width * height;
}

function relaxedAxisBounds(canvasExtent: number, nodeExtent: number): [number, number] {
  if (canvasExtent <= nodeExtent) {
    const midpoint = Math.max(0, canvasExtent) / 2;
    return [midpoint, midpoint];
  }
  const halfNode = nodeExtent / 2;
  const padding = Math.min(CANVAS_EDGE_PADDING, (canvasExtent - nodeExtent) / 2);
  return [halfNode + padding, canvasExtent - halfNode - padding];
}

function steppedRange(min: number, max: number, step: number): number[] {
  const values: number[] = [];
  for (let value = min; value <= max; value += step) values.push(value);
  if (values.length === 0 || values[values.length - 1] < max) values.push(max);
  return values;
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
  const [minY, maxY] = relaxedAxisBounds(canvas.h, REVIEWER_NODE_BOUNDS.height);
  const desiredStep = REVIEWER_NODE_BOUNDS.height + REVIEWER_NODE_GAP;
  const availableSpan = maxY - minY;
  const step = count <= 1 ? 0 : Math.min(desiredStep, availableSpan / (count - 1));
  const span = step * (count - 1);
  const orbitCenterY = clamp(center.y, minY + span / 2, maxY - span / 2);

  const [leftEdge, rightEdge] = relaxedAxisBounds(canvas.w, REVIEWER_NODE_BOUNDS.width);
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

/** Places one compact activity node, preferring collision-free space with a bounded fallback. */
export function layoutReviewerActivityPosition(
  reviewer: CanvasPoint,
  otherReviewers: CanvasPoint[],
  placedActivities: CanvasPoint[],
  canvas: { w: number; h: number },
  center: CanvasPoint
): CanvasPoint {
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
  let nearest: CanvasPoint | null = null;
  let nearestDistance = Infinity;
  for (let y = minY; y <= maxY; y += 16) {
    for (let x = minX; x <= maxX; x += 16) {
      const point = { x, y };
      if (!isAvailable(point)) continue;
      const distance = (x - reviewer.x) ** 2 + (y - reviewer.y) ** 2;
      if (distance < nearestDistance) {
        nearestDistance = distance;
        nearest = point;
      }
    }
  }
  if (nearest) return nearest;

  // A narrow canvas may not have any collision-free point. Keep the activity
  // visible by choosing the bounded point with the least overlap instead of
  // silently dropping the reviewer branch.
  const [fallbackMinX, fallbackMaxX] = relaxedAxisBounds(canvas.w, REVIEWER_ACTIVITY_BOUNDS.width);
  const [fallbackMinY, fallbackMaxY] = relaxedAxisBounds(canvas.h, REVIEWER_ACTIVITY_BOUNDS.height);
  let fallback = {
    x: clamp(reviewer.x, fallbackMinX, fallbackMaxX),
    y: clamp(reviewer.y, fallbackMinY, fallbackMaxY),
  };
  let fallbackOverlap = Number.POSITIVE_INFINITY;
  let fallbackDistance = Number.POSITIVE_INFINITY;

  for (const y of steppedRange(fallbackMinY, fallbackMaxY, 16)) {
    for (const x of steppedRange(fallbackMinX, fallbackMaxX, 16)) {
      const point = { x, y };
      const overlap = obstacles.reduce(
        (total, obstacle) =>
          total + overlapArea(point, REVIEWER_ACTIVITY_BOUNDS, obstacle.point, obstacle.bounds),
        0
      );
      const distance = (x - reviewer.x) ** 2 + (y - reviewer.y) ** 2;
      if (
        overlap < fallbackOverlap ||
        (overlap === fallbackOverlap && distance < fallbackDistance)
      ) {
        fallback = point;
        fallbackOverlap = overlap;
        fallbackDistance = distance;
      }
    }
  }

  return fallback;
}
