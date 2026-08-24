import { useEffect, useState } from "react";
import { useAppStore } from "../lib/store";
import { formatDuration, formatTime } from "../lib/ipc";
import { FileJson, Sparkles } from "lucide-react";

type TabKey = "overview" | "request" | "response";

function formatBody(body: string | null): string {
  if (!body) return "";
  try {
    const parsed = JSON.parse(body);
    return JSON.stringify(parsed, null, 2);
  } catch {
    return body;
  }
}

export default function RequestDetail() {
  const { selectedDetail, selectedId } = useAppStore();
  const [tab, setTab] = useState<TabKey>("overview");

  useEffect(() => {
    setTab("overview");
  }, [selectedId]);

  const onAiAnalyze = () => {
    // AI 分析面板由父组件控制弹窗（见 App.tsx）
    document.dispatchEvent(
      new CustomEvent("paxi:analyze", { detail: selectedDetail })
    );
  };

  if (!selectedDetail) {
    return (
      <div className="detail-panel empty">
        <p>选择左侧的一条请求查看详情</p>
      </div>
    );
  }

  const d = selectedDetail;

  return (
    <div className="detail-panel">
      <div className="detail-header">
        <div className="detail-url" title={d.url}>
          <span className="method-tag" style={{ background: "#3498db" }}>
            {d.method}
          </span>
          <span className="detail-url-text">{d.url}</span>
        </div>
        <button className="btn btn-ai" onClick={onAiAnalyze} title="AI 分析此请求">
          <Sparkles size={14} /> AI 分析
        </button>
      </div>

      <div className="tab-bar">
        <button
          className={tab === "overview" ? "active" : ""}
          onClick={() => setTab("overview")}
        >
          概览
        </button>
        <button
          className={tab === "request" ? "active" : ""}
          onClick={() => setTab("request")}
        >
          请求
        </button>
        <button
          className={tab === "response" ? "active" : ""}
          onClick={() => setTab("response")}
        >
          响应
        </button>
      </div>

      <div className="detail-body">
        {tab === "overview" && (
          <div className="overview">
            <div className="kv-grid">
              <div className="kv"><span className="k">URL</span><span className="v">{d.url}</span></div>
              <div className="kv"><span className="k">方法</span><span className="v">{d.method}</span></div>
              <div className="kv"><span className="k">状态码</span><span className="v">{d.status || "失败"}</span></div>
              <div className="kv"><span className="k">协议</span><span className="v">{d.scheme.toUpperCase()}</span></div>
              <div className="kv"><span className="k">主机</span><span className="v">{d.host}</span></div>
              <div className="kv"><span className="k">耗时</span><span className="v">{formatDuration(d.duration_ms)}</span></div>
              <div className="kv"><span className="k">时间</span><span className="v">{formatTime(d.started_at)}</span></div>
              <div className="kv"><span className="k">WebSocket</span><span className="v">{d.is_websocket ? "是" : "否"}</span></div>
              {d.error && (
                <div className="kv error"><span className="k">错误</span><span className="v">{d.error}</span></div>
              )}
            </div>
          </div>
        )}

        {tab === "request" && (
          <div className="req-resp">
            <h4>请求头</h4>
            <pre className="headers-pre">
              {d.request_headers.map(([k, v]) => `${k}: ${v}`).join("\n")}
            </pre>
            {d.request_body && (
              <>
                <h4>请求体</h4>
                <pre className="body-pre">
                  <FileJson size={12} className="inline-icon" />
                  {formatBody(d.request_body)}
                </pre>
              </>
            )}
            {!d.request_body && <p className="hint">（无请求体）</p>}
          </div>
        )}

        {tab === "response" && (
          <div className="req-resp">
            <h4>响应头</h4>
            <pre className="headers-pre">
              {d.response_headers.map(([k, v]) => `${k}: ${v}`).join("\n")}
            </pre>
            {d.response_body && (
              <>
                <h4>响应体</h4>
                <pre className="body-pre">
                  <FileJson size={12} className="inline-icon" />
                  {formatBody(d.response_body)}
                </pre>
              </>
            )}
            {!d.response_body && <p className="hint">（无响应体）</p>}
          </div>
        )}
      </div>
    </div>
  );
}
