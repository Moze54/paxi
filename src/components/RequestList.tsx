import { useMemo, useRef, useCallback } from "react";
import { useAppStore } from "../lib/store";
import { filterRequests } from "../lib/filters";
import {
  methodColor,
  statusColor,
  formatDuration,
  formatTime,
  formatBytes,
} from "../lib/ipc";
import { toCurl } from "../lib/codegen";
import { Globe, Lock, Zap, RotateCcw, ArrowDownUp, Star } from "lucide-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { openContextMenu, hideContextMenu } from "./ContextMenu";

const ROW_HEIGHT = 30;

/** URL 展示：host 灰 + path 白（虚拟列表内减少重排） */
function UrlDisplay({ url }: { url: string }) {
  const { origin, rest } = useMemo(() => {
    try {
      const u = new URL(url);
      return { origin: u.host, rest: u.pathname + u.search };
    } catch {
      return { origin: "", rest: url };
    }
  }, [url]);
  return (
    <span className="url-text">
      {origin && <span className="url-host">{origin}</span>}
      <span className="url-path">{rest}</span>
    </span>
  );
}

/** 进程名压缩为徽标（去 .exe，取前 12 字符） */
function shortApp(process: string): string {
  const base = process.replace(/\.exe$/i, "");
  return base.length > 12 ? base.slice(0, 12) + "…" : base;
}

