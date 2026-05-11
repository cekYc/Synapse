// ============================================================
// Synapse — Flow Data Structures
// ============================================================

import type { Edge } from '@xyflow/react';
import type { SynapseNode } from './nodes';

/** Serializable flow document — the JSON that gets saved to disk */
export interface FlowDocument {
  id: string;
  name: string;
  description: string;
  version: number;
  createdAt: string;
  updatedAt: string;
  nodes: SynapseNode[];
  edges: Edge[];
  viewport: {
    x: number;
    y: number;
    zoom: number;
  };
  variables: FlowVariable[];
}

export interface FlowVariable {
  name: string;
  defaultValue: string;
  type: 'string' | 'number' | 'boolean';
}

/** Summary for listing flows without loading full graph data */
export interface FlowSummary {
  id: string;
  name: string;
  description: string;
  updatedAt: string;
  nodeCount: number;
}

/** Execution state reported from the Rust engine */
export type ExecutionStatus = 'idle' | 'running' | 'paused' | 'stopped' | 'error';

export interface ExecutionState {
  status: ExecutionStatus;
  activeNodeId: string | null;
  currentIteration: number;
  startedAt: string | null;
  error: string | null;
  variables: Record<string, string>;
}
