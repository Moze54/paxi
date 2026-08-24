import { useState } from "react";
import { useAppStore } from "../lib/store";
import { exportCaCert } from "../lib/ipc";
import { ShieldCheck, Smartphone, Copy, Check } from "lucide-react";

interface SettingsProps {
  onClose: () => void;
}

export default function Settings({ onClose }: SettingsProps) {
  const { proxy } = useAppStore();
  const [msg, setMsg] = useState("");
  const [copied, setCopied] = useState(false);

  const handleExportCert = async () => {
    try {
      const path = await exportCaCert();
      setMsg(`根证书已导出到 ${path}`);
    } catch (e) {
      setMsg(`导出失败：${e}`);
    }
  };

  const proxyAddress = `${proxy.local_ip || "127.0.0.1"}:${proxy.port}`;

  const copyAddress = async () => {
    try {
      await navigator.clipboard.writeText(proxyAddress);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {}
  };

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h3>设置</h3>
          <button className="btn btn-icon" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="settings-body">
          <section>
            <h4>
              <ShieldCheck size={16} /> HTTPS 证书
            </h4>
            <p className="hint">
              要解密 HTTPS 流量，需要安装并信任 paxi 的根证书（CA）。
            </p>
            <button className="btn btn-primary" onClick={handleExportCert}>
              导出根证书
            </button>
            {msg && <p className="msg">{msg}</p>}
            <div className="install-steps">
              <p><strong>Windows 安装：</strong>双击导出的 .crt 文件 → "安装证书" → "本地计算机" → "受信任的根证书颁发机构"</p>
              <p><strong>macOS 安装：</strong>双击 .crt → 钥匙串访问 → 选择证书 → "始终信任"</p>
            </div>
          </section>

          <section>
            <h4>
              <Smartphone size={16} /> 连接手机
            </h4>
            <p className="hint">
              手机与电脑连接同一 Wi-Fi，然后在手机 Wi-Fi 设置中配置代理：
            </p>
            <div className="proxy-address">
              <code>{proxyAddress}</code>
              <button className="btn btn-ghost" onClick={copyAddress}>
                {copied ? <Check size={14} /> : <Copy size={14} />}
                {copied ? "已复制" : "复制"}
              </button>
            </div>
            <div className="install-steps">
              <p><strong>iPhone：</strong>设置 → Wi-Fi → 当前网络 → 配置代理 → 手动 → 填入地址</p>
              <p><strong>Android：</strong>设置 → WLAN → 长按当前网络 → 修改网络 → 高级 → 手动代理 → 填入地址</p>
              <p>随后用手机浏览器访问 <code>http://{proxy.local_ip || "电脑IP"}:{proxy.port}</code> 下载并安装证书</p>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
