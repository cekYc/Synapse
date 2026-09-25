import { useCallback, useEffect, useMemo, useRef, type DragEvent } from 'react';
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  BackgroundVariant,
  type ReactFlowInstance,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';

import { useFlowStore } from './stores/flowStore';
import { useExecutionStore } from './stores/executionStore';
import { Toolbar } from './components/layout/Toolbar';
import { Sidebar } from './components/layout/Sidebar';
import { StatusBar } from './components/layout/StatusBar';
import { PropertiesPanel } from './components/layout/PropertiesPanel';
import {
  TriggerNode,
  ActionNode,
  ConditionNode,
  LoopNode,
} from './components/nodes/NodeWrapper';
import type { NodeKind } from './types/nodes';

const nodeTypes = {
  trigger: TriggerNode,
  action: ActionNode,
  condition: ConditionNode,
  loop: LoopNode,
};

export default function App() {
  const reactFlowWrapper = useRef<HTMLDivElement>(null);
  const reactFlowInstance = useRef<ReactFlowInstance | null>(null);

  const nodes = useFlowStore((s) => s.nodes);
  const edges = useFlowStore((s) => s.edges);
  const onNodesChange = useFlowStore((s) => s.onNodesChange);
  const onEdgesChange = useFlowStore((s) => s.onEdgesChange);
  const onConnect = useFlowStore((s) => s.onConnect);
  const addNode = useFlowStore((s) => s.addNode);
  const selectNode = useFlowStore((s) => s.selectNode);
  const setViewport = useFlowStore((s) => s.setViewport);

  const activeNodeId = useExecutionStore((s) => s.activeNodeId);
  const startListening = useExecutionStore((s) => s.startListening);
  const stopListening = useExecutionStore((s) => s.stopListening);

  // Start listening to Tauri execution events on mount
  useEffect(() => {
    startListening();
    return () => stopListening();
  }, [startListening, stopListening]);

  // Apply 'running' class to the active node
  const styledNodes = useMemo(() => {
    return nodes.map((node) => ({
      ...node,
      className: node.id === activeNodeId ? 'running' : '',
    }));
  }, [nodes, activeNodeId]);

  const onDragOver = useCallback((event: DragEvent) => {
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
  }, []);

  const onDrop = useCallback(
    (event: DragEvent) => {
      event.preventDefault();
      const kind = event.dataTransfer.getData('application/synapse-node-kind') as NodeKind;
      if (!kind || !reactFlowInstance.current) return;

      const bounds = reactFlowWrapper.current?.getBoundingClientRect();
      if (!bounds) return;

      const position = reactFlowInstance.current.screenToFlowPosition({
        x: event.clientX - bounds.left,
        y: event.clientY - bounds.top,
      });

      addNode(kind, position);
    },
    [addNode]
  );

  const onNodeClick = useCallback(
    (_: React.MouseEvent, node: any) => {
      selectNode(node.id);
    },
    [selectNode]
  );

  const onPaneClick = useCallback(() => {
    selectNode(null);
  }, [selectNode]);

  return (
    <div className="app-layout">
      <Toolbar />
      <Sidebar />

      <div className="canvas" ref={reactFlowWrapper}>
        <ReactFlow
          nodes={styledNodes}
          edges={edges}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onInit={(instance) => { reactFlowInstance.current = instance as unknown as ReactFlowInstance; }}
          onDrop={onDrop}
          onDragOver={onDragOver}
          onNodeClick={onNodeClick}
          onPaneClick={onPaneClick}
          onMoveEnd={(_, vp) => setViewport(vp)}
          nodeTypes={nodeTypes}
          fitView
          snapToGrid
          snapGrid={[16, 16]}
          defaultEdgeOptions={{
            animated: true,
            style: { strokeWidth: 2 },
          }}
          proOptions={{ hideAttribution: true }}
        >
          <Background
            variant={BackgroundVariant.Dots}
            gap={20}
            size={1}
            color="rgba(255,255,255,0.05)"
          />
          <Controls
            position="bottom-left"
            showInteractive={false}
          />
          <MiniMap
            position="bottom-right"
            nodeColor={(n) => {
              const cat = (n.data as any)?.category;
              if (cat === 'trigger') return '#ef4444';
              if (cat === 'action') return '#3b82f6';
              if (cat === 'condition') return '#f59e0b';
              if (cat === 'loop') return '#10b981';
              return '#6366f1';
            }}
            maskColor="rgba(0,0,0,0.7)"
            style={{ background: 'var(--bg-surface)' }}
          />
        </ReactFlow>
      </div>

      <PropertiesPanel />
      <StatusBar />
    </div>
  );
}
