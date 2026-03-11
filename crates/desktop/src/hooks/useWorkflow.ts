import { useState, useEffect, useCallback } from "react";
import { useNodesState, useEdgesState, type Node, type Edge } from "@xyflow/react";
import {
  getWorkflow,
  saveWorkflow,
  toXYFlowNodes,
  toXYFlowEdges,
  fromXYFlow,
} from "@/api/workflow";

export function useWorkflow(workflowId: string) {
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [workflowName, setWorkflowName] = useState("");

  useEffect(() => {
    setLoading(true);
    setError(null);
    getWorkflow(workflowId)
      .then((w) => {
        setWorkflowName(w.name);
        setNodes(toXYFlowNodes(w.nodes));
        setEdges(toXYFlowEdges(w.edges));
      })
      .catch((e) => setError(e.message || String(e)))
      .finally(() => setLoading(false));
  }, [workflowId, setNodes, setEdges]);

  const save = useCallback(async () => {
    const { nodes: wn, edges: we } = fromXYFlow(nodes, edges);
    await saveWorkflow({
      id: workflowId,
      name: workflowName || "Workflow",
      nodes: wn,
      edges: we,
    });
  }, [workflowId, workflowName, nodes, edges]);

  return {
    nodes,
    edges,
    onNodesChange,
    onEdgesChange,
    loading,
    error,
    save,
    workflowName,
    setWorkflowName,
  };
}
