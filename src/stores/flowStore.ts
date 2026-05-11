// ============================================================
// Synapse — Flow Store (Zustand)
// ============================================================
// Manages the React Flow graph state. Designed so that the full
// node/edge graph can be serialized to JSON for Tauri IPC at
// any point, and later deserialized back into the store.
// ============================================================

import { create } from 'zustand';
import {
  type Node,
  type Edge,
  type OnNodesChange,
  type OnEdgesChange,
  type OnConnect,
  applyNodeChanges,
  applyEdgeChanges,
  addEdge,
  type Connection,
  type Viewport,
} from '@xyflow/react';
import type { SynapseNodeData, NodeKind, NodeConfig } from '../types/nodes';
import { getNodeEntry } from '../utils/nodeRegistry';

export type SynapseNode = Node<SynapseNodeData>;

interface FlowState {
  // Graph data
  nodes: SynapseNode[];
  edges: Edge[];
  viewport: Viewport;

  // Flow metadata
  flowId: string;
  flowName: string;
  isDirty: boolean;

  // Selection
  selectedNodeId: string | null;

  // React Flow callbacks
  onNodesChange: OnNodesChange;
  onEdgesChange: OnEdgesChange;
  onConnect: OnConnect;

  // Actions
  addNode: (kind: NodeKind, position: { x: number; y: number }) => void;
  removeNode: (id: string) => void;
  updateNodeConfig: (id: string, config: Partial<NodeConfig>) => void;
  selectNode: (id: string | null) => void;
  setViewport: (viewport: Viewport) => void;

  // Flow management
  newFlow: () => void;
  loadFlow: (data: {
    id: string;
    name: string;
    nodes: SynapseNode[];
    edges: Edge[];
    viewport: Viewport;
  }) => void;
  setFlowName: (name: string) => void;

  // Serialization
  toJSON: () => object;
}

let nodeIdCounter = 0;
function generateNodeId(): string {
  return `node_${Date.now()}_${++nodeIdCounter}`;
}

export const useFlowStore = create<FlowState>((set, get) => ({
  nodes: [],
  edges: [],
  viewport: { x: 0, y: 0, zoom: 1 },

  flowId: crypto.randomUUID(),
  flowName: 'İsimsiz Flow',
  isDirty: false,

  selectedNodeId: null,

  onNodesChange: (changes) => {
    set((state) => ({
      nodes: applyNodeChanges(changes, state.nodes) as SynapseNode[],
      isDirty: true,
    }));
  },

  onEdgesChange: (changes) => {
    set((state) => ({
      edges: applyEdgeChanges(changes, state.edges),
      isDirty: true,
    }));
  },

  onConnect: (connection: Connection) => {
    set((state) => ({
      edges: addEdge(
        {
          ...connection,
          animated: true,
          style: { strokeWidth: 2 },
        },
        state.edges
      ),
      isDirty: true,
    }));
  },

  addNode: (kind: NodeKind, position: { x: number; y: number }) => {
    const entry = getNodeEntry(kind);
    if (!entry) return;

    const newNode: SynapseNode = {
      id: generateNodeId(),
      type: entry.category, // Maps to custom node component name
      position,
      data: {
        nodeKind: kind,
        category: entry.category,
        config: { ...entry.defaultConfig },
      },
    };

    set((state) => ({
      nodes: [...state.nodes, newNode],
      isDirty: true,
    }));
  },

  removeNode: (id: string) => {
    set((state) => ({
      nodes: state.nodes.filter((n) => n.id !== id),
      edges: state.edges.filter((e) => e.source !== id && e.target !== id),
      selectedNodeId: state.selectedNodeId === id ? null : state.selectedNodeId,
      isDirty: true,
    }));
  },

  updateNodeConfig: (id: string, config: Partial<NodeConfig>) => {
    set((state) => ({
      nodes: state.nodes.map((node) =>
        node.id === id
          ? {
              ...node,
              data: {
                ...node.data,
                config: { ...node.data.config, ...config },
              },
            }
          : node
      ),
      isDirty: true,
    }));
  },

  selectNode: (id: string | null) => {
    set({ selectedNodeId: id });
  },

  setViewport: (viewport: Viewport) => {
    set({ viewport });
  },

  newFlow: () => {
    set({
      nodes: [],
      edges: [],
      viewport: { x: 0, y: 0, zoom: 1 },
      flowId: crypto.randomUUID(),
      flowName: 'İsimsiz Flow',
      isDirty: false,
      selectedNodeId: null,
    });
  },

  loadFlow: (data) => {
    set({
      nodes: data.nodes,
      edges: data.edges,
      viewport: data.viewport,
      flowId: data.id,
      flowName: data.name,
      isDirty: false,
      selectedNodeId: null,
    });
  },

  setFlowName: (name: string) => {
    set({ flowName: name, isDirty: true });
  },

  toJSON: () => {
    const state = get();
    return {
      id: state.flowId,
      name: state.flowName,
      version: 1,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      nodes: state.nodes,
      edges: state.edges,
      viewport: state.viewport,
      variables: [],
    };
  },
}));
