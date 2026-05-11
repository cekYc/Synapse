// ============================================================
// Synapse — Node Wrapper Component
// ============================================================
// Generic wrapper that renders any Synapse node with the correct
// header color, icon, handles, and body content.
// ============================================================

import { memo } from 'react';
import { Handle, Position, type NodeProps } from '@xyflow/react';
import type { SynapseNodeData, NodeCategory } from '../../types/nodes';
import { getNodeEntry } from '../../utils/nodeRegistry';
import * as Icons from 'lucide-react';

type LucideIcon = React.ComponentType<{ size?: number; className?: string }>;

function getIcon(iconName: string): LucideIcon {
  const icon = (Icons as Record<string, unknown>)[iconName] as LucideIcon | undefined;
  return icon || Icons.CircleDot;
}

interface NodeFieldProps {
  label: string;
  value: string | number;
}

function NodeField({ label, value }: NodeFieldProps) {
  return (
    <div className="synapse-node__field">
      <span className="synapse-node__field-label">{label}</span>
      <span className="synapse-node__field-value">{String(value)}</span>
    </div>
  );
}

function getNodeFields(data: SynapseNodeData): { label: string; value: string | number }[] {
  const config = data.config;
  const kind = data.nodeKind;

  switch (kind) {
    case 'hotkey_trigger':
      return [{ label: 'Tuşlar', value: (config as any).keys?.join(' + ') || '—' }];
    case 'pixel_color_trigger':
      return [
        { label: 'Konum', value: `(${(config as any).x}, ${(config as any).y})` },
        { label: 'Renk', value: (config as any).color || '#000' },
      ];
    case 'image_match_trigger':
      return [
        { label: 'Güven', value: `${((config as any).confidence * 100).toFixed(0)}%` },
      ];
    case 'timer_trigger':
      return [{ label: 'Aralık', value: `${(config as any).intervalMs}ms` }];
    case 'mouse_click':
      return [
        { label: 'Buton', value: (config as any).button },
        { label: 'Konum', value: `(${(config as any).x}, ${(config as any).y})` },
      ];
    case 'mouse_move':
      return [
        { label: 'Hedef', value: `(${(config as any).x}, ${(config as any).y})` },
        { label: 'Süre', value: `${(config as any).duration}ms` },
      ];
    case 'key_press':
      return [
        { label: 'Tuş', value: (config as any).key },
        {
          label: 'Modifiers',
          value: (config as any).modifiers?.length > 0
            ? (config as any).modifiers.join(' + ')
            : 'Yok',
        },
      ];
    case 'type_text':
      return [
        { label: 'Metin', value: (config as any).text || '(boş)' },
      ];
    case 'delay':
      return [{ label: 'Süre', value: `${(config as any).durationMs}ms` }];
    case 'run_program':
      return [{ label: 'Yol', value: (config as any).path || '(boş)' }];
    case 'set_variable':
      return [
        { label: 'Değişken', value: (config as any).variableName },
        { label: 'Değer', value: (config as any).value || '(boş)' },
      ];
    case 'if_else':
      return [
        {
          label: 'Koşul',
          value: `${(config as any).leftOperand} ${(config as any).operator} ${(config as any).rightOperand}`,
        },
      ];
    case 'pixel_check':
      return [
        { label: 'Konum', value: `(${(config as any).x}, ${(config as any).y})` },
        { label: 'Renk', value: (config as any).expectedColor },
      ];
    case 'image_exists':
      return [{ label: 'Güven', value: `${((config as any).confidence * 100).toFixed(0)}%` }];
    case 'loop':
      return [{ label: 'Tekrar', value: (config as any).count === 0 ? '∞' : (config as any).count }];
    case 'while_loop':
      return [{ label: 'Koşul', value: (config as any).condition }];
    default:
      return [];
  }
}

/** Which handle positions a node category uses */
const handleConfig: Record<NodeCategory, { inputs: Position[]; outputs: Position[] }> = {
  trigger: {
    inputs: [],
    outputs: [Position.Bottom],
  },
  action: {
    inputs: [Position.Top],
    outputs: [Position.Bottom],
  },
  condition: {
    inputs: [Position.Top],
    outputs: [Position.Bottom, Position.Right], // Bottom = true, Right = false
  },
  loop: {
    inputs: [Position.Top],
    outputs: [Position.Bottom, Position.Right], // Bottom = body, Right = exit
  },
};

function NodeWrapperComponent({ data, selected }: NodeProps) {
  const nodeData = data as unknown as SynapseNodeData;
  const entry = getNodeEntry(nodeData.nodeKind);
  if (!entry) return null;

  const Icon = getIcon(entry.icon);
  const handles = handleConfig[nodeData.category];
  const fields = getNodeFields(nodeData);

  return (
    <div
      className={`synapse-node synapse-node--${nodeData.category} ${selected ? 'selected' : ''}`}
    >
      {/* Input handles */}
      {handles.inputs.map((pos, i) => (
        <Handle
          key={`input-${i}`}
          type="target"
          position={pos}
          id={`input-${i}`}
        />
      ))}

      {/* Header */}
      <div className="synapse-node__header">
        <div className="synapse-node__header-icon">
          <Icon size={14} />
        </div>
        <span className="synapse-node__header-title">
          {nodeData.config.label}
        </span>
      </div>

      {/* Body */}
      {fields.length > 0 && (
        <div className="synapse-node__body">
          {fields.map((field, i) => (
            <NodeField key={i} label={field.label} value={field.value} />
          ))}
        </div>
      )}

      {/* Output handles */}
      {handles.outputs.map((pos, i) => (
        <Handle
          key={`output-${i}`}
          type="source"
          position={pos}
          id={`output-${i}`}
        />
      ))}
    </div>
  );
}

export const TriggerNode = memo(NodeWrapperComponent);
export const ActionNode = memo(NodeWrapperComponent);
export const ConditionNode = memo(NodeWrapperComponent);
export const LoopNode = memo(NodeWrapperComponent);
