import { useEffect, useState } from "react";
import { getStats, Stats } from "../lib/ipc";
import { BarChart3, X, RefreshCw, Loader2 } from "lucide-react";

interface StatsPanelProps {
  onClose: () => void;
}

/** 简单 CSS 条形图（不引第三方图表库，控制体积） */
function Bars({ data, maxHint, colorKey }: { data: [string, number][]; maxHint: number; colorKey: string }) {
  const max = Math.max(...data.map(([, v]) => v), 1);
  const total = data.reduce((s, [, v]) => s + v, 0);
  return (
    <div className="stats-bars">
      {data.map(([label, count]) => (
        <div key={label} className="stats-bar-row" title={`${label}: ${count}`}>
          <span className="stats-bar-label">{label}</span>
          <div className="stats-bar-track">
            <div
              className={`stats-bar-fill ${colorKey}`}
              style={{ width: `${Math.max((count / max) * 100, 2)}%` }}
            />
          </div>
          <span className="stats-bar-count">{count}</span>
          <span className="stats-bar-pct">
            {total ? `${((count / total) * 100).toFixed(0)}%` : ""}
          </span>
        </div>
      ))}
      {data.length === 0 && <span className="hint">暂无数据</span>}
      {/* maxHint 用于高度归一提示（保留参数以避免 unused） */}
      <span style={{ display: "none" }}>{maxHint}</span>
    </div>
  );
}

export default function StatsPanel({ onClose }: StatsPanelProps) {
  const [stats, setStats] = useState<Stats | null>(null);
  const [loading, setLoading] = useState(true);

  const load = async () => {
    setLoading(true);
    try {
      setStats(await getStats());
    } catch (e) {
      alert(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 时间线（24h，最近在前）：翻转成从旧到新展示
  const timeline = stats ? [...stats.timeline].reverse() : [];
  const maxTime = Math.max(...timeline, 1);

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="stats-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h3>
            <BarChart3 size={15} /> 流量统计
            {stats && <span className="stats-total">共 {stats.total} 条记录</span>}
          </h3>
          <div className="settings-actions">
            <button className="btn btn-icon" onClick={load} title="刷新">
              {loading ? <Loader2 size={14} className="spin" /> : <RefreshCw size={14} />}
            </button>
            <button className="btn btn-icon" onClick={onClose}>
              <X size={14} />
            </button>
          </div>
        </div>

        <div className="stats-body">
          {!stats ? (
            <p className="hint">加载中…</p>
          ) : (
            <>
              {/* 概览卡片 */}
              <div className="stats-cards">
                <div className="stats-card">
                  <span className="stats-card-num">{stats.total}</span>
                  <span className="stats-card-label">总请求</span>
                </div>
                <div className="stats-card good">
                  <span className="stats-card-num">{stats.succeeded}</span>
                  <span className="stats-card-label">成功</span>
                </div>
                <div className="stats-card bad">
                  <span className="stats-card-num">{stats.failed}</span>
                  <span className="stats-card-label">失败</span>
                </div>
                <div className="stats-card">
                  <span className="stats-card-num">{stats.avg_duration_ms}ms</span>
                  <span className="stats-card-label">平均耗时</span>
                </div>
                <div className="stats-card">
                  <span className="stats-card-num">{stats.max_duration_ms}ms</span>
                  <span className="stats-card-label">最大耗时</span>
                </div>
              </div>

              {/* 24h 时间线 */}
              <div className="stats-section">
                <h4>近 24 小时请求量（小时 ×2）</h4>
                <div className="stats-timeline">
                  {timeline.map((v, i) => (
                    <div key={i} className="stats-tl-col" title={`${i}h前: ${v}`}>
                      <div
                        className="stats-tl-bar"
                        style={{ height: `${(v / maxTime) * 100}%` }}
                      />
                    </div>
                  ))}
                </div>
                <div className="stats-tl-axis">
                  <span>24h前</span>
                  <span>12h前</span>
                  <span>现在</span>
                </div>
              </div>

              <div className="stats-grid">
                <div className="stats-section">
                  <h4>状态码分布</h4>
                  <Bars data={stats.status_dist} maxHint={stats.total} colorKey="fill-status" />
                </div>
                <div className="stats-section">
                  <h4>方法分布</h4>
                  <Bars
                    data={stats.method_dist}
                    maxHint={stats.total}
                    colorKey="fill-method"
                  />
                </div>
                <div className="stats-section">
                  <h4>协议分布</h4>
                  <Bars data={stats.scheme_dist} maxHint={stats.total} colorKey="fill-scheme" />
                </div>
                <div className="stats-section">
                  <h4>域名 TOP</h4>
                  <Bars data={stats.host_top.slice(0, 10)} maxHint={stats.total} colorKey="fill-host" />
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}