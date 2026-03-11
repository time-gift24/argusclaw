import type { ReactFlowProps } from "@xyflow/react";
import { Background, ReactFlow } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { ReactNode } from "react";

type CanvasProps = ReactFlowProps & {
  children?: ReactNode;
};

function Canvas({ children, ...props }: CanvasProps) {
  return (
    <ReactFlow deleteKeyCode={["Backspace", "Delete"]} {...props}>
      <Background />
      {children}
    </ReactFlow>
  );
}

export { Canvas };
export type { CanvasProps };
