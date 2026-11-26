import { useState } from "react";

import type { MobileSyncState } from "../api";
import { IconRefresh, IconX } from "../components/Icons";

const PHASE_LABEL: Record<string, string> = {
  unpaired: "未配对",
  pairing: "配对中",
  paired: "已配对",
  firstSync: "首次同步中",
  syncing: "同步中",
  synced: "同步成功",
  offline: "离线",
  failed: "同步失败",
  needsPairing: "需要重新配对",
};

function formatTime(ms: number | null): string {
  if (!ms) return "尚未同步";
  return new Date(ms).toLocaleString("zh-CN", { hour12: false });
}

function shortFingerprint(fingerprint: string): string {
  if (fingerprint.length <= 23) return fingerprint;
  return `${fingerprint.slice(0, 11)}…${fingerprint.slice(-8)}`;
}

interface MobileSyncSheetProps {
  state: MobileSyncState;
  busy: boolean;
  onClose: () => void;
  onSync: () => void;
  onUnpair: () => void;
  onRepair: () => void;
}

/** 同步状态面板：状态、手动同步、连接信息与解除配对 */
export function MobileSyncSheet({
  state,
  busy,
  onClose,
  onSync,
  onUnpair,
  onRepair,
}: MobileSyncSheetProps) {
  const [confirmUnpair, setConfirmUnpair] = useState(false);
  const phase = state.phase;
  const syncing = busy || phase === "firstSync" || phase === "syncing";

  return (
    <div className="mobile-sheet" role="dialog" aria-modal="true">
      <div className="mobile-sheet-backdrop" onClick={onClose} />
      <div className="mobile-sheet-panel">
        <header className="mobile-sheet-header">
          <h2>同步状态</h2>
          <button className="mobile-icon-button" onClick={onClose} aria-label="关闭">
            <IconX size={20} />
          </button>
        </header>

        <div className="mobile-sheet-body">
          <div className="mobile-status-row">
            <span className={`mobile-badge phase-${phase}`}>{PHASE_LABEL[phase] ?? phase}</span>
            {state.needsSnapshot && <span className="mobile-warning">需要重新获取快照</span>}
          </div>
          {syncing && (
            <div className="mobile-progress">
              <span />
            </div>
          )}

          {(state.error || (phase === "needsPairing" && !state.error)) && (
            <div className="mobile-alert error">
              {state.error ?? "设备令牌已失效，请重新扫码配对。"}
            </div>
          )}

          <dl className="mobile-kv">
            <dt>上次同步</dt>
            <dd>{formatTime(state.lastSyncAt)}</dd>
            <dt>数据量</dt>
            <dd>
              {state.counts.tasks} 个任务 · {state.counts.lists} 个清单
            </dd>
            <dt>已应用版本</dt>
            <dd>rev {state.lastRev}</dd>
          </dl>

          <button className="mobile-button primary" onClick={onSync} disabled={busy}>
            <IconRefresh size={16} /> {syncing ? "同步中…" : "立即同步"}
          </button>

          {state.connection && (
            <section className="mobile-sheet-section">
              <h3>已连接的电脑</h3>
              <dl className="mobile-kv">
                <dt>地址</dt>
                <dd>
                  {state.connection.host}:{state.connection.port}
                </dd>
                <dt>设备</dt>
                <dd>{state.connection.deviceName}</dd>
                <dt>证书指纹</dt>
                <dd className="mobile-mono">{shortFingerprint(state.connection.fingerprint)}</dd>
                <dt>配对时间</dt>
                <dd>{formatTime(state.connection.pairedAt)}</dd>
              </dl>
            </section>
          )}

          {state.paired ? (
            confirmUnpair ? (
              <div className="mobile-actions">
                <button
                  className="mobile-button danger"
                  onClick={() => {
                    onUnpair();
                    setConfirmUnpair(false);
                  }}
                  disabled={busy}
                >
                  确认解除配对
                </button>
                <button
                  className="mobile-button"
                  onClick={() => setConfirmUnpair(false)}
                  disabled={busy}
                >
                  取消
                </button>
              </div>
            ) : (
              <button
                className="mobile-button"
                onClick={() => setConfirmUnpair(true)}
                disabled={busy}
              >
                解除配对
              </button>
            )
          ) : (
            <button className="mobile-button" onClick={onRepair} disabled={busy}>
              重新配对电脑
            </button>
          )}

          <p className="mobile-hint">
            解除配对只删除设备令牌与连接信息，最近一次同步的数据仍保留在本机，可继续离线查看。
          </p>
        </div>
      </div>
    </div>
  );
}
