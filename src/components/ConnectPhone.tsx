import { useState, useMemo } from "react";
import { useAppStore } from "../lib/store";
import { QRCodeSVG } from "qrcode.react";
import { Copy, Check, Smartphone, X } from "lucide-react";

interface ConnectPhoneProps {
  onClose: () => void;
}

export default function ConnectPhone({ onClose }: ConnectPhoneProps) {
  const { proxy } = useAppStore();
  const [copied, setCopied] = useState(false);
  const [copiedPortal, setCopiedPortal] = useState(false);

  const ip = proxy.local_ip || "127.0.0.1";
  const port = proxy.port || 8888;
  const proxyAddress = `${ip}:${port}`;
  const portalUrl = `http://${ip}:${port}/`;

  const qrValue = useMemo(() => portalUrl, [portalUrl]);

  const copy = async (text: string, which: "addr" | "portal") => {
    try {
      await navigator.clipboard.writeText(text);
      if (which === "addr") {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      } else {
        setCopiedPortal(true);
        setTimeout(() => setCopiedPortal(false), 1500);
      }
    } catch {
      /* 剪贴板不可用时忽略 */
    }
  };

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-panel connect-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h3>
            <Smartphone size={16} /> 连接手机
          </h3>
          <button className="btn btn-icon" onClick={onClose}>
            <X size={14} />
          </button>
        </div>

        <div className="settings-body">
          {!proxy.running && (
            <div className="portal-warning">
              代理未启动，请先启动代理再连接手机。
            </div>
          )}

          <section className="qr-section">
            <div className="qr-wrap">
              <QRCodeSVG value={qrValue} size={172} marginSize={1} />
            </div>
            <p className="hint center">
              手机与电脑连接<b>同一 Wi-Fi</b>，用相机/微信扫一扫打开证书安装页
            </p>
            <div className="portal-url-row">
              <code>{portalUrl}</code>
              <button
                className="btn btn-ghost"
                onClick={() => copy(portalUrl, "portal")}
              >
                {copiedPortal ? <Check size={14} /> : <Copy size={14} />}
                {copiedPortal ? "已复制" : "复制"}
              </button>
            </div>
          </section>

          <section>
            <h4>第 1 步 · 配置 Wi-Fi 代理</h4>
            <div className="proxy-address">
              <code>{proxyAddress}</code>
              <button className="btn btn-ghost" onClick={() => copy(proxyAddress, "addr")}>
                {copied ? <Check size={14} /> : <Copy size={14} />}
                {copied ? "已复制" : "复制"}
              </button>
            </div>
            <div className="install-steps">
              <p>
                <strong>iPhone：</strong>设置 → Wi-Fi → 点击当前网络 → 配置代理 →
                手动 → 服务器填 <code>{ip}</code>，端口填 <code>{port}</code> → 存储
              </p>
              <p>
                <strong>Android：</strong>设置 → WLAN → 长按当前网络 → 修改网络 →
                高级选项 → 代理手动 → 填入 <code>{proxyAddress}</code>
              </p>
            </div>
          </section>

          <section>
            <h4>第 2 步 · 安装证书</h4>
            <p className="hint">
              扫上方二维码，或手机浏览器访问 <code>{portalUrl}</code>：
            </p>
            <div className="install-steps">
              <p>
                <strong>iOS：</strong>下载描述文件 → 设置中安装 →
                <b>关于本机 → 证书信任设置</b>开启完全信任（关键，别漏）
              </p>
              <p>
                <strong>Android：</strong>下载 .crt → 设置 → 安全 →
                安装证书 → CA 证书
              </p>
              <p className="dim">
                提示：Android 7+ 的 App 默认不信任用户证书，仅浏览器流量可解密；
                抓 App 需要应用支持或使用模拟器。
              </p>
            </div>
          </section>

          <section>
            <h4>第 3 步 · 验证</h4>
            <p className="hint">
              门户页底部点击「测试连接」，或直接打开任意 App/网页，本列表将实时显示手机流量。
            </p>
          </section>

          <section>
            <h4>抓小程序 / App 的技巧</h4>
            <div className="install-steps">
              <p>
                <strong>微信小程序：</strong>iOS 需在「设置 → 通用 →
                关于本机 → 证书信任设置」开启完全信任；Android 7+
                微信不信任用户证书，建议用 iOS 设备或 Android 模拟器（可将系统证书目录改为可写）抓小程序。
              </p>
              <p>
                <strong>App 断网 / 抓不到：</strong>多为 App 做了证书校验（SSL
                Pinning）。到「设置 → TLS 直通」把该 App 的域名加入直通列表，
                让这些域名走隧道转发——App 恢复可用，其余域名照常解密。
              </p>
              <p>
                <strong>IP 直连：</strong>部分框架（如微信 mars）直接连
                IP，paxi 已支持为 IP 签发证书，正常可见。
              </p>
              <p className="dim">
                若列表出现「TLS 握手失败」记录，即证书未被信任或命中
                Pinning，按提示处理即可。
              </p>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
