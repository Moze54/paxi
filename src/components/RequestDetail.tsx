import { useEffect, useState, useMemo } from "react";
import { useAppStore } from "../lib/store";
import { formatDuration, formatTime, formatBytes, getWsFrames, WsFrame } from "../lib/ipc";
import { Sparkles, ChevronRight, ChevronDown, Copy, Check } from "lucide-react";

type TabKey = "overview" | "request" | "response" | "frames";

/** 简易 JSON 树节点 */
function JsonNode({
  name,
  value,
  depth,
}: {
  name: string | null;
  value: unknown;
  depth: number;
}) {
  const [open, setOpen] = useState(depth < 2);
  const isObj =
    value !== null && typeof value === "object" && !(value instanceof Array);
  const isArr = Array.isArray(value);

  if (!isObj && !isArr) {
    const valStr =
      typeof value === "string" ? `"${value}"` : String(value);
    const numOrBool =
      typeof value === "number" || typeof value === "boolean";
    return (
      <div className="json-row" style={{ paddingLeft: depth * 14 }}>
        {name !== null && <span className="json-key">{name}:</span>}
        <span className={numOrBool ? "json-num" : "json-str"}>{valStr}</span>
      </div>
    );
  }

  const entries: [string, unknown][] = isArr
    ? (value as unknown[]).map((v, i) => [String(i), v])
    : Object.entries(value as Record<string, unknown>);

  return (
    <div>
      <div
        className="json-row json-toggle"
        style={{ paddingLeft: depth * 14 }}
        onClick={() => setOpen(!open)}
      >
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        {name !== null && <span className="json-key">{name}:</span>}
        <span className="json-brace">{isArr ? `[${entries.length}]` : `{${entries.length}}`}</span>
      </div>
      {open &&
        entries.map(([k, v]) => (
          <JsonNode key={k} name={k} value={v} depth={depth + 1} />
        ))}
    </div>
  );
}

/** JSON 树（解析失败回退纯文本） */
function JsonTree({ body }: { body: string }) {
  const parsed = useMemo(() => {
    try {
      return { ok: true, value: JSON.parse(body) as unknown };
    } catch {
      return { ok: false as const };
    }
  }, [body]);

  if (!parsed.ok) {
    return <pre className="body-pre">{body}</pre>;
  }
  return (
    <div className="json-tree">
      <JsonNode name={null} value={parsed.value} depth={0} />
    </div>
  );
}

