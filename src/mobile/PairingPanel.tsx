import { useCallback, useState } from "react";
import {
  checkPermissions,
  requestPermissions,
  scan,
} from "@tauri-apps/plugin-barcode-scanner";

import { api, type MobilePairingPreview, type MobileSyncState } from "../api";
import { errorText } from "./mobileUtils";

function defaultDeviceName(): string {
  const match = navigator.userAgent.match(/Android\s([\d.]+)/i);
  return match ? `Android ${match[1]} 手机` : "Android 手机";
}

interface PairingPanelProps {
  state: MobileSyncState;
  onPaired: (state: MobileSyncState) => void;
  /** 已有本地数据时允许返回离线浏览 */
  onBack?: () => void;
}

/**
 * 配对页：扫码或手动粘贴配对信息，确认服务器后再完成配对。
 * 这里只负责建立连接，不提供任何任务业务操作。
 */
export function PairingPanel({ state, onPaired, onBack }: PairingPanelProps) {
  const [busy, setBusy] = useState<null | "scan" | "pair">(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ payload: string; info: MobilePairingPreview } | null>(
    null,
  );
  const [manual, setManual] = useState("");
  const [showManual, setShowManual] = useState(false);
  const [deviceName, setDeviceName] = useState(defaultDeviceName);

  const inspect = useCallback(async (payload: string) => {
    try {
      const info = await api.mobileInspectPairing(payload);
      setPreview({ payload, info });
      setError(null);
    } catch (e) {
      setPreview(null);
      setError(errorText(e));
    }
  }, []);

  const startScan = async () => {
    setError(null);
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
      const text = errorText(e);
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
    try {
      const next = await api.mobilePair(preview.payload, deviceName);
      setPreview(null);
      setManual("");
      onPaired(next);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="mobile-page">
      <header className="mobile-header">
        <div className="mobile-header-row">
          <h1>Local To Do</h1>
          {onBack && (
            <button className="mobile-text-button" onClick={onBack} disabled={busy !== null}>
              返回任务
            </button>
          )}
        </div>
        <p className="mobile-subtitle">手机只读伴侣 · 数据由 Windows 电脑提供</p>
      </header>

      {error && <div className="mobile-alert error">{error}</div>}
      {!error && state.error && <div className="mobile-alert error">{state.error}</div>}

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

      <footer className="mobile-footer">Android 端为只读伴侣，任务修改请回到 Windows 电脑。</footer>
    </div>
  );
}