export default function RequestList() {
  const {
    requests,
    filter,
    selectedId,
    selectRequest,
    methodFilter,
    statusFilter,
    schemeFilter,
    marks,
    toggleStar,
    toggleStarOnly,
    processFilter,
    setProcessFilter,
  } = useAppStore();
  // 工具栏星标过滤（独立于其他筛选）
  const starOnly = useAppStore((s) => s.starOnly);

  const parentRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(
    () =>
      filterRequests(requests, filter, methodFilter, statusFilter, schemeFilter, starOnly, marks, processFilter),
    [requests, filter, methodFilter, statusFilter, schemeFilter, starOnly, marks, processFilter]
  );

  // 来源应用选项（去重）：本机进程名 + 远程设备 IP
  const appOptions = useMemo(() => {
    const set = new Set<string>();
    for (const r of requests) {
      set.add(r.client_process ?? `📱 ${r.client_ip ?? "unknown"}`);
    }
    return Array.from(set).sort((a, b) => (a.startsWith("📱") ? 1 : b.startsWith("📱") ? -1 : a.localeCompare(b)));
  }, [requests]);

  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 20,
  });

  const onSelect = useCallback(
    (id: string) => {
      hideContextMenu();
      selectRequest(id);
    },
    [selectRequest]
  );

  const onRowContext = useCallback(
    async (e: React.MouseEvent, url: string, id: string) => {
      e.preventDefault();
      import("../lib/ipc").then(async (m) => {
        const rec = await m.getRequestDetail(id);
        if (!rec) return;
        const cur = useAppStore.getState().marks[id];
        openContextMenu(e.clientX, e.clientY, [
          {
            key: "star",
            label: cur?.star ? "取消收藏" : "收藏 ★",
            onClick: () => useAppStore.getState().toggleStar(id),
          },
          {
            key: "mark",
            label: "标注颜色",
            onClick: () => {
              // 二级菜单：直接在下方追加颜色项
              openContextMenu(e.clientX + 8, e.clientY + 8, [
                { key: "c-none", label: "清除标注", onClick: () => useAppStore.getState().setMarkColor(id, "") },
                { key: "c-red", label: "● 红", onClick: () => useAppStore.getState().setMarkColor(id, "red") },
                { key: "c-yellow", label: "● 黄", onClick: () => useAppStore.getState().setMarkColor(id, "yellow") },
                { key: "c-green", label: "● 绿", onClick: () => useAppStore.getState().setMarkColor(id, "green") },
                { key: "c-blue", label: "● 蓝", onClick: () => useAppStore.getState().setMarkColor(id, "blue") },
                { key: "c-purple", label: "● 紫", onClick: () => useAppStore.getState().setMarkColor(id, "purple") },
              ]);
            },
          },
          {
            key: "copy-url",
            label: "复制 URL",
            onClick: () => navigator.clipboard.writeText(rec.url).catch(() => {}),
          },
          {
            key: "copy-curl",
            label: "复制为 cURL",
            onClick: () =>
              navigator.clipboard.writeText(toCurl(rec, false)).catch(() => {}),
          },
          {
            key: "codegen",
            label: "生成代码…",
            onClick: () =>
              document.dispatchEvent(
                new CustomEvent("paxi:codegen", { detail: rec })
              ),
          },
          {
            key: "replay",
            label: "重放此请求…",
            onClick: () =>
              document.dispatchEvent(
                new CustomEvent("paxi:replay", { detail: rec })
              ),
          },
          {
            key: "ai",
            label: "AI 分析",
            onClick: () =>
              document.dispatchEvent(new CustomEvent("paxi:analyze", { detail: rec })),
          },
        ]);
        void url;
      });
    },
    []
  );

  return (
    <div className="request-list">
      <div className="list-header">
        <span style={{ width: 52 }}>方法</span>
        <span className="flex-1">URL</span>
        <span style={{ width: 46, textAlign: "right" }}>状态</span>
        <span style={{ width: 62, textAlign: "right" }}>耗时</span>
        <span style={{ width: 64, textAlign: "right" }}>大小</span>
        <span style={{ width: 76, textAlign: "right" }}>时间</span>
      </div>
      <div className="list-star-bar">
        <button
          className={`btn btn-ghost btn-mini ${starOnly ? "star-active" : ""}`}
          onClick={() => toggleStarOnly()}
          title={starOnly ? "显示全部" : "仅显示收藏"}
        >
          <Star size={12} fill={starOnly ? "currentColor" : "none"} />
          {starOnly ? "显示全部" : "仅看收藏"}
        </button>
        <select
          className="app-filter-select"
          value={processFilter}
          onChange={(e) => setProcessFilter(e.target.value)}
          title="按来源应用筛选"
        >
          <option value="">全部应用</option>
          {appOptions.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
        <span className="hint">
          {processFilter
            ? `当前筛选：${processFilter}（${new Set(requests.filter((r) => (r.client_process ?? `📱 ${r.client_ip ?? "unknown"}`) === processFilter).map((r) => r.id)).size} 条）`
            : appOptions.length > 0
              ? `${appOptions.length} 个来源 · 右键可收藏 / 标注颜色`
              : "右键可收藏 / 标注颜色"}
        </span>
      </div>

      {filtered.length === 0 ? (
        <div className="empty-state">
          <p>暂无抓包记录</p>
          <p className="hint">启动代理后，访问网页或连接手机即可看到请求</p>
        </div>
      ) : (
        <div className="list-scroll" ref={parentRef}>
          <div
            style={{
              height: virtualizer.getTotalSize(),
              position: "relative",
              width: "100%",
            }}
          >
            {virtualizer.getVirtualItems().map((vRow) => {
              const r = filtered[vRow.index];
              return (
                <div
                  key={r.id}
                  className={`request-row ${selectedId === r.id ? "selected" : ""} ${marks[r.id]?.color ? `mark-${marks[r.id].color}` : ""}`}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    height: vRow.size,
                    transform: `translateY(${vRow.start}px)`,
                  }}
                  onClick={() => onSelect(r.id)}
                  onContextMenu={(e) => onRowContext(e, r.url, r.id)}
                >
                  <span className="cell-method" style={{ width: 52 }}>
                    <span
                      className="method-tag"
                      style={{ background: methodColor(r.method) }}
                    >
                      {r.method}
                    </span>
                  </span>
                  <span className="cell-url flex-1" title={`${r.client_process ?? `📱 ${r.client_ip ?? "未知设备"}`} · ${r.url}`}>
                    <button
                      className={`row-star ${marks[r.id]?.star ? "active" : ""}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleStar(r.id);
                      }}
                      title={marks[r.id]?.star ? "取消收藏" : "收藏"}
                    >
                      <Star size={11} fill={marks[r.id]?.star ? "currentColor" : "none"} />
                    </button>
                    {r.client_process && (
                      <span
                        className="app-tag"
                        title={`来源：${r.client_process}`}
                      >
                        {shortApp(r.client_process)}
                      </span>
                    )}
                    <span className="scheme-icon" title={r.is_websocket ? "WebSocket" : undefined}>
                      {r.is_websocket ? (
                        <Zap size={11} color="#f39c12" />
                      ) : r.scheme === "https" || r.scheme === "wss" ? (
                        <Lock size={11} color="#2ecc71" />
                      ) : (
                        <Globe size={11} color="#7f8c8d" />
                      )}
                    </span>
                    {r.is_replay && (
                      <RotateCcw
                        size={11}
                        color="#9b59b6"
                        className="replay-icon"
                      />
                    )}
                    {r.passthrough && (
                      <ArrowDownUp
                        size={11}
                        color="#16a085"
                        className="replay-icon"
                      />
                    )}
                    <UrlDisplay url={r.url} />
                    {r.is_websocket && (
                      <span className="ws-count">×{r.ws_frame_count}</span>
                    )}
                  </span>
                  <span className="cell-status" style={{ width: 46 }}>
                    <span style={{ color: statusColor(r.status) }}>
                      {r.status || "✕"}
                    </span>
                  </span>
                  <span className="cell-mono" style={{ width: 62 }}>
                    {formatDuration(r.duration_ms)}
                  </span>
                  <span className="cell-mono" style={{ width: 64 }}>
                    {formatBytes(r.response_body_size)}
                  </span>
                  <span className="cell-mono cell-time" style={{ width: 76 }}>
                    {formatTime(r.started_at)}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
