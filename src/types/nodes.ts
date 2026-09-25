// ============================================================
// Synapse — Node Type Definitions
// ============================================================
// These types define the data model for all visual nodes in the
// flow editor. They are designed with Phase 2 IR compilation
// in mind: every node carries a `nodeKind` discriminator and a
// strongly-typed `config` payload that maps 1:1 to IR opcodes.
// ============================================================

import type { Node } from '@xyflow/react';

export type NodeCategory = 'trigger' | 'action' | 'condition' | 'loop';

/** Base configuration shared by all node types */
export interface BaseNodeConfig {
  label: string;
  description?: string;
  disabled?: boolean;
}

// --- Trigger Configs ---
export interface HotkeyTriggerConfig extends BaseNodeConfig {
  keys: string[];
}

export interface PixelColorTriggerConfig extends BaseNodeConfig {
  x: number;
  y: number;
  color: string; // hex
  tolerance: number;
  region?: { x: number; y: number; w: number; h: number };
}

export interface ImageMatchTriggerConfig extends BaseNodeConfig {
  templatePath: string;
  confidence: number; // 0-1
  region?: { x: number; y: number; w: number; h: number };
}

export interface TimerTriggerConfig extends BaseNodeConfig {
  intervalMs: number;
  repeat: boolean;
}

// --- Action Configs ---
export interface MouseClickConfig extends BaseNodeConfig {
  button: 'left' | 'right' | 'middle';
  clickType: 'single' | 'double' | 'hold';
  x: number;
  y: number;
  relative: boolean;
  inputLevel: 'standard' | 'interception' | 'virtual_hid';
}

export interface MouseMoveConfig extends BaseNodeConfig {
  x: number;
  y: number;
  duration: number; // ms
  curve: 'linear' | 'bezier' | 'humanized';
  relative: boolean;
}

export interface KeyPressConfig extends BaseNodeConfig {
  key: string;
  modifiers: ('ctrl' | 'alt' | 'shift' | 'win')[];
  holdMs: number;
  inputLevel: 'standard' | 'interception' | 'virtual_hid';
}

export interface TypeTextConfig extends BaseNodeConfig {
  text: string;
  delayPerChar: number; // ms
  humanized: boolean;
}

export interface DelayConfig extends BaseNodeConfig {
  durationMs: number;
  randomRange: number; // ±ms
}

export interface RunProgramConfig extends BaseNodeConfig {
  path: string;
  args: string[];
  waitForExit: boolean;
}

export interface SetVariableConfig extends BaseNodeConfig {
  variableName: string;
  value: string;
  valueType: 'string' | 'number' | 'boolean';
}

// --- Condition Configs ---
export interface IfElseConfig extends BaseNodeConfig {
  leftOperand: string;
  operator: '==' | '!=' | '>' | '<' | '>=' | '<=';
  rightOperand: string;
}

export interface PixelCheckConfig extends BaseNodeConfig {
  x: number;
  y: number;
  expectedColor: string;
  tolerance: number;
}

export interface ImageExistsConfig extends BaseNodeConfig {
  templatePath: string;
  confidence: number;
  region?: { x: number; y: number; w: number; h: number };
}

// --- Loop Configs ---
export interface LoopConfig extends BaseNodeConfig {
  count: number; // 0 = infinite
}

export interface WhileLoopConfig extends BaseNodeConfig {
  condition: string; // expression
}

/** Union of all possible node configs */
export type NodeConfig =
  | HotkeyTriggerConfig
  | PixelColorTriggerConfig
  | ImageMatchTriggerConfig
  | TimerTriggerConfig
  | MouseClickConfig
  | MouseMoveConfig
  | KeyPressConfig
  | TypeTextConfig
  | DelayConfig
  | RunProgramConfig
  | SetVariableConfig
  | IfElseConfig
  | PixelCheckConfig
  | ImageExistsConfig
  | LoopConfig
  | WhileLoopConfig;

/** The `nodeKind` discriminator — maps directly to IR opcode families */
export type NodeKind =
  // Triggers
  | 'hotkey_trigger'
  | 'pixel_color_trigger'
  | 'image_match_trigger'
  | 'timer_trigger'
  // Actions
  | 'mouse_click'
  | 'mouse_move'
  | 'key_press'
  | 'type_text'
  | 'delay'
  | 'run_program'
  | 'set_variable'
  // Conditions
  | 'if_else'
  | 'pixel_check'
  | 'image_exists'
  // Flow Control
  | 'loop'
  | 'while_loop';

/**
 * Custom data payload attached to every React Flow node.
 *
 * Extends `Record<string, unknown>` to satisfy @xyflow/react v12's `Node<T>`
 * data constraint (`T extends Record<string, unknown>`).
 */
export interface SynapseNodeData extends Record<string, unknown> {
  nodeKind: NodeKind;
  category: NodeCategory;
  config: NodeConfig;
}

/** A React Flow node specialized with Synapse's data payload. */
export type SynapseNode = Node<SynapseNodeData>;
