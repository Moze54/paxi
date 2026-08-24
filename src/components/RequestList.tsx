import { useAppStore } from "../lib/store";
import { methodColor, statusColor, formatDuration, formatTime } from "../lib/ipc";
import { Globe, Lock, Zap } from "lucide-react";

export default function RequestList() {
  const {
    requests,
    filter,
    selectedId,
    selectRequest,
    methodFilter,
    statusFilter,
    schemeFilter,
  } = useAppStore();

  const filtered = requests.filter((r) => {
    // 关键字搜索
    if (filter) {
      const f = filter.toLowerCase();
      const match =
        r.url.toLowerCase().includes(f) ||
        r.method.toLowerCase().includes(f) ||
        String(r.status).includes(f) ||
        r.host.toLowerCase().includes(f);
      if (!match) return false;
    }
    // 方法筛选
    if (methodFilter && r.method.toUpperCase() !== methodFilter) return false;
    // 状态筛选
    if (statusFilter) {
      if (statusFilter === "error") {
        if (r.status !== 0) return false;
      } else {
        const prefix = statusFilter[0]; // "2" / "3" / "4" / "5"
        if (Math.floor(r.status / 100) !== Number(prefix)) return false;
      }
    }
    // 协议筛选
    if (schemeFilter) {
      if (schemeFilter === "ws") {
        if (!r.is_websocket) return false;
      } else if (r.scheme !== schemeFilter) {
        return false;
      }
    }
    return true;
  });

  return (
    <div className="request-list">
      {filtered.length === 0 ? (
        <div className="empty-state">
          <p>暂无抓包记录</p>
          <p className="hint">启动代理后，访问网页或连接手机即可看到请求</p>
        </div>
      ) : (
        <table className="request-table">
          <thead>
            <tr>
              <th style={{ width: 48 }}>方法</th>
              <th>URL</th>
              <th style={{ width: 60 }}>状态</th>
              <th style={{ width: 70 }}>耗时</th>
              <th style={{ width: 90 }}>时间</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((r) => (
              <tr
                key={r.id}
                className={selectedId === r.id ? "selected" : ""}
                onClick={() => selectRequest(r.id)}
              >
                <td>
                  <span
                    className="method-tag"
                    style={{ background: methodColor(r.method) }}
                  >
                    {r.method}
                  </span>
                </td>
                <td className="url-cell" title={r.url}>
                  <span className="scheme-icon">
                    {r.is_websocket ? (
                      <Zap size={12} color="#f39c12" />
                    ) : r.scheme === "https" ? (
                      <Lock size={12} color="#2ecc71" />
                    ) : (
                      <Globe size={12} color="#7f8c8d" />
                    )}
                  </span>
                  <span className="url-text">{r.url}</span>
                </td>
                <td>
                  <span
                    className="status-tag"
                    style={{ color: statusColor(r.status) }}
                  >
                    {r.status || "✕"}
                  </span>
                </td>
                <td className="mono">{formatDuration(r.duration_ms)}</td>
                <td className="mono time-cell">{formatTime(r.started_at)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
