// ============================================================
// Synapse — Share Dialog
// ============================================================
// Exports the flow in the editor as a `.synapse` package file or
// a share code. Template images are embedded by the backend.
// ============================================================

import { useState } from 'react';
import { Share2, FileDown, Copy } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { Modal } from './Modal';
import { useFlowStore } from '../../stores/flowStore';
import { PACKAGE_FILTERS, type ShareMeta } from '../../types/sharing';

/** Share codes beyond this length are unwieldy to paste; suggest a file */
const LONG_CODE = 20_000;

function fileNameFor(name: string): string {
  const cleaned = name.replace(/[<>:"\/\\|?*\u0000-\u001f]/g, '').trim();
  return `${cleaned || 'akis'}.synapse`;
}

export function ShareDialog({ onClose }: { onClose: () => void }) {
  const flowName = useFlowStore((s) => s.flowName);
  const nodeCount = useFlowStore((s) => s.nodes.length);
  const toJSON = useFlowStore((s) => s.toJSON);

  const [name, setName] = useState(flowName);
  const [description, setDescription] = useState('');
  const [author, setAuthor] = useState('');
  const [tags, setTags] = useState('');
  const [code, setCode] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const meta = (): ShareMeta => ({
    name: name.trim(),
    description: description.trim(),
    author: author.trim(),
    tags: tags.split(',').map((t) => t.trim()).filter(Boolean),
  });

  const run = async (task: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await task();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const exportFile = () =>
    run(async () => {
      const path = await save({ defaultPath: fileNameFor(name), filters: PACKAGE_FILTERS });
      if (!path) return;
      await invoke('export_flow_package', { flowJson: JSON.stringify(toJSON()), meta: meta(), path });
      setNotice(`Kaydedildi: ${path}`);
    });

  const createCode = () =>
    run(async () => {
      setCode(await invoke<string>('create_share_code', { flowJson: JSON.stringify(toJSON()), meta: meta() }));
    });

  const copyCode = () =>
    run(async () => {
      if (!code) return;
      await navigator.clipboard.writeText(code);
      setNotice('Paylaşım kodu panoya kopyalandı');
    });

  const empty = nodeCount === 0;

  return (
    <Modal
      title={<><Share2 size={16} /> Akışı Paylaş</>}
      onClose={onClose}
      footer={
        <>
          <button className="btn btn-ghost" onClick={createCode} disabled={busy || empty}>
            <Copy size={14} /> Paylaşım Kodu
          </button>
          <button className="btn btn-primary" onClick={exportFile} disabled={busy || empty}>
            <FileDown size={14} /> Dosyaya Kaydet…
          </button>
        </>
      }
    >
      {empty && <div className="modal__error">Paylaşılacak düğüm yok. Önce akışa düğüm ekleyin.</div>}

      <p className="share-hint">
        Akış, kullandığı şablon görselleriyle birlikte tek bir pakete dönüştürülür. Paketi alan kişi içe
        aktarmadan önce akışın ne yaptığını görür.
      </p>

      <div>
        <label className="properties-panel__label" htmlFor="share-name">Ad</label>
        <input id="share-name" className="properties-panel__input" value={name} onChange={(e) => setName(e.target.value)} />
      </div>
      <div>
        <label className="properties-panel__label" htmlFor="share-description">Açıklama</label>
        <textarea
          id="share-description"
          className="properties-panel__input share-textarea"
          rows={3}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Bu akış ne yapar, nasıl kullanılır?"
        />
      </div>
      <div className="share-row">
        <div className="share-row__cell">
          <label className="properties-panel__label" htmlFor="share-author">Yazar</label>
          <input id="share-author" className="properties-panel__input" value={author} onChange={(e) => setAuthor(e.target.value)} />
        </div>
        <div className="share-row__cell">
          <label className="properties-panel__label" htmlFor="share-tags">Etiketler</label>
          <input
            id="share-tags"
            className="properties-panel__input"
            value={tags}
            onChange={(e) => setTags(e.target.value)}
            placeholder="ofis, veri girişi"
          />
        </div>
      </div>

      {code && (
        <div>
          <div className="share-code__header">
            <label className="properties-panel__label" htmlFor="share-code">Paylaşım kodu</label>
            <button className="btn btn-ghost share-code__copy" onClick={copyCode} disabled={busy}>
              <Copy size={12} /> Kopyala
            </button>
          </div>
          <textarea
            id="share-code"
            className="properties-panel__input share-code"
            readOnly
            value={code}
            onFocus={(e) => e.currentTarget.select()}
          />
          <p className="share-hint">
            {code.length.toLocaleString('tr-TR')} karakter
            {code.length > LONG_CODE && ' — kod uzun (büyük şablon görselleri); dosya olarak paylaşmak daha pratik olabilir.'}
          </p>
        </div>
      )}

      {error && <div className="modal__error">{error}</div>}
      {notice && <div className="modal__notice">{notice}</div>}
    </Modal>
  );
}
