// ============================================================
// Synapse — Node Registry
// ============================================================
// Central registry of all available node types. This powers
// the sidebar palette and provides default configs for new nodes.
// ============================================================

import type { NodeCategory, NodeKind, NodeConfig } from '../types/nodes';

export interface NodeRegistryEntry {
  kind: NodeKind;
  category: NodeCategory;
  name: string;
  description: string;
  icon: string; // Lucide icon name
  defaultConfig: NodeConfig;
}

export const nodeRegistry: NodeRegistryEntry[] = [
  // ─── Triggers ──────────────────────────────────────
  {
    kind: 'hotkey_trigger',
    category: 'trigger',
    name: 'Kısayol Tuşu',
    description: 'Tuş kombinasyonuna basıldığında başlat',
    icon: 'Keyboard',
    defaultConfig: {
      label: 'Kısayol Tuşu',
      keys: ['Ctrl', 'Shift', 'F1'],
    },
  },
  {
    kind: 'pixel_color_trigger',
    category: 'trigger',
    name: 'Piksel Rengi',
    description: 'Belirli bir renk tespit edildiğinde',
    icon: 'Pipette',
    defaultConfig: {
      label: 'Piksel Rengi',
      x: 0,
      y: 0,
      color: '#ff0000',
      tolerance: 10,
    },
  },
  {
    kind: 'image_match_trigger',
    category: 'trigger',
    name: 'Görsel Eşleme',
    description: 'Ekranda bir görsel tespit edildiğinde',
    icon: 'ScanSearch',
    defaultConfig: {
      label: 'Görsel Eşleme',
      templatePath: '',
      confidence: 0.9,
    },
  },
  {
    kind: 'timer_trigger',
    category: 'trigger',
    name: 'Zamanlayıcı',
    description: 'Belirli aralıklarla tekrarla',
    icon: 'Timer',
    defaultConfig: {
      label: 'Zamanlayıcı',
      intervalMs: 1000,
      repeat: true,
    },
  },

  // ─── Actions ──────────────────────────────────────
  {
    kind: 'mouse_click',
    category: 'action',
    name: 'Fare Tıklama',
    description: 'Belirli bir noktaya tıkla',
    icon: 'MousePointerClick',
    defaultConfig: {
      label: 'Fare Tıklama',
      button: 'left',
      clickType: 'single',
      x: 0,
      y: 0,
      relative: false,
      inputLevel: 'standard',
    },
  },
  {
    kind: 'mouse_move',
    category: 'action',
    name: 'Fare Hareketi',
    description: 'Fareyi belirli bir noktaya taşı',
    icon: 'Move',
    defaultConfig: {
      label: 'Fare Hareketi',
      x: 0,
      y: 0,
      duration: 200,
      curve: 'humanized',
      relative: false,
    },
  },
  {
    kind: 'key_press',
    category: 'action',
    name: 'Tuş Vuruşu',
    description: 'Klavye tuşuna bas',
    icon: 'KeyRound',
    defaultConfig: {
      label: 'Tuş Vuruşu',
      key: 'Enter',
      modifiers: [],
      holdMs: 50,
      inputLevel: 'standard',
    },
  },
  {
    kind: 'type_text',
    category: 'action',
    name: 'Metin Yaz',
    description: 'Metin dizisi yaz',
    icon: 'Type',
    defaultConfig: {
      label: 'Metin Yaz',
      text: '',
      delayPerChar: 30,
      humanized: true,
    },
  },
  {
    kind: 'delay',
    category: 'action',
    name: 'Bekle',
    description: 'Belirli süre bekle',
    icon: 'Clock',
    defaultConfig: {
      label: 'Bekle',
      durationMs: 1000,
      randomRange: 0,
    },
  },
  {
    kind: 'run_program',
    category: 'action',
    name: 'Program Çalıştır',
    description: 'Harici bir program başlat',
    icon: 'Terminal',
    defaultConfig: {
      label: 'Program Çalıştır',
      path: '',
      args: [],
      waitForExit: true,
    },
  },
  {
    kind: 'set_variable',
    category: 'action',
    name: 'Değişken Ata',
    description: 'Bir değişkene değer ata',
    icon: 'Variable',
    defaultConfig: {
      label: 'Değişken Ata',
      variableName: 'myVar',
      value: '',
      valueType: 'string',
    },
  },

  // ─── Conditions ──────────────────────────────────────
  {
    kind: 'if_else',
    category: 'condition',
    name: 'Koşul (Eğer)',
    description: 'Koşula göre dallan',
    icon: 'GitBranch',
    defaultConfig: {
      label: 'Koşul (Eğer)',
      leftOperand: '',
      operator: '==',
      rightOperand: '',
    },
  },
  {
    kind: 'pixel_check',
    category: 'condition',
    name: 'Piksel Kontrolü',
    description: 'Piksel rengini kontrol et',
    icon: 'Crosshair',
    defaultConfig: {
      label: 'Piksel Kontrolü',
      x: 0,
      y: 0,
      expectedColor: '#ff0000',
      tolerance: 10,
    },
  },
  {
    kind: 'image_exists',
    category: 'condition',
    name: 'Görsel Var mı?',
    description: 'Ekranda bir görselin varlığını kontrol et',
    icon: 'ImageSearch',
    defaultConfig: {
      label: 'Görsel Var mı?',
      templatePath: '',
      confidence: 0.9,
    },
  },

  // ─── Flow Control ──────────────────────────────────
  {
    kind: 'loop',
    category: 'loop',
    name: 'Döngü',
    description: 'Belirli sayıda veya sonsuz tekrarla',
    icon: 'Repeat',
    defaultConfig: {
      label: 'Döngü',
      count: 10,
    },
  },
  {
    kind: 'while_loop',
    category: 'loop',
    name: 'Koşullu Döngü',
    description: 'Koşul sağlandıkça tekrarla',
    icon: 'RefreshCw',
    defaultConfig: {
      label: 'Koşullu Döngü',
      condition: 'true',
    },
  },
];

/** Lookup a registry entry by kind */
export function getNodeEntry(kind: NodeKind): NodeRegistryEntry | undefined {
  return nodeRegistry.find((e) => e.kind === kind);
}

/** Get all entries for a category */
export function getNodesByCategory(category: NodeCategory): NodeRegistryEntry[] {
  return nodeRegistry.filter((e) => e.category === category);
}

/** Category display metadata */
export const categoryMeta: Record<NodeCategory, { name: string; color: string }> = {
  trigger: { name: 'Tetikleyiciler', color: 'var(--node-trigger)' },
  action: { name: 'Eylemler', color: 'var(--node-action)' },
  condition: { name: 'Koşullar', color: 'var(--node-condition)' },
  loop: { name: 'Akış Kontrolü', color: 'var(--node-loop)' },
};
