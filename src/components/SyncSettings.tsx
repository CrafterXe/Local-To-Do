import { useCallback, useEffect, useState } from "react";
import { api, type PairingInfo, type SyncDevice, type SyncStatus } from "../api";
import { formatTimestamp } from "../utils/date";
import { ConfirmModal } from "./Modal";

interface Props {
  notify: (msg: string, error?: boolean) => void;
}

/** 设置页中的「局域网同步」区块：开关、状态、配对二维码与设备管理 */
export function SyncSettings({ notify }: Props) {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [devices, setDevices] = useState<SyncDevice[]>([]);
  const [pairing, setPairing] = useState<PairingInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [revoking, setRevoking] = useState<SyncDevice | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const next = await api.syncServerStatus();
      setStatus(next);
      if (!next.running || !next.pairingActive) {
        setPairing(null);
      }
      setDevices(await api.syncListDevices());
    } catch {
      // 轮询失败不打扰用户，保留上一次状态
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 3000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const toggle = async () => {
    if (!status) return;
    setBusy(true);
    try {
      const next = status.running ? await api.syncServerStop() : await api.syncServerStart();
      setStatus(next);
      setPairing(null);
      await refresh();
      notify(next.running ? "局域网同步已开启" : "局域网同步已关闭");
    } catch (e) {
      notify(`操作失败：${String(e)}`, true);
    } finally {
      setBusy(false);
    }
  };

  const createPairing = async () => {
    try {
      setPairing(await api.syncCreatePairing());
    } catch (e) {
      notify(`无法生成配对码：${String(e)}`, true);
    }
  };

  const cancelPairing = async () => {
    try {
      await api.syncCancelPairing();
      setPairing(null);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const revoke = async (device: SyncDevice) => {
    try {
      setDevices(await api.syncRevokeDevice(device.id));
      setRevoking(null);
      notify("已撤销该设备授权，其令牌立即失效");
    } catch (e) {
      notify(`撤销失败：${String(e)}`, true);
    }
  };

  const resetPairing = async () => {
    try {
      const next = await api.syncResetPairing();
      setStatus(next);
      setPairing(null);
      setDevices(await api.syncListDevices());
      setConfirmReset(false);
      notify("已重置配对并更换证书，手机需要重新扫码");
    } catch (e) {
      notify(`重置失败：${String(e)}`, true);
    }
  };

  return (
    <section className="settings-section">
      <h2 className="settings-title">局域网同步</h2>
      <p className="settings-desc">
        Windows 作为唯一数据源，手机在同一 Wi-Fi 下通过 HTTPS 只读同步。首次使用需在手机端扫描配对二维码。
      </p>

      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-name">同步服务</div>
          <div className="setting-hint">
            {status
              ? status.running
                ? `运行中：https://${status.host}:${status.port}`
                : "已停止"
              : "读取状态中…"}
          </div>
          {status?.error && <div className="setting-hint missing">{status.error}</div>}
        </div>
        <button className="btn" disabled={busy || !status} onClick={() => void toggle()}>
          {status?.running ? "关闭" : "开启"}
        </button>
      </div>

      {status?.running && (
        <>
          <div className="setting-row" style={{ flexDirection: "column", alignItems: "stretch" }}>
            <div className="setting-text">
              <div className="setting-name">证书指纹（SHA-256）</div>
              <div className="sync-fingerprint">{status.fingerprint}</div>
            </div>
          </div>

          <div style={{ display: "flex", gap: 8, flexWrap: "wrap", margin: "10px 0" }}>
            <button className="btn" onClick={() => void createPairing()}>
              生成配对二维码
            </button>
            {pairing && (
              <button className="btn" onClick={() => void cancelPairing()}>
                取消配对码
              </button>
            )}
          </div>

          {pairing && (
            <div className="sync-pair">
              <div className="sync-qr" dangerouslySetInnerHTML={{ __html: pairing.qrSvg }} />
              <div className="setting-text">
                <div className="setting-name">
                  配对码：<span className="sync-code">{pairing.code}</span>
                </div>
                <div className="setting-hint">
                  有效期至 {formatTimestamp(pairing.expiresAt)}，仅可使用一次。
                </div>
                <div className="setting-hint">请让手机与电脑连接同一 Wi-Fi 后扫码。</div>
              </div>
            </div>
          )}

          <div className="section-label">已配对设备</div>
          {devices.length === 0 ? (
            <div style={{ fontSize: 12, color: "var(--muted-2)", padding: "8px 4px" }}>
              暂无已配对设备，点击「生成配对二维码」开始配对。
            </div>
          ) : (
            <div>
              {devices.map((device) => (
                <div key={device.id} className="backup-row">
                  <span className="name">{device.name}</span>
                  <span className="size">
                    {device.lastSeenAt ? `最后同步 ${formatTimestamp(device.lastSeenAt)}` : "尚未同步"}
                  </span>
                  <button className="btn small" onClick={() => setRevoking(device)}>
                    撤销
                  </button>
                </div>
              ))}
            </div>
          )}

          <div className="settings-inline-hint">{status.firewallHint}</div>

          <div style={{ marginTop: 12 }}>
            <button className="btn small" onClick={() => setConfirmReset(true)}>
              重置配对（更换证书）
            </button>
          </div>
        </>
      )}

      {revoking && (
        <ConfirmModal
          title="撤销设备授权"
          message={`撤销「${revoking.name}」后，其同步令牌立即失效，需要重新扫码配对才能继续同步。`}
          actions={[
            {
              label: "撤销授权",
              kind: "danger",
              onClick: () => revoke(revoking),
            },
          ]}
          onClose={() => setRevoking(null)}
        />
      )}

      {confirmReset && (
        <ConfirmModal
          title="重置同步配对"
          message="将清除全部已配对设备并更换 HTTPS 证书，所有手机都需要重新扫码配对。任务数据不会受到影响。"
          actions={[
            {
              label: "重置配对",
              kind: "danger",
              onClick: resetPairing,
            },
          ]}
          onClose={() => setConfirmReset(false)}
        />
      )}
    </section>
  );
}
