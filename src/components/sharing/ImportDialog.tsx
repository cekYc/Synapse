// ============================================================
// Synapse — Import Dialog
// ============================================================
// Import a shared flow from a `.synapse` file or a share code.
// The package is validated and reviewed first; the user sees what
// the flow will do and confirms. Importing loads the flow into the
// editor — it never starts it.
// ============================================================

import { useState } from 'react';
import { FileUp, FolderOpen, ShieldAlert, ShieldCheck, OctagonAlert, TriangleAlert, Info } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { open, confirm } from '@tauri-apps/plugin-dialog';
import { Modal } from './Modal';
import { useFlowStore } from '../../stores/flowStore';
import { useExecutionStore } from '../../stores/executionStore';
import { PACKAGE_FILTERS, type ImportPreview, type PackageSource, type Risk } from '../../types/sharing';

const riskLabel: Record<Risk, string> = { danger: 'Tehlikeli', warning: 'Dikkat', info: 'Bilgi' };
const RiskIcon: Record<Risk, typeof Info> = { danger: OctagonAlert, warning: TriangleAlert, info: Info };

function Stat({ value, label }: { value: number; label: string }) {
  return (
    <div className="share-stat">
      <div className="share-stat__value">{value}</div>
      <div className="share-stat__label">{label}</div>
    </div>
  );
}

export function ImportDialog({ onClose }: { onClose: () => void }) {
  const loadFlow = useFlowStore((s) => s.loadFlow);
  const isDirty = useFlowStore((s) => s.isDirty);
  const status = useExecutionStore((s) => s.status);

  const [code, setCode] = useState('');
  const [source, setSource] = useState<PackageSource | null>(null);
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const running = status === 'running' || status === 'paused';

  const inspect = async (next: PackageSource) => {
    setBusy(true);
    setError(null);
    try {
      setPreview(await invoke<ImportPreview>('preview_flow_package', { source: next }));
      setSource(next);
      setAcknowledged(false);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const pickFile = async () => {
    const path = await open({ multiple: false, directory: false, filters: PACKAGE_FILTERS });
    if (typeof path === 'string') await inspect({ kind: 'file', path });
  };

  const doImport = async () => {
    if (!source) return;
    if (isDirty) {
      const ok = await confirm('Açık akıştaki kaydedilmemiş değişiklikler kaybolacak. Devam edilsin mi?', {
        title: 'Akışı değiştir',
        kind: 'warning',
      });
      if (!ok) return;
    }
    setBusy(true);
    setError(null);
    try {
      const flow = JSON.parse(await invoke<string>('import_flow_package', { source }));
      loadFlow(flow);
      // Not saved to disk yet
      useFlowStore.setState({ isDirty: true });
      useExecutionStore.getState().addLog(`"${flow.name}" içe aktarıldı`, 'info');
      onClose();
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  };

  const reset = () => {
    setPreview(null);
    setSource(null);
    setError(null);
  };

  const needsAck = preview?.risk === 'danger';

  return (
    <Modal
      title={<><FileUp size={16} /> Akış İçe Aktar</>}
      onClose={onClose}
      footer={
        preview ? (
          <>
            <button className="btn btn-ghost" onClick={reset} disabled={busy}>Geri</button>
            <button
              className="btn btn-primary"
              onClick={doImport}
              disabled={busy || running || (needsAck && !acknowledged)}
              title={running ? 'Önce çalışan akışı durdurun' : undefined}
            >
              İçe Aktar
            </button>
          </>
        ) : (
          <button className="btn btn-primary" onClick={() => inspect({ kind: 'code', code })} disabled={busy || !code.trim()}>
            Kodu İncele
          </button>
        )
      }
    >
      {!preview ? (
        <>
          <button className="btn btn-ghost share-pick" onClick={pickFile} disabled={busy}>
            <FolderOpen size={16} /> .synapse dosyası seç…
          </button>
          <div className="share-divider"><span>veya</span></div>
          <div>
            <label className="properties-panel__label" htmlFor="import-code">Paylaşım kodu</label>
            <textarea
              id="import-code"
              className="properties-panel__input share-code"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="SYN1:…"
              spellCheck={false}
            />
          </div>
        </>
      ) : (
        <>
          <div>
            <div className="share-preview__name">{preview.meta.name || preview.flowName}</div>
            <div className="share-hint">
              {preview.meta.author ? `${preview.meta.author} · ` : ''}
              {preview.createdAt ? new Date(preview.createdAt).toLocaleDateString('tr-TR') : 'tarih yok'}
              {preview.appVersion ? ` · Synapse ${preview.appVersion}` : ''}
            </div>
            {preview.meta.description && <p className="share-preview__description">{preview.meta.description}</p>}
            {preview.meta.tags.length > 0 && (
              <div className="share-tags">
                {preview.meta.tags.map((tag) => <span key={tag} className="share-tag">{tag}</span>)}
              </div>
            )}
          </div>

          <div className="share-stats">
            <Stat value={preview.nodeCount} label="düğüm" />
            <Stat value={preview.edgeCount} label="bağlantı" />
            <Stat value={preview.inputSteps} label="fare/klavye adımı" />
            <Stat value={preview.assetCount} label="şablon görseli" />
          </div>

          <div>
            <div className="share-section-title">
              {preview.findings.length === 0 ? <ShieldCheck size={14} /> : <ShieldAlert size={14} />}
              Güvenlik incelemesi
            </div>
            {preview.findings.length === 0 ? (
              <div className="modal__notice">
                Riskli adım bulunamadı: program çalıştırma, metin yazma veya Windows kısayolu yok.
              </div>
            ) : (
              <ul className="share-findings">
                {preview.findings.map((f, i) => {
                  const Icon = RiskIcon[f.risk];
                  return (
                    <li key={`${f.nodeId}-${i}`} className={`share-finding share-finding--${f.risk}`}>
                      <span className={`share-badge share-badge--${f.risk}`}>
                        <Icon size={11} /> {riskLabel[f.risk]}
                      </span>
                      <span className="share-finding__text">
                        <strong>{f.nodeLabel}</strong> — {f.message}
                      </span>
                    </li>
                  );
                })}
              </ul>
            )}
            <p className="share-hint">
              İçe aktarma akışı çalıştırmaz; akış editörde açılır, inceleyip kendiniz başlatırsınız.
              Yalnızca güvendiğiniz kişilerden gelen akışları çalıştırın.
            </p>
          </div>

          {needsAck && (
            <label className="share-ack">
              <input type="checkbox" checked={acknowledged} onChange={(e) => setAcknowledged(e.target.checked)} />
              Bu akışın bilgisayarımda program çalıştırabileceğini anladım.
            </label>
          )}
        </>
      )}

      {error && <div className="modal__error">{error}</div>}
    </Modal>
  );
}
