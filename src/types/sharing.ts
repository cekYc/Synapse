// ============================================================
// Synapse — Flow Sharing Types
// ============================================================
// Mirrors src-tauri/src/sharing (ShareMeta, ImportPreview).
// ============================================================

export interface ShareMeta {
  name: string;
  description: string;
  author: string;
  tags: string[];
}

export type Risk = 'info' | 'warning' | 'danger';

export interface Finding {
  risk: Risk;
  nodeId: string;
  nodeLabel: string;
  message: string;
}

export interface ImportPreview {
  meta: ShareMeta;
  flowName: string;
  appVersion: string;
  createdAt: string;
  nodeCount: number;
  edgeCount: number;
  assetCount: number;
  inputSteps: number;
  findings: Finding[];
  risk: Risk;
}

export type PackageSource =
  | { kind: 'file'; path: string }
  | { kind: 'code'; code: string };

export const PACKAGE_FILTERS = [{ name: 'Synapse Akışı', extensions: ['synapse'] }];
