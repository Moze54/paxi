import { useEffect, useState } from "react";
import { useAppStore } from "../lib/store";
import {
  exportCaCert,
  getPassthroughHosts,
  setPassthroughHosts,
  getUpstreamProxy,
  setUpstreamProxy,
  UpstreamProxy,
} from "../lib/ipc";
import {
  ShieldCheck,
  Smartphone,
  Copy,
  Check,
  ArrowDownUp,
  Plus,
  X,
  Network,
} from "lucide-react";

interface SettingsProps {
  onClose: () => void;
}

export default function Settings({ onClose }: SettingsProps) {
  const { proxy } = useAppStore();
  const [msg, setMsg] = useState("");
  const [copied, setCopied] = useState(false);

  // TLS 直通列表
  const [ptHosts, setPtHosts] = useState<string[]>([]);
  const [ptInput, setPtInput] = useState("");
  const [ptSaved, setPtSaved] = useState(false);

  // 上游代理
  const [upstream, setUpstream] = useState<UpstreamProxy>({
    enabled: false,
    host: "",
    port: 8080,
    username: "",
    password: "",
  });
  const [upstreamSaved, setUpstreamSaved] = useState(false);

  useEffect(() => {
    getPassthroughHosts()
      .then(setPtHosts)
      .catch(() => {});
    getUpstreamProxy()
      .then(setUpstream)
      .catch(() => {});
  }, []);

  const saveUpstream = async () => {
    try {
      await setUpstreamProxy(upstream);
      setUpstreamSaved(true);
      setTimeout(() => setUpstreamSaved(false), 1200);
    } catch (e) {
      alert(String(e));
    }
  };

  const handleExportCert = async () => {
    try {
      const path = await exportCaCert();
      setMsg(`根证书已导出到 ${path}`);
    } catch (e) {
      setMsg(`导出失败：${e}`);
    }
  };

  const savePtHosts = async (next: string[]) => {
    setPtHosts(next);
    try {
      await setPassthroughHosts(next);
      setPtSaved(true);
      setTimeout(() => setPtSaved(false), 1200);
    } catch {
      /* 忽略 */
    }
  };

  const addPtHost = () => {
    const v = ptInput.trim();
    if (!v || ptHosts.includes(v)) return;
    setPtInput("");
    savePtHosts([...ptHosts, v]);
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
              <ArrowDownUp size={16} /> TLS 直通（不解密）
            </h4>
            <p className="hint">
              命中列表的域名不做中间人解密，直接隧道转发。适用于：
            </p>
            <ul className="pt-reasons">
              <li>App 做了证书校验（SSL Pinning），MITM 会导致其断网</li>
              <li>银行/支付类 App 检测到代理后拒绝工作</li>
              <li>微信主应用（小程序流量通常仍可解密，主应用可用直通兜底）</li>
            </ul>
            <div className="pt-editor">
              <div className="pt-list">
                {ptHosts.length === 0 && (
                  <p className="hint">（空，支持 glob 通配，如 <code>*.wechat.com</code>、<code>192.168.*</code>）</p>
                )}
                {ptHosts.map((h) => (
                  <div key={h} className="pt-item">
                    <code>{h}</code>
                    <button
                      className="btn btn-icon btn-mini"
                      onClick={() => savePtHosts(ptHosts.filter((x) => x !== h))}
                    >
                      <X size={12} />
                    </button>
                  </div>
                ))}
              </div>
              <div className="pt-add">
                <input
                  className="mono-input-flat"
                  placeholder="*.example.com"
                  value={ptInput}
                  onChange={(e) => setPtInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") addPtHost();
                  }}
                />
                <button className="btn btn-ghost" onClick={addPtHost}>
                  <Plus size={14} /> 添加
                </button>
                {ptSaved && (
                  <span className="pt-saved">
                    <Check size={13} /> 已保存
                  </span>
                )}
              </div>
            </div>
          </section>

          <section>
            <h4>
              <Network size={16} /> 上游代理链
            </h4>
            <p className="hint">
              企业/公司网络需要经过二级代理才能访问外网时，在此配置。启用后所有
              转发与重放请求都会先发给该上游代理（HTTP 代理协议），支持基本认证。
            </p>
            <div className="up-editor">
              <label className="up-row">
                <input
                  type="checkbox"
                  checked={upstream.enabled}
                  onChange={(e) => setUpstream({ ...upstream, enabled: e.target.checked })}
                />
                启用上游代理
              </label>
              <div className="up-row">
                <input
                  className="mono-input-flat"
                  placeholder="代理地址 host"
                  value={upstream.host}
                  onChange={(e) => setUpstream({ ...upstream, host: e.target.value })}
                />
                <input
                  className="mono-input-flat up-port"
                  type="number"
                  placeholder="端口"
                  value={upstream.port || ""}
                  onChange={(e) => setUpstream({ ...upstream, port: Number(e.target.value) || 0 })}
                />
              </div>
              <div className="up-row">
                <input
                  className="mono-input-flat"
                  placeholder="用户名（可选）"
                  value={upstream.username}
                  onChange={(e) => setUpstream({ ...upstream, username: e.target.value })}
                />
                <input
                  className="mono-input-flat"
                  type="password"
                  placeholder="密码（可选）"
                  value={upstream.password}
                  onChange={(e) => setUpstream({ ...upstream, password: e.target.value })}
                />
              </div>
              <div className="up-row">
                <button className="btn btn-primary" onClick={saveUpstream}>
                  {upstreamSaved ? <Check size={14} /> : <Network size={14} />}
                  {upstreamSaved ? "已保存" : "保存配置"}
                </button>
              </div>
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
