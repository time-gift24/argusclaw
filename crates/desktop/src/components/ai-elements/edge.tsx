import {
  BaseEdge,
  getBezierPath,
  getSimpleBezierPath,
  type EdgeProps,
} from "@xyflow/react";

/**
 * Temporary edge - dashed line for temporary connections
 */
function EdgeTemporary({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  style,
}: EdgeProps) {
  const [edgePath] = getSimpleBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
  });

  return (
    <BaseEdge
      id={id}
      path={edgePath}
      style={{
        ...style,
        stroke: "var(--muted-foreground)",
        strokeWidth: 2,
        strokeDasharray: "5 5",
      }}
    />
  );
}

/**
 * Animated edge - solid line with animation
 */
function EdgeAnimated({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  style,
}: EdgeProps) {
  const [edgePath] = getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
  });

  return (
    <BaseEdge
      id={id}
      path={edgePath}
      style={{
        ...style,
        stroke: "var(--muted-foreground)",
        strokeWidth: 2,
      }}
      markerEnd={`url(#arrow-${id})`}
    />
  );
}

const Edge = {
  Temporary: EdgeTemporary,
  Animated: EdgeAnimated,
};

export { Edge, EdgeTemporary, EdgeAnimated };
