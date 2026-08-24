import { useAppStore } from "../lib/store";
import { Play, Square, Trash2, Settings, Wifi } from "lucide-react";

interface ToolbarProps {
  onOpenSettings: () => void;
}

export default function Toolbar({ onOpenSettings }: ToolbarProps) {
  const { proxy, toggleProxy, clearAll, filter, setFilter, loading, requests } =
    useAppStore();

  return (
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
          placeholder="搜索 URL / 方法 / 状态码…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>

      <div className="toolbar-right">
        <button className="btn btn-icon" onClick={clearAll} title="清空所有记录">
          <Trash2 size={16} />
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
  );
}
