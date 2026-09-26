import { useState } from 'react';
import {
  Play, Pause, Square, Save, FolderOpen, FilePlus,
  Undo, Redo, Settings, Zap, Share2, FileUp,
} from 'lucide-react';
import { useFlowStore } from '../../stores/flowStore';
import { useExecutionStore } from '../../stores/executionStore';
import { invoke } from '@tauri-apps/api/core';
import { ShareDialog } from '../sharing/ShareDialog';
import { ImportDialog } from '../sharing/ImportDialog';

export function Toolbar() {
  const flowName = useFlowStore((s) => s.flowName);
  const isDirty = useFlowStore((s) => s.isDirty);
  const newFlow = useFlowStore((s) => s.newFlow);
  const toJSON = useFlowStore((s) => s.toJSON);
  const status = useExecutionStore((s) => s.status);
  const [dialog, setDialog] = useState<'share' | 'import' | null>(null);

  const handleSave = async () => {
    try {
      const flowData = toJSON();
      await invoke('save_flow', { flow: JSON.stringify(flowData) });
    } catch (err) { console.error('Save failed:', err); }
  };

  const handlePlay = async () => {
    try {
      if (status === 'paused') {
        await invoke('resume_execution');
      } else {
        const flowData = toJSON();
        const flowJson = JSON.stringify(flowData);
        await invoke('execute_flow', { flowJson });
      }
    } catch (err) {
      console.error('Execute failed:', err);
      useExecutionStore.getState().addLog(`Execute failed: ${err}`, 'error');
    }
  };

  const handlePause = async () => {
    try {
      await invoke('pause_execution');
    } catch (err) { console.error('Pause failed:', err); }
  };

  const handleStop = async () => {
    try {
      await invoke('stop_execution');
    } catch (err) { console.error('Stop failed:', err); }
  };

  return (
    <header className="toolbar" id="toolbar">
      <div className="toolbar__brand">
        <Zap className="toolbar__brand-icon" size={20} />
        <span className="toolbar__brand-name">Synapse</span>
      </div>
      <div className="toolbar__divider" />
      <div className="toolbar__group">
        <button className="btn btn-icon btn-ghost tooltip" onClick={newFlow} data-tooltip="Yeni Flow" id="btn-new-flow"><FilePlus size={16} /></button>
        <button className="btn btn-icon btn-ghost tooltip" data-tooltip="Aç" id="btn-open-flow"><FolderOpen size={16} /></button>
        <button className="btn btn-icon btn-ghost tooltip" onClick={handleSave} data-tooltip="Kaydet" id="btn-save-flow"><Save size={16} /></button>
      </div>
      <div className="toolbar__divider" />
      <div className="toolbar__group">
        <button className="btn btn-icon btn-ghost tooltip" onClick={() => setDialog('import')} data-tooltip="İçe Aktar" id="btn-import-flow"><FileUp size={16} /></button>
        <button className="btn btn-icon btn-ghost tooltip" onClick={() => setDialog('share')} data-tooltip="Paylaş" id="btn-share-flow"><Share2 size={16} /></button>
      </div>
      <div className="toolbar__divider" />
      <div className="toolbar__group">
        <button className="btn btn-icon btn-ghost tooltip" data-tooltip="Geri Al" id="btn-undo"><Undo size={16} /></button>
        <button className="btn btn-icon btn-ghost tooltip" data-tooltip="Yinele" id="btn-redo"><Redo size={16} /></button>
      </div>
      <div className="toolbar__divider" />
      <div className="toolbar__group">
        {status === 'running' ? (
          <button className="btn btn-icon btn-ghost tooltip" onClick={handlePause} data-tooltip="Duraklat" id="btn-pause"><Pause size={16} /></button>
        ) : (
          <button className="btn btn-success tooltip" onClick={handlePlay} data-tooltip={status === 'paused' ? 'Devam Et' : 'Çalıştır'} id="btn-play"><Play size={14} /><span>{status === 'paused' ? 'Devam' : 'Çalıştır'}</span></button>
        )}
        <button className="btn btn-icon btn-danger tooltip" onClick={handleStop} disabled={status === 'idle' || status === 'stopped'} data-tooltip="Durdur" id="btn-stop"><Square size={14} /></button>
      </div>
      <div className="toolbar__spacer" />
      <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
        <span style={{ fontSize: 'var(--font-size-sm)', fontWeight: 500, color: 'var(--text-secondary)' }}>{flowName}</span>
        {isDirty && <span style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--color-accent-amber)' }} />}
      </div>
      <div className="toolbar__spacer" />
      <button className="btn btn-icon btn-ghost tooltip" data-tooltip="Ayarlar" id="btn-settings"><Settings size={16} /></button>
      {dialog === 'share' && <ShareDialog onClose={() => setDialog(null)} />}
      {dialog === 'import' && <ImportDialog onClose={() => setDialog(null)} />}
    </header>
  );
}
