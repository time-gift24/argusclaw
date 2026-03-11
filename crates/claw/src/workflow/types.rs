use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    pub position: Position,
    pub data: NodeData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeData {
    pub label: String,
}

impl Workflow {
    /// 创建演示 workflow
    #[must_use]
    pub fn demo() -> Self {
        Self {
            id: "demo".to_string(),
            name: "Demo Workflow".to_string(),
            nodes: vec![
                WorkflowNode {
                    id: "1".into(),
                    position: Position { x: 100.0, y: 100.0 },
                    data: NodeData {
                        label: "输入节点".into(),
                    },
                },
                WorkflowNode {
                    id: "2".into(),
                    position: Position { x: 300.0, y: 100.0 },
                    data: NodeData {
                        label: "处理节点".into(),
                    },
                },
                WorkflowNode {
                    id: "3".into(),
                    position: Position { x: 500.0, y: 100.0 },
                    data: NodeData {
                        label: "输出节点".into(),
                    },
                },
                WorkflowNode {
                    id: "4".into(),
                    position: Position { x: 300.0, y: 250.0 },
                    data: NodeData {
                        label: "动画连接".into(),
                    },
                },
            ],
            edges: vec![
                WorkflowEdge {
                    id: "e1-2".into(),
                    source: "1".into(),
                    target: "2".into(),
                    edge_type: Some("temporary".into()),
                },
                WorkflowEdge {
                    id: "e2-3".into(),
                    source: "2".into(),
                    target: "3".into(),
                    edge_type: Some("temporary".into()),
                },
                WorkflowEdge {
                    id: "e2-4".into(),
                    source: "2".into(),
                    target: "4".into(),
                    edge_type: Some("animated".into()),
                },
            ],
        }
    }
}
