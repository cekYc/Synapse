import { MousePointer, Layers, ZoomIn } from 'lucide-react';
import { useFlowStore } from '../../stores/flowStore';
import { useExecutionStore } from '../../stores/executionStore';

export function StatusBar() {
  const nodes = useFlowStore((s) => s.nodes);
  const edges = useFlowStore((s) => s.edges);
  const viewport = useFlowStore((s) => s.viewport);
  const status = useExecutionStore((s) => s.status);

  const statusLabel: Record<string, string> = {
    idle: 'Hazır', running: 'Çalışıyor', paused: 'Duraklatıldı',
    stopped: 'Durduruldu', error: 'Hata',
  };
  const statusDotClass: Record<string, string> = {
    idle: 'statusbar__dot--ready', running: 'statusbar__dot--running',
    paused: 'statusbar__dot--running', stopped: 'statusbar__dot--ready',
    error: 'statusbar__dot--error',
  };

  return (
    <footer className="statusbar" id="statusbar">
      <div className="statusbar__left">
        <div className="statusbar__item">
          <span className={`statusbar__dot ${statusDotClass[status] || ''}`} />
          <span>{statusLabel[status] || 'Bilinmiyor'}</span>
        </div>
        <div className="statusbar__item">
          <Layers size={11} />
          <span>{nodes.length} düğüm, {edges.length} bağlantı</span>
        </div>
      </div>
      <div className="statusbar__right">
        <div className="statusbar__item">
          <ZoomIn size={11} />
          <span>{(viewport.zoom * 100).toFixed(0)}%</span>
        </div>
        <div className="statusbar__item">
          <MousePointer size={11} />
          <span>({viewport.x.toFixed(0)}, {viewport.y.toFixed(0)})</span>
        </div>
      </div>
    </footer>
  );
}
