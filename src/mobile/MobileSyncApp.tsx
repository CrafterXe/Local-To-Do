import { useCallback, useEffect, useRef, useState } from "react";
import {
  checkPermissions,
  requestPermissions,
  scan,
} from "@tauri-apps/plugin-barcode-scanner";

import { api, type MobilePairingPreview, type MobileSyncState } from "../api";

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

function defaultDeviceName(): string {
  const match = navigator.userAgent.match(/Android\s([\d.]+)/i);
  return match ? `Android ${match[1]} 手机` : "Android 手机";
}

function formatTime(ms: number | null): string {
  if (!ms) return "尚未同步";
  return new Date(ms).toLocaleString("zh-CN", { hour12: false });
}

function shortFingerprint(fingerprint: string): string {
  if (fingerprint.length <= 23) return fingerprint;
  return `${fingerprint.slice(0, 11)}…${fingerprint.slice(-8)}`;
}

/**
 * Android 只读同步界面：仅用于配对、查看同步状态与手动同步。
 * 不提供任何任务新建/编辑/完成/删除入口。
 */
export function MobileSyncApp() {
  const [state, setState] = useState<MobileSyncState | null>(null);
  const [busy, setBusy] = useState<null | "scan" | "pair" | "sync" | "unpair">(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ payload: string; info: MobilePairingPreview } | null>(
    null,
  );
  const [manual, setManual] = useState("");
  const [showManual, setShowManual] = useState(false);
  const [deviceName, setDeviceName] = useState(defaultDeviceName);
  const [confirmUnpair, setConfirmUnpair] = useState(false);
  const autoSynced = useRef(false);

  const refresh = useCallback(async () => {
    try {
      setState(await api.mobileSyncState());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const runSync = useCallback(async () => {
    setBusy("sync");
    setError(null);
    setNotice(null);
    try {
      const next = await api.mobileSyncNow();
      setState(next);
      if (next.phase === "synced") setNotice("同步完成");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
      void refresh();
    }
  }, [refresh]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // 应用启动（或刚配对完成）时自动同步一次
  useEffect(() => {
    if (!state?.paired || autoSynced.current) return;
    autoSynced.current = true;
    void runSync();
  }, [state?.paired, runSync]);

  const inspect = useCallback(async (payload: string) => {
    try {
      const info = await api.mobileInspectPairing(payload);
      setPreview({ payload, info });
      setError(null);
    } catch (e) {
      setPreview(null);
      setError(String(e));
    }
  }, []);

  const startScan = async () => {
    setError(null);
    setNotice(null);
    setShowManual(false);
    setBusy("scan");
    try {
      const permission = await checkPermissions();
      if (permission !== "granted") {
        const granted = await requestPermissions();
        if (granted !== "granted") {
          throw new Error("未授予相机权限，无法扫描二维码");
        }
      }
      const result = await scan({ windowed: false });
      if (result?.content) {
        await inspect(result.content);
      }
    } catch (e) {
      const text = String(e);
      if (!/cancel|取消/i.test(text)) {
        setError(`扫码失败：${text}`);
      }
    } finally {
      setBusy(null);
    }
  };

  const confirmPair = async () => {
    if (!preview) return;
    setBusy("pair");
    setError(null);
    setNotice(null);
    try {
      const next = await api.mobilePair(preview.payload, deviceName);
      setState(next);
      setPreview(null);
      setManual("");
      setNotice("配对成功，正在获取数据…");
      autoSynced.current = false;
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const doUnpair = async () => {
    setBusy("unpair");
    setError(null);
    setNotice(null);
    try {
      const next = await api.mobileUnpair();
      setState(next);
      setConfirmUnpair(false);
      autoSynced.current = false;
      setNotice("已解除配对，本地数据将保留供离线查看");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const syncing = busy === "sync" || state?.phase === "firstSync" || state?.phase === "syncing";
  const phase = state?.phase ?? "unpaired";

  return (
    <div className="mobile-page">
      <header className="mobile-header">
        <h1>Local To Do</h1>
        <p className="mobile-subtitle">手机只读同步 · 数据由 Windows 电脑提供</p>
      </header>

      {error && <div className="mobile-alert error">{error}</div>}
      {!error && state?.error && <div className="mobile-alert error">{state.error}</div>}
      {notice && <div className="mobile-alert">{notice}</div>}

      {!state?.paired && (
        <section className="mobile-card">
          <h2>连接电脑</h2>
          <p className="mobile-hint">
            在 Windows 的「设置 → 局域网同步」中开启服务并生成配对二维码，然后用手机扫描。
          </p>
          <button
            className="mobile-button primary"
            onClick={() => void startScan()}
            disabled={busy !== null}
          >
            {busy === "scan" ? "正在打开相机…" : "扫描配对二维码"}
          </button>
          <button
            className="mobile-button"
            onClick={() => setShowManual((value) => !value)}
            disabled={busy !== null}
          >
            {showManual ? "收起手动输入" : "手动粘贴配对信息"}
          </button>

          {showManual && (
            <div className="mobile-manual">
              <textarea
                className="mobile-textarea"
                placeholder="粘贴电脑上显示的配对信息（含地址、端口、证书指纹和配对码）"
                value={manual}
                onChange={(event) => setManual(event.target.value)}
                rows={4}
              />
              <button
                className="mobile-button"
                onClick={() => void inspect(manual.trim())}
                disabled={!manual.trim() || busy !== null}
              >
                校验配对信息
              </button>
            </div>
          )}

          {preview && (
            <div className="mobile-preview">
              <h3>确认服务器</h3>
              <dl className="mobile-kv">
                <dt>地址</dt>
                <dd>
                  {preview.info.host}:{preview.info.port}
                </dd>
                <dt>证书指纹</dt>
                <dd className="mobile-mono">{preview.info.fingerprint}</dd>
                <dt>协议版本</dt>
                <dd>{preview.info.protocolVersion}</dd>
              </dl>
              {preview.info.expired && (
                <p className="mobile-warning">二维码已过期，请在电脑上重新生成。</p>
              )}
              <label className="mobile-label" htmlFor="mobile-device-name">
                设备名称
              </label>
              <input
                id="mobile-device-name"
                className="mobile-input"
                value={deviceName}
                maxLength={64}
                onChange={(event) => setDeviceName(event.target.value)}
              />
              <div className="mobile-actions">
                <button
                  className="mobile-button primary"
                  onClick={() => void confirmPair()}
                  disabled={preview.info.expired || busy !== null}
                >
                  {busy === "pair" ? "配对中…" : "确认配对"}
                </button>
                <button
                  className="mobile-button"
                  onClick={() => setPreview(null)}
                  disabled={busy !== null}
                >
                  取消
                </button>
              </div>
            </div>
          )}
        </section>
      )}

      {state?.paired && (
        <>
          <section className="mobile-card">
            <div className="mobile-status-row">
              <h2>同步状态</h2>
              <span className={`mobile-badge phase-${phase}`}>{PHASE_LABEL[phase] ?? phase}</span>
            </div>
            {syncing && (
              <div className="mobile-progress">
                <span />
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
            {state.needsSnapshot && (
              <p className="mobile-warning">本地镜像已清空，需要重新获取完整快照。</p>
            )}
            <button
              className="mobile-button primary"
              onClick={() => void runSync()}
              disabled={busy !== null}
            >
              {syncing ? "同步中…" : "立即同步"}
            </button>
          </section>

          <section className="mobile-card">
            <h2>已连接的电脑</h2>
            <dl className="mobile-kv">
              <dt>地址</dt>
              <dd>
                {state.connection?.host}:{state.connection?.port}
              </dd>
              <dt>设备</dt>
              <dd>{state.connection?.deviceName}</dd>
              <dt>证书指纹</dt>
              <dd className="mobile-mono">
                {shortFingerprint(state.connection?.fingerprint ?? "")}
              </dd>
              <dt>配对时间</dt>
              <dd>{formatTime(state.connection?.pairedAt ?? null)}</dd>
            </dl>
            {confirmUnpair ? (
              <div className="mobile-actions">
                <button
                  className="mobile-button danger"
                  onClick={() => void doUnpair()}
                  disabled={busy !== null}
                >
                  {busy === "unpair" ? "解除中…" : "确认解除配对"}
                </button>
                <button
                  className="mobile-button"
                  onClick={() => setConfirmUnpair(false)}
                  disabled={busy !== null}
                >
                  取消
                </button>
              </div>
            ) : (
              <button
                className="mobile-button"
                onClick={() => setConfirmUnpair(true)}
                disabled={busy !== null}
              >
                解除配对
              </button>
            )}
            <p className="mobile-hint">
              解除配对只会删除设备令牌与连接信息，最近一次同步的数据仍保留在本机。
            </p>
          </section>
        </>
      )}

      <footer className="mobile-footer">Android 端为只读伴侣，任务修改请回到 Windows 电脑。</footer>
    </div>
  );
}
