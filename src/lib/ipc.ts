import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ===== 与 Rust 端对齐的类型定义 =====

export interface ProxyState {
  running: boolean;
  port: number;
  local_ip: string;
}

export interface RequestMeta {
  id: string;
  method: string;
  url: string;
  host: string;
  scheme: string;
  status: number;
  duration_ms: number;
  started_at: number;
  is_websocket: boolean;
  ws_frame_count: number;
  error: string | null;
  client_ip: string | null;
  /** 来源进程名（本机进程识别；手机无） */
  client_process: string | null;
  request_body_size: number;
  response_body_size: number;
  content_type: string | null;
  /** 是否为重放请求 */
  is_replay: boolean;
  /** 是否为 TLS 直通（不解密，仅转发） */
  passthrough: boolean;
}

export interface RequestRecord extends RequestMeta {
  request_body: string | null;
  response_body: string | null;
  request_headers: [string, string][];
  response_headers: [string, string][];
  /** 命中的规则名（若有） */
  matched_rule: string | null;
}

export interface ReplayParams {
  method: string;
  url: string;
  headers: [string, string][];
  body: string | null;
}

/** 断点快照（挂起中的请求） */
export interface BreakpointInfo {
  bp_id: string;
  record_id: string;
  method: string;
  url: string;
  headers: [string, string][];
  body: string | null;
  started_at: number;
}

/** 断点决策 */
export type BreakpointDecision =
  | {
      type: "forward";
      method: string;
      url: string;
      headers: [string, string][];
      body: string | null;
    }
  | { type: "abort" };

// ===== 规则 =====

export type RuleAction =
  | { type: "mock"; params: { status: number; content_type: string; body: string } }
  | { type: "redirect"; params: { to: string; status: number } }
  | { type: "delay"; params: { ms: number } }
  | { type: "delay_response"; params: { ms: number } }
  | { type: "abort" }
  | { type: "breakpoint"; params: Record<string, never> }
  | { type: "throttle"; params: { kbps: number; delay_ms: number; drop_pct: number } };

export interface RuleMatcher {
  host: string | null;
  path: string | null;
  method: string | null;
}

export interface Rule {
  id: string;
  name: string;
  enabled: boolean;
  priority: number;
  matcher: RuleMatcher;
  action: RuleAction;
  hits: number;
  updated_at: number;
}

export interface WsFrame {
  seq: number;
  /** 0 = 客户端→服务端，1 = 服务端→客户端 */
  dir: 0 | 1;
  opcode: string;
  payload_len: number;
  payload_text: string | null;
  ts_ms: number;
}

export interface ClientInfo {
  ip: string;
  is_local: boolean;
  first_seen: number;
  last_seen: number;
  connections: number;
}

// ===== IPC 调用封装 =====

export const startProxy = (port: number) =>
  invoke<ProxyState>("start_proxy", { port });

export const stopProxy = () => invoke<void>("stop_proxy");

export const getProxyStatus = () => invoke<ProxyState>("get_proxy_status");

export const getRequests = () => invoke<RequestMeta[]>("get_requests");

export const getRequestDetail = (id: string) =>
  invoke<RequestRecord | null>("get_request_detail", { id });

export const getWsFrames = (id: string) =>
  invoke<WsFrame[]>("get_ws_frames", { id });

export const getClients = () => invoke<ClientInfo[]>("get_clients");

export const clearRequests = () => invoke<void>("clear_requests");

export const exportCaCert = () => invoke<string>("export_ca_cert");

export const getCaCertPem = () => invoke<string>("get_ca_cert_pem");

export const listRules = () => invoke<Rule[]>("list_rules");

export const upsertRule = (rule: Rule) => invoke<void>("upsert_rule", { rule });

export const deleteRule = (id: string) => invoke<void>("delete_rule", { id });

export const replayRequest = (params: ReplayParams) =>
  invoke<RequestMeta>("replay_request", { params });

