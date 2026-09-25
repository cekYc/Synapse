import { useEffect, useState } from 'react';
import { MousePointer, Layers, ZoomIn, Cpu, Monitor } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useFlowStore } from '../../stores/flowStore';
import { useExecutionStore } from '../../stores/executionStore';
import type { VisionBackends } from '../../types/vision';

const captureLabel: Record<VisionBackends['capture'], string> = {
  dxgi: 'DXGI', gdi: 'GDI', unsupported: 'Yok',
};

export function StatusBar() {
  const nodes = useFlowStore((s) => s.nodes);
  const edges = useFlowStore((s) => s.edges);
  const viewport = useFlowStore((s) => s.viewport);
  const status = useExecutionStore((s) => s.status);
  const [vision, setVision] = useState<VisionBackends | null>(null);

  // Probe capture/matching backends once; the first call initializes them
  useEffect(() => {
    let cancelled = false;
    invoke<VisionBackends>('get_vision_backends')
      .then((info) => { if (!cancelled) setVision(info); })
      .catch(() => { /* not running inside Tauri */ });
    return () => { cancelled = true; };
  }, []);

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
        {vision && (
          <>
            <div
              className="statusbar__item"
              title={vision.captureAdapter ? `Ekran yakalama: ${vision.captureAdapter}` : 'Ekran yakalama'}
            >
              <Monitor size={11} />
              <span>{captureLabel[vision.capture]}</span>
            </div>
            <div
              className="statusbar__item"
              title={vision.gpuAdapter ? `Görsel eşleme: ${vision.gpuAdapter}` : 'Görsel eşleme: CPU (paralel)'}
            >
              <Cpu size={11} />
              <span>{vision.matcher === 'gpu' ? 'GPU' : 'CPU'}</span>
            </div>
          </>
        )}
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
