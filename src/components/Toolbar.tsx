import { useAppStore } from "../lib/store";
import { Play, Square, Trash2, Settings, Wifi } from "lucide-react";

interface ToolbarProps {
  onOpenSettings: () => void;
}

const METHOD_OPTIONS = ["", "GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"];
const STATUS_OPTIONS = [
  { value: "", label: "全部状态" },
  { value: "2xx", label: "2xx 成功" },
  { value: "3xx", label: "3xx 重定向" },
  { value: "4xx", label: "4xx 客户端错误" },
  { value: "5xx", label: "5xx 服务端错误" },
  { value: "error", label: "失败" },
];
const SCHEME_OPTIONS = [
  { value: "", label: "全部协议" },
  { value: "http", label: "HTTP" },
  { value: "https", label: "HTTPS" },
  { value: "ws", label: "WebSocket" },
];

export default function Toolbar({ onOpenSettings }: ToolbarProps) {
  const {
    proxy,
    toggleProxy,
    clearAll,
    filter,
    setFilter,
    loading,
    requests,
    methodFilter,
    setMethodFilter,
    statusFilter,
    setStatusFilter,
    schemeFilter,
    setSchemeFilter,
  } = useAppStore();

  return (
    <div className="toolbar-wrap">
      <div className="toolbar">
        <div className="toolbar-left">
          <button
            className={`btn btn-toggle ${proxy.running ? "running" : ""}`}
            onClick={toggleProxy}
            disabled={loading}
            title={proxy.running ? "停止代理" : "启动代理"}
          >
            {proxy.running ? (
              <>
                <Square size={16} /> 停止
              </>
            ) : (
              <>
                <Play size={16} /> 启动
              </>
            )}
          </button>

          <div className="proxy-status">
            {proxy.running ? (
              <span className="status-badge active">
                <Wifi size={14} />
                代理运行中 · {proxy.local_ip}:{proxy.port} · 系统代理已自动配置
              </span>
            ) : (
              <span className="status-badge">代理未启动</span>
            )}
          </div>

          <span className="count-badge">{requests.length} 条记录</span>
        </div>

        <div className="toolbar-center">
          <input
            className="filter-input"
            type="text"
            placeholder="搜索 URL / 域名 / 状态码…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        </div>

        <div className="toolbar-right">
          <button
            className="btn btn-danger"
            onClick={clearAll}
            title="清空所有抓包记录"
          >
            <Trash2 size={16} /> 清空
          </button>
          <button
            className="btn btn-icon"
            onClick={onOpenSettings}
            title="设置（AI / 证书）"
          >
            <Settings size={16} />
          </button>
        </div>
      </div>

      {/* 筛选栏 */}
      <div className="filter-bar">
        <span className="filter-label">方法</span>
        <div className="filter-group">
          {METHOD_OPTIONS.map((m) => (
            <button
              key={m || "all"}
              className={`filter-chip ${methodFilter === m ? "active" : ""}`}
              onClick={() => setMethodFilter(m)}
            >
              {m || "全部"}
            </button>
          ))}
        </div>

        <span className="filter-divider" />

        <span className="filter-label">状态</span>
        <div className="filter-group">
          {STATUS_OPTIONS.map((s) => (
            <button
              key={s.value}
              className={`filter-chip ${statusFilter === s.value ? "active" : ""}`}
              onClick={() => setStatusFilter(s.value)}
            >
              {s.label}
            </button>
          ))}
        </div>

        <span className="filter-divider" />

        <span className="filter-label">协议</span>
        <div className="filter-group">
          {SCHEME_OPTIONS.map((s) => (
            <button
              key={s.value}
              className={`filter-chip ${schemeFilter === s.value ? "active" : ""}`}
              onClick={() => setSchemeFilter(s.value)}
            >
              {s.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