/** 头部列表 */
function HeadersView({ headers }: { headers: [string, string][] }) {
  if (headers.length === 0) return <p className="hint">（无）</p>;
  return (
    <table className="headers-table">
      <tbody>
        {headers.map(([k, v], i) => (
          <tr key={i}>
            <td className="h-k">{k}</td>
            <td className="h-v">{v}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Query 参数表 */
function QueryParams({ url }: { url: string }) {
  const params = useMemo(() => {
    try {
      const u = new URL(url);
      return [...u.searchParams.entries()];
    } catch {
      return [];
    }
  }, [url]);

  if (params.length === 0) return null;
  return (
    <div className="detail-section">
      <h4>Query 参数</h4>
      <table className="headers-table">
        <tbody>
          {params.map(([k, v], i) => (
            <tr key={i}>
              <td className="h-k">{k}</td>
              <td className="h-v">{v}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** 复制按钮 */
function CopyBtn({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="btn btn-ghost btn-mini"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        } catch {}
      }}
    >
      {copied ? <Check size={12} /> : <Copy size={12} />}
      {copied ? "已复制" : "复制"}
    </button>
  );
}

/** WebSocket 帧时间线 */
function WsFrames({ recordId }: { recordId: string }) {
  const [frames, setFrames] = useState<WsFrame[]>([]);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let alive = true;
    setLoaded(false);
    getWsFrames(recordId)
      .then((f) => {
        if (alive) {
          setFrames(f);
          setLoaded(true);
        }
      })
      .catch(() => setLoaded(true));
    return () => {
      alive = false;
    };
  }, [recordId]);

  if (!loaded) return <p className="hint">加载帧数据…</p>;
  if (frames.length === 0)
    return <p className="hint">（暂无帧，WebSocket 连接可能已断开）</p>;

  return (
    <div className="ws-frames">
      {frames.map((f) => (
        <div key={f.seq} className={`ws-frame ${f.dir === 0 ? "c2s" : "s2c"}`}>
          <span className="frame-meta">
            #{f.seq} {f.dir === 0 ? "↑ 发送" : "↓ 收到"} {f.opcode}
            {f.payload_len > 0 && ` · ${formatBytes(f.payload_len)}`}
          </span>
          {f.payload_text && (
            <pre className="frame-payload">{f.payload_text}</pre>
          )}
        </div>
      ))}
    </div>
  );
}

export default function RequestDetail() {
  const { selectedDetail, selectedId } = useAppStore();
  const [tab, setTab] = useState<TabKey>("overview");

  useEffect(() => {
    setTab("overview");
  }, [selectedId]);

  const onAiAnalyze = () => {
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
        {d.is_websocket && (
          <button
            className={tab === "frames" ? "active" : ""}
            onClick={() => setTab("frames")}
          >
            帧 ({d.ws_frame_count})
          </button>
        )}
      </div>

      <div className="detail-body">
        {tab === "overview" && (
          <div className="overview">
            <div className="kv-grid">
              <div className="kv">
                <span className="k">URL</span>
                <span className="v">{d.url}</span>
              </div>
              <div className="kv">
                <span className="k">方法</span>
                <span className="v">{d.method}</span>
              </div>
              <div className="kv">
                <span className="k">状态码</span>
                <span className="v">{d.status || "失败"}</span>
              </div>
              <div className="kv">
                <span className="k">协议</span>
                <span className="v">{d.scheme.toUpperCase()}</span>
              </div>
              <div className="kv">
                <span className="k">主机</span>
                <span className="v">{d.host}</span>
              </div>
              {d.client_ip && (
                <div className="kv">
                  <span className="k">来源</span>
                  <span className="v mono">
                    {d.client_process ?? `📱 ${d.client_ip}`}
                    {d.client_process && <span className="dim">（{d.client_ip}）</span>}
                  </span>
                </div>
              )}
              <div className="kv">
                <span className="k">耗时</span>
                <span className="v">{formatDuration(d.duration_ms)}</span>
              </div>
              <div className="kv">
                <span className="k">时间</span>
                <span className="v">{formatTime(d.started_at)}</span>
              </div>
              <div className="kv">
                <span className="k">请求大小</span>
                <span className="v">{formatBytes(d.request_body_size)}</span>
              </div>
              <div className="kv">
                <span className="k">响应大小</span>
                <span className="v">{formatBytes(d.response_body_size)}</span>
              </div>
              {d.is_websocket && (
                <div className="kv">
                  <span className="k">WebSocket 帧</span>
                  <span className="v">{d.ws_frame_count}</span>
                </div>
              )}
              {d.content_type && (
                <div className="kv">
                  <span className="k">Content-Type</span>
                  <span className="v">{d.content_type}</span>
                </div>
              )}
              {d.error && (
                <div className="kv error">
                  <span className="k">错误</span>
                  <span className="v">{d.error}</span>
                </div>
              )}
              {d.matched_rule && (
                <div className="kv">
                  <span className="k">命中规则</span>
                  <span className="v rule-hit">⚡ {d.matched_rule}</span>
                </div>
              )}
            </div>
          </div>
        )}

        {tab === "request" && (
          <div className="req-resp">
            <QueryParams url={d.url} />
            <div className="detail-section">
              <h4>请求头</h4>
              <HeadersView headers={d.request_headers} />
            </div>
            <div className="detail-section">
              <h4>请求体</h4>
              {d.request_body ? (
                <>
                  <div className="body-toolbar">
                    <CopyBtn text={d.request_body} />
                  </div>
                  {d.content_type?.includes("json") || looksJson(d.request_body) ? (
                    <JsonTree body={d.request_body} />
                  ) : (
                    <pre className="body-pre">{d.request_body}</pre>
                  )}
                </>
              ) : (
                <p className="hint">（无请求体）</p>
              )}
            </div>
          </div>
        )}

        {tab === "response" && (
          <div className="req-resp">
            <div className="detail-section">
              <h4>响应头</h4>
              <HeadersView headers={d.response_headers} />
            </div>
            <div className="detail-section">
              <h4>响应体</h4>
              {d.response_body ? (
                <>
                  <div className="body-toolbar">
                    <CopyBtn text={d.response_body} />
                  </div>
                  {d.content_type?.includes("json") ||
                  looksJson(d.response_body) ? (
                    <JsonTree body={d.response_body} />
                  ) : (
                    <pre className="body-pre">{d.response_body}</pre>
                  )}
                </>
              ) : (
                <p className="hint">（无响应体）</p>
              )}
            </div>
          </div>
        )}

        {tab === "frames" && d.is_websocket && <WsFrames recordId={d.id} />}
      </div>
    </div>
  );
}

/** 粗略判断 body 是否为 JSON（含 content-type 未声明的情况） */
function looksJson(body: string): boolean {
  const t = body.trimStart();
  return t.startsWith("{") || t.startsWith("[");
}
