import { useState } from "react";
import { useAppStore } from "../lib/store";
import { BreakpointInfo } from "../lib/ipc";
import { PauseCircle, Play, X, Ban, Plus } from "lucide-react";

/**
 * 断点调试面板：代理命中断点规则时弹出，
 * 可查看/编辑挂起的请求，然后「放行」（转发修改后的请求）或「拦截」（403）。
 * 多个断点同时挂起时排队处理。
 */
export default function BreakpointPanel() {
  const breakpoints = useAppStore((s) => s.breakpoints);
  const resumeBreakpoint = useAppStore((s) => s.resumeBreakpoint);

  if (breakpoints.length === 0) return null;
  // 处理最早挂起的一个（FIFO）
  const current = [...breakpoints].sort((a, b) => a.started_at - b.started_at)[0];

  return (
    <div className="rules-overlay">
      <div className="bp-panel">
        <Editor key={current.bp_id} info={current} resume={resumeBreakpoint} pending={breakpoints.length} />
      </div>
    </div>
  );
}

function Editor({
  info,
  resume,
  pending,
}: {
  info: BreakpointInfo;
  resume: (bpId: string, decision: import("../lib/ipc").BreakpointDecision) => Promise<void>;
  pending: number;
}) {
  const [method, setMethod] = useState(info.method);
  const [url, setUrl] = useState(info.url);
  const [headers, setHeaders] = useState<[string, string][]>(info.headers);
  const [body, setBody] = useState(info.body ?? "");
  const [sending, setSending] = useState(false);

  const addHeader = () => setHeaders([...headers, ["", ""]]);
  const removeHeader = (i: number) => setHeaders(headers.filter((_, idx) => idx !== i));
  const updateHeader = (i: number, part: 0 | 1, v: string) => {
    const next = [...headers];
    next[i] = [part === 0 ? v : next[i][0], part === 1 ? v : next[i][1]];
    setHeaders(next);
  };

  const forward = async () => {
    setSending(true);
    await resume(info.bp_id, {
      type: "forward",
      method,
      url,
      headers: headers.filter(([k, v]) => k.trim() && v.trim()),
      body: body.trim() ? body : null,
    });
  };

  const abort = async () => {
    setSending(true);
    await resume(info.bp_id, { type: "abort" });
  };

  return (
    <>
      <div className="settings-header">
        <h3>
          <PauseCircle size={15} className="bp-icon" /> 请求已挂起 · 断点调试
          {pending > 1 && <span className="bp-queue">还有 {pending - 1} 个排队</span>}
        </h3>
      </div>

      <div className="replay-body">
        <div className="replay-editor">
          <div className="editor-row">
            <label>方法</label>
            <select value={method} onChange={(e) => setMethod(e.target.value)}>
              {["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"].map((m) => (
                <option key={m}>{m}</option>
              ))}
            </select>
          </div>
          <div className="editor-row">
            <label>URL</label>
            <input className="mono-input-flat" value={url} onChange={(e) => setUrl(e.target.value)} />
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
              <Plus size={12} /> 添加头部
            </button>
          </div>

          <div className="editor-section">请求体</div>
          <textarea
            rows={6}
            className="mono-input"
            placeholder="（无请求体）"
            value={body}
            onChange={(e) => setBody(e.target.value)}
          />

          <div className="editor-actions">
            <button className="btn btn-primary" onClick={forward} disabled={sending || !url.trim()}>
              <Play size={14} /> 放行（转发修改后的请求）
            </button>
            <button className="btn btn-danger" onClick={abort} disabled={sending}>
              <Ban size={14} /> 拦截（返回 403）
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
