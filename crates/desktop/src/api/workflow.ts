import { invoke } from "@tauri-apps/api/core";
import type { Node, Edge } from "@xyflow/react";

export interface Workflow {
  id: string;
  name: string;
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
}

export interface WorkflowNode {
  id: string;
  position: { x: number; y: number };
  data: { label: string };
}

export interface WorkflowEdge {
  id: string;
  source: string;
  target: string;
  type?: string;
}

export async function getWorkflow(id: string): Promise<Workflow> {
  return invoke("get_workflow", { id });
}

export async function saveWorkflow(workflow: Workflow): Promise<void> {
  return invoke("save_workflow", { workflow });
}

export async function listWorkflows(): Promise<Workflow[]> {
  return invoke("list_workflows");
}

export function toXYFlowNodes(nodes: WorkflowNode[]): Node[] {
  return nodes.map((n) => ({ ...n, type: "default" }));
}

export function toXYFlowEdges(edges: WorkflowEdge[]): Edge[] {
  return edges.map((e) => ({ ...e }));
}

export function fromXYFlow(
  nodes: Node[],
  edges: Edge[]
): { nodes: WorkflowNode[]; edges: WorkflowEdge[] } {
  return {
    nodes: nodes.map((n) => ({
      id: n.id,
      position: n.position,
      data: { label: String(n.data?.label || "") },
    })),
    edges: edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      type: e.type,
    })),
  };
}
