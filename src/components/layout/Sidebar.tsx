// ============================================================
// Synapse — Sidebar (Node Palette)
// ============================================================

import { useState, useCallback, type DragEvent } from 'react';
import { Search } from 'lucide-react';
import * as Icons from 'lucide-react';
import { nodeRegistry, categoryMeta, type NodeRegistryEntry } from '../../utils/nodeRegistry';
import type { NodeCategory } from '../../types/nodes';

type LucideIcon = React.ComponentType<{ size?: number; className?: string }>;

function getIcon(iconName: string): LucideIcon {
  const icon = (Icons as Record<string, unknown>)[iconName] as LucideIcon | undefined;
  return icon || Icons.CircleDot;
}

const categories: NodeCategory[] = ['trigger', 'action', 'condition', 'loop'];

export function Sidebar() {
  const [search, setSearch] = useState('');

  const filteredRegistry = search
    ? nodeRegistry.filter(
        (entry) =>
          entry.name.toLowerCase().includes(search.toLowerCase()) ||
          entry.description.toLowerCase().includes(search.toLowerCase())
      )
    : nodeRegistry;

  const onDragStart = useCallback(
    (event: DragEvent<HTMLDivElement>, entry: NodeRegistryEntry) => {
      event.dataTransfer.setData('application/synapse-node-kind', entry.kind);
      event.dataTransfer.effectAllowed = 'move';
    },
    []
  );

  return (
    <aside className="sidebar" id="sidebar-palette">
      <div className="sidebar__header">
        <div className="sidebar__title">Düğüm Paleti</div>
        <div className="sidebar__search">
          <Search className="sidebar__search-icon" />
          <input
            className="sidebar__search-input"
            type="text"
            placeholder="Düğüm ara..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            id="sidebar-search"
          />
        </div>
      </div>

      <div className="sidebar__content">
        {categories.map((cat) => {
          const entries = filteredRegistry.filter((e) => e.category === cat);
          if (entries.length === 0) return null;

          const meta = categoryMeta[cat];

          return (
            <div className="sidebar__category" key={cat}>
              <div className="sidebar__category-title">
                <span
                  className="sidebar__category-dot"
                  style={{ background: meta.color }}
                />
                {meta.name}
              </div>

              {entries.map((entry) => {
                const Icon = getIcon(entry.icon);
                return (
                  <div
                    key={entry.kind}
                    className="node-item"
                    draggable
                    onDragStart={(e) => onDragStart(e, entry)}
                    id={`node-item-${entry.kind}`}
                  >
                    <div
                      className="node-item__icon"
                      style={{
                        background:
                          cat === 'trigger'
                            ? 'var(--node-trigger-muted)'
                            : cat === 'action'
                            ? 'var(--node-action-muted)'
                            : cat === 'condition'
                            ? 'var(--node-condition-muted)'
                            : 'var(--node-loop-muted)',
                        color:
                          cat === 'trigger'
                            ? 'var(--node-trigger)'
                            : cat === 'action'
                            ? 'var(--node-action)'
                            : cat === 'condition'
                            ? 'var(--node-condition)'
                            : 'var(--node-loop)',
                      }}
                    >
                      <Icon size={16} />
                    </div>
                    <div className="node-item__info">
                      <div className="node-item__name">{entry.name}</div>
                      <div className="node-item__desc">{entry.description}</div>
                    </div>
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
    </aside>
  );
}
