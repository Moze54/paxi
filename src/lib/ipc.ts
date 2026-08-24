import { invoke } from "@tauri-apps/api/core";

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
  error: string | null;
}

export interface RequestRecord extends RequestMeta {
  request_body: string | null;
  response_body: string | null;
  request_headers: [string, string][];
  response_headers: [string, string][];
}

// ===== IPC 调用封装 =====

export const startProxy = (port: number) =>
  invoke<ProxyState>("start_proxy", { port });

export const stopProxy = () => invoke<void>("stop_proxy");

export const getProxyStatus = () => invoke<ProxyState>("get_proxy_status");

export const getRequests = () => invoke<RequestMeta[]>("get_requests");

export const getRequestDetail = (id: string) =>
  invoke<RequestRecord | null>("get_request_detail", { id });

export const clearRequests = () => invoke<void>("clear_requests");

export const exportCaCert = () => invoke<string>("export_ca_cert");

export const getCaCertPem = () => invoke<string>("get_ca_cert_pem");

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