export const exportHar = () => invoke<string>("export_har");

export const importHar = (path: string) =>
  invoke<number>("import_har", { path });

/** 上游代理配置 */
export interface UpstreamProxy {
  enabled: boolean;
  host: string;
  port: number;
  username: string;
  password: string;
}

export const getUpstreamProxy = () =>
  invoke<UpstreamProxy>("get_upstream_proxy");

export const setUpstreamProxy = (config: UpstreamProxy) =>
  invoke<void>("set_upstream_proxy", { config });

/** 统计结果 */
export interface Stats {
  total: number;
  succeeded: number;
  failed: number;
  status_dist: [string, number][];
  method_dist: [string, number][];
  scheme_dist: [string, number][];
  host_top: [string, number][];
  avg_duration_ms: number;
  max_duration_ms: number;
  timeline: number[];
}

export const getStats = () => invoke<Stats>("get_stats");

export const getPassthroughHosts = () =>
  invoke<string[]>("get_passthrough_hosts");

export const setPassthroughHosts = (hosts: string[]) =>
  invoke<void>("set_passthrough_hosts", { hosts });

export const listBreakpoints = () =>
  invoke<BreakpointInfo[]>("list_breakpoints");

export const resumeBreakpoint = (bpId: string, decision: BreakpointDecision) =>
  invoke<void>("resume_breakpoint", { bpId, decision });

/** 订阅断点命中事件 */
export const onBreakpoint = (cb: (info: BreakpointInfo) => void) => {
  let unlisten: (() => void) | null = null;
  let disposed = false;
  listen<BreakpointInfo>("breakpoint://hit", (e) => cb(e.payload)).then((f) => {
    if (disposed) f();
    else unlisten = f;
  });
  return () => {
    disposed = true;
    unlisten?.();
  };
};

// ===== 事件订阅（后端 → 前端） =====

export const onTrafficNew = (cb: (batch: RequestMeta[]) => void) =>
  listen<RequestMeta[]>("traffic://new", (e) => cb(e.payload));

export const onTrafficUpdate = (cb: (batch: RequestMeta[]) => void) =>
  listen<RequestMeta[]>("traffic://update", (e) => cb(e.payload));

export const onWsFrames = (cb: (batch: [string, WsFrame][]) => void) =>
  listen<[string, WsFrame][]>("traffic://ws-frames", (e) => cb(e.payload));

export const onClientsUpdate = (cb: (clients: ClientInfo[]) => void) =>
  listen<ClientInfo[]>("clients://update", (e) => cb(e.payload));

// ===== AI =====

export interface AiMessage {
  role: string;
  content: string;
}

export interface AiChatParams {
  base_url: string;
  api_key: string;
  model: string;
  messages: AiMessage[];
}

export const aiChat = (params: AiChatParams) =>
  invoke<string>("ai_chat", { params });

// ===== 通用工具 =====

/** 格式化耗时 */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60000).toFixed(1)}min`;
}

/** 格式化字节 */
export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "-";
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)}MB`;
}

/** 格式化时间 */
export function formatTime(epochMs: number): string {
  const d = new Date(epochMs);
  return d.toLocaleTimeString("zh-CN", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** 方法对应的颜色 */
export function methodColor(method: string): string {
  switch (method.toUpperCase()) {
    case "GET":
      return "#2ecc71";
    case "POST":
      return "#3498db";
    case "PUT":
      return "#e67e22";
    case "DELETE":
      return "#e74c3c";
    case "PATCH":
      return "#9b59b6";
    case "HEAD":
      return "#95a5a6";
    case "OPTIONS":
      return "#1abc9c";
    default:
      return "#7f8c8d";
  }
}

/** 状态码颜色 */
export function statusColor(status: number): string {
  if (status === 0) return "#e74c3c";
  if (status < 300) return "#2ecc71";
  if (status < 400) return "#f39c12";
  if (status < 500) return "#e67e22";
  return "#e74c3c";
}
