import { X, MousePointer } from 'lucide-react';
import { useFlowStore, type SynapseNode } from '../../stores/flowStore';
import { getNodeEntry } from '../../utils/nodeRegistry';

export function PropertiesPanel() {
  const nodes = useFlowStore((s) => s.nodes);
  const selectedNodeId = useFlowStore((s) => s.selectedNodeId);
  const updateNodeConfig = useFlowStore((s) => s.updateNodeConfig);
  const removeNode = useFlowStore((s) => s.removeNode);
  const selectNode = useFlowStore((s) => s.selectNode);

  const selectedNode: SynapseNode | undefined = nodes.find((n) => n.id === selectedNodeId);

  if (!selectedNode) {
    return (
      <aside className="properties-panel" id="properties-panel">
        <div className="properties-panel__header">
          <span className="properties-panel__title">Özellikler</span>
        </div>
        <div className="properties-panel__empty">
          <MousePointer size={32} className="properties-panel__empty-icon" />
          <p className="properties-panel__empty-text">
            Bir düğüm seçin<br />veya canvas'a sürükleyin
          </p>
        </div>
      </aside>
    );
  }

  const entry = getNodeEntry(selectedNode.data.nodeKind);
  const config = selectedNode.data.config as unknown as Record<string, unknown>;

  const handleChange = (key: string, value: string | number | boolean) => {
    updateNodeConfig(selectedNode.id, { [key]: value } as any);
  };

  return (
    <aside className="properties-panel" id="properties-panel">
      <div className="properties-panel__header">
        <span className="properties-panel__title">
          {entry?.name || 'Özellikler'}
        </span>
        <button
          className="btn btn-icon btn-ghost"
          onClick={() => selectNode(null)}
          style={{ width: 24, height: 24, padding: 2 }}
        >
          <X size={14} />
        </button>
      </div>
      <div className="properties-panel__content">
        {/* General Section */}
        <div className="properties-panel__section">
          <div className="properties-panel__section-title">Genel</div>
          <div className="properties-panel__input-group">
            <label className="properties-panel__label">Etiket</label>
            <input
              className="properties-panel__input"
              value={String(config.label || '')}
              onChange={(e) => handleChange('label', e.target.value)}
            />
          </div>
          <div className="properties-panel__input-group">
            <label className="properties-panel__label">Açıklama</label>
            <input
              className="properties-panel__input"
              value={String(config.description || '')}
              onChange={(e) => handleChange('description', e.target.value)}
              placeholder="İsteğe bağlı açıklama..."
            />
          </div>
        </div>

        {/* Dynamic Config Section */}
        <div className="properties-panel__section">
          <div className="properties-panel__section-title">Yapılandırma</div>
          {Object.entries(config)
            .filter(([key]) => !['label', 'description', 'disabled'].includes(key))
            .map(([key, value]) => (
              <div className="properties-panel__input-group" key={key}>
                <label className="properties-panel__label">{key}</label>
                {typeof value === 'boolean' ? (
                  <select
                    className="properties-panel__select"
                    value={String(value)}
                    onChange={(e) => handleChange(key, e.target.value === 'true')}
                  >
                    <option value="true">Evet</option>
                    <option value="false">Hayır</option>
                  </select>
                ) : typeof value === 'number' ? (
                  <input
                    className="properties-panel__input"
                    type="number"
                    value={value}
                    onChange={(e) => handleChange(key, Number(e.target.value))}
                  />
                ) : Array.isArray(value) ? (
                  <input
                    className="properties-panel__input"
                    value={(value as string[]).join(', ')}
                    onChange={(e) => handleChange(key, e.target.value)}
                    placeholder="Virgülle ayır..."
                  />
                ) : (
                  <input
                    className="properties-panel__input"
                    value={String(value || '')}
                    onChange={(e) => handleChange(key, e.target.value)}
                  />
                )}
              </div>
            ))}
        </div>

        {/* Danger Zone */}
        <div className="properties-panel__section">
          <button
            className="btn btn-danger"
            style={{ width: '100%' }}
            onClick={() => { removeNode(selectedNode.id); selectNode(null); }}
          >
            Düğümü Sil
          </button>
        </div>
      </div>
    </aside>
  );
}
