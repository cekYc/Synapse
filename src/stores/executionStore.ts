// ============================================================
// Synapse — Execution Store (Zustand)
// ============================================================
// Manages execution state. Listens to Tauri events from the
// Rust executor for real-time status updates.
// ============================================================

import { create } from 'zustand';
import type { ExecutionStatus } from '../types/flow';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type LogLevel = 'info' | 'warn' | 'error' | 'debug';

export interface LogEntry {
  id: string;
  timestamp: string;
  level: LogLevel;
  message: string;
}

interface ExecutionState {
  status: ExecutionStatus;
  activeNodeId: string | null;
  currentIteration: number;
  startedAt: string | null;
  error: string | null;
  variables: Record<string, string>;
  logs: LogEntry[];

  // Event listener cleanup
  _unlisten: UnlistenFn | null;

  // Actions
  setStatus: (status: ExecutionStatus) => void;
  setActiveNode: (id: string | null) => void;
  setError: (error: string) => void;
  addLog: (message: string, level: LogLevel) => void;
  clearLogs: () => void;
  reset: () => void;

  // Tauri event listener
  startListening: () => Promise<void>;
  stopListening: () => void;
}

export const useExecutionStore = create<ExecutionState>((set, get) => ({
  status: 'idle',
  activeNodeId: null,
  currentIteration: 0,
  startedAt: null,
  error: null,
  variables: {},
  logs: [],
  _unlisten: null,

  setStatus: (status) => set({ status }),

  setActiveNode: (id) => set({ activeNodeId: id }),

  setError: (error) => set({ status: 'error', error }),

  addLog: (message, level) =>
    set((state) => ({
      logs: [
        ...state.logs.slice(-499),
        {
          id: crypto.randomUUID(),
          timestamp: new Date().toISOString(),
          level,
          message,
        },
      ],
    })),

  clearLogs: () => set({ logs: [] }),

  reset: () =>
    set({
      status: 'idle',
      activeNodeId: null,
      currentIteration: 0,
      startedAt: null,
      error: null,
    }),

  startListening: async () => {
    // Clean up previous listener if any
    const prev = get()._unlisten;
    if (prev) prev();

    const unlisten = await listen<any>('synapse://execution', (event) => {
      const payload = event.payload;
      const store = get();

      switch (payload.type) {
        case 'Started':
          set({
            status: 'running',
            startedAt: new Date().toISOString(),
            error: null,
            activeNodeId: null,
          });
          store.addLog(`Flow started`, 'info');
          break;

        case 'NodeActivated':
          set({ activeNodeId: payload.node_id });
          break;

        case 'NodeCompleted':
          // Node is done, but keep activeNodeId until next activation
          break;

        case 'Log':
          store.addLog(payload.message, payload.level as LogLevel);
          break;

        case 'VariableChanged':
          set((state) => ({
            variables: {
              ...state.variables,
              [payload.name]: payload.value,
            },
          }));
          break;

        case 'Paused':
          set({ status: 'paused' });
          store.addLog('Execution paused', 'info');
          break;

        case 'Resumed':
          set({ status: 'running' });
          store.addLog('Execution resumed', 'info');
          break;

        case 'Completed':
          set({ status: 'idle', activeNodeId: null });
          store.addLog('Flow completed successfully ✓', 'info');
          break;

        case 'Stopped':
          set({ status: 'stopped', activeNodeId: null });
          store.addLog('Flow stopped by user', 'warn');
          break;

        case 'Error':
          set({
            status: 'error',
            error: payload.message,
            activeNodeId: null,
          });
          store.addLog(`Error: ${payload.message}`, 'error');
          break;
      }
    });

    set({ _unlisten: unlisten });
  },

  stopListening: () => {
    const unlisten = get()._unlisten;
    if (unlisten) {
      unlisten();
      set({ _unlisten: null });
    }
  },
}));
