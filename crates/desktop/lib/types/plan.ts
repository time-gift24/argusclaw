export type StepStatus = "pending" | "in_progress" | "completed";

export interface PlanItem {
  step: string;
  status: StepStatus;
}
