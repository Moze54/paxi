import { useMemo, useState } from "react";
import { useAppStore } from "../lib/store";
import { RequestRecord, getRequestDetail, replayRequest, getRequests } from "../lib/ipc";
import { diffLines, diffStat } from "../lib/diff";
import { Send, X, Diff } from "lucide-react";

interface ReplayPanelProps {
  record: RequestRecord;
  onClose: () => void;
}

type Tab = "edit" | "result" | "diff";

const METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"];

export default function ReplayPanel({ record, onClose }: ReplayPanelProps) {
  const setError = useAppStore((s) => s.setError);
  const refreshRequests = useAppStore((s) => s.refreshRequests);

  const [tab, setTab] = useState<Tab>("edit");
  const [method, setMethod] = useState(record.method);
  const [url, setUrl] = useState(record.url);
  const [headers, setHeaders] = useState<[string, string][]>(
    record.request_headers.filter(([k]) => !["host", "connection", "proxy-connection", "content-length", "keep-alive", "transfer-encoding", "upgrade"].includes(k.toLowerCase()))
  );
  const [body, setBody] = useState(record.request_body ?? "");
  const [sending, setSending] = useState(false);
  const [result, setResult] = useState<RequestRecord | null>(null);
  const [replayed, setReplayed] = useState(false);

  const addHeader = () => setHeaders([...headers, ["", ""]]);
  const removeHeader = (i: number) =>
    setHeaders(headers.filter((_, idx) => idx !== i));
  const updateHeader = (i: number, part: 0 | 1, v: string) => {
    const next = [...headers];
    next[i] = [part === 0 ? v : next[i][0], part === 1 ? v : next[i][1]];
    setHeaders(next);
  };

  const send = async () => {
    setSending(true);
    setReplayed(true);
    try {
      const validHeaders = headers.filter(([k, v]) => k.trim() && v.trim());
      await replayRequest({
        method,
        url,
        headers: validHeaders,
        body: body.trim() ? body : null,
      });
      // 稍等写入完成再拉取最新记录（重放结果是最新一条 is_replay 记录）
      setTimeout(async () => {
        await refreshRequests();
        try {
          const list = await getRequests();
          const latestReplay = list.find((r) => r.is_replay);
          if (latestReplay) {
            const detail = await getRequestDetail(latestReplay.id);
            setResult(detail);
            setTab("result");
          }
        } catch {
          /* 忽略 */
        }
      }, 400);
    } catch (e) {
      setError(String(e));
    } finally {
      setSending(false);
    }
  };

  const bodyDiff = useMemo(() => {
    if (!result) return [];
    return diffLines(record.response_body ?? "", result.response_body ?? "");
  }, [result, record]);

  const stat = useMemo(() => diffStat(bodyDiff), [bodyDiff]);

  const statusChanged = result && record.status !== result.status;

  return (
    <div className="rules-overlay" onClick={onClose}>
      <div className="replay-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h3>
            <Send size={15} /> 重放请求
            <span className="replay-src" title={record.url}>
              {record.method} {record.url}
            </span>
          </h3>
          <button className="btn btn-icon" onClick={onClose}>
            <X size={14} />
          </button>
        </div>

        <div className="tab-bar">
          <button className={tab === "edit" ? "active" : ""} onClick={() => setTab("edit")}>
            编辑
          </button>
          <button
            className={tab === "result" ? "active" : ""}
            onClick={() => setTab("result")}
            disabled={!result}
          >
            结果 {result && `(${result.status || "失败"})`}
          </button>
          <button
            className={tab === "diff" ? "active" : ""}
            onClick={() => setTab("diff")}
            disabled={!result}
          >
            <Diff size={13} /> 对比 {result && `(+${stat.added}/-${stat.removed})`}
          </button>
        </div>

        <div className="replay-body">
          {tab === "edit" && (
            <div className="replay-editor">
              <div className="editor-row">
                <label>方法</label>
                <select value={method} onChange={(e) => setMethod(e.target.value)}>
                  {METHODS.map((m) => (
                    <option key={m}>{m}</option>
                  ))}
                </select>
              </div>
              <div className="editor-row">
                <label>URL</label>
                <input
                  className="mono-input-flat"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                />
              </div>

              <div className="editor-section">请求头</div>
              <div className="headers-editor">
                {headers.map(([k, v], i) => (
                  <div key={i} className="header-edit-row">
                    <input
                      className="mono-input-flat hkey"
                      placeholder="名称"
                      value={k}
                      onChange={(e) => updateHeader(i, 0, e.target.value)}
                    />
                    <input
                      className="mono-input-flat"
                      placeholder="值"
                      value={v}
                      onChange={(e) => updateHeader(i, 1, e.target.value)}
                    />
                    <button className="btn btn-icon btn-mini" onClick={() => removeHeader(i)}>
                      <X size={12} />
                    </button>
                  </div>
                ))}
                <button className="btn btn-ghost btn-mini" onClick={addHeader}>
                  + 添加头部
                </button>
              </div>

              <div className="editor-section">请求体</div>
              <textarea
                rows={8}
                className="mono-input"
                placeholder="（无请求体）"
                value={body}
                onChange={(e) => setBody(e.target.value)}
              />

              <div className="editor-actions">
                <button className="btn btn-primary" onClick={send} disabled={sending || !url.trim()}>
                  <Send size={14} /> {sending ? "发送中…" : "发送"}
                </button>
                <span className="hint">
                  {replayed && "结果将以 REPLAY 标记出现在列表顶部"}
                </span>
              </div>
            </div>
          )}

          {tab === "result" && result && (
            <div className="replay-result">
              <div className="result-summary">
                <span className={`status-tag ${statusChanged ? "changed" : ""}`}>
                  {record.status} → {result.status || "失败"}
                </span>
                <span className="hint">耗时 {result.duration_ms}ms</span>
                {result.error && <span className="error-text">{result.error}</span>}
              </div>
              <div className="editor-section">响应头</div>
              <table className="headers-table">
                <tbody>
                  {result.response_headers.map(([k, v], i) => (
                    <tr key={i}>
                      <td className="h-k">{k}</td>
                      <td className="h-v">{v}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="editor-section">响应体</div>
              <pre className="frame-payload">{result.response_body ?? "（空）"}</pre>
            </div>
          )}

          {tab === "diff" && result && (
            <div className="replay-diff">
              <div className="result-summary">
                <span>
                  左 <span className="diff-old">{record.status || "失败"}</span> → 右{" "}
                  <span className="diff-new">{result.status || "失败"}</span>
                </span>
                <span className="diff-stat">
                  <span className="diff-add">+{stat.added}</span>{" "}
                  <span className="diff-del">-{stat.removed}</span>
                </span>
              </div>
              <div className="diff-view">
                {bodyDiff.map((d, i) => (
                  <div key={i} className={`diff-line ${d.type}`}>
                    <span className="diff-marker">
                      {d.type === "add" ? "+" : d.type === "del" ? "-" : " "}
                    </span>
                    <span className="diff-text">{d.text || " "}</span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
