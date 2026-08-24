/**
 * AI 分析前的敏感信息脱敏：
 * - 请求头：authorization / cookie / set-cookie / proxy-authorization / x-api-key 等值打码
 * - 请求/响应体（JSON）：token / password / secret / sign / key / appsecret 等字段值打码
 * - 非 JSON 文本：匹配常见 token 样式的字符串打码
 */

const SENSITIVE_HEADERS = new Set([
  "authorization",
  "proxy-authorization",
  "cookie",
  "set-cookie",
  "x-api-key",
  "api-key",
  "apikey",
  "token",
  "x-access-token",
  "x-auth-token",
  "session",
  "sessionid",
  "x-session",
  "pwd",
  "password",
]);

const SENSITIVE_KEYS = new Set([
  "token",
  "access_token",
  "refresh_token",
  "password",
  "passwd",
  "pwd",
  "secret",
  "appsecret",
  "app_secret",
  "sign",
  "signature",
  "auth",
  "apikey",
  "api_key",
  "private_key",
  "secretkey",
  "sessionkey",
  "session_key",
  "cookie",
]);

/** 打码一段值：保留前后各 2 字符，中间以 * 替换；过短则整体打码 */
export function maskValue(v: string): string {
  const s = String(v);
  if (s.length <= 4) return "****";
  return `${s.slice(0, 2)}${"*".repeat(Math.min(8, s.length - 4))}${s.slice(-2)}`;
}

/** 头部列表脱敏（返回新数组） */
export function redactHeaders(headers: [string, string][]): [string, string][] {
  return headers.map(([k, v]) =>
    SENSITIVE_HEADERS.has(k.toLowerCase()) ? [k, maskValue(v) + " [已脱敏]"] : [k, v]
  );
}

/** 判断 key 是否敏感（精确或包含敏感词） */
function isSensitiveKey(k: string): boolean {
  const key = String(k).toLowerCase();
  if (SENSITIVE_KEYS.has(key)) return true;
  return (
    key.includes("token") ||
    key.includes("secret") ||
    key.includes("password") ||
    key.includes("passwd") ||
    key.includes("apikey") ||
    key.includes("api_key") ||
    key.includes("private_key") ||
    key.includes("sessionkey") ||
    key.includes("session_key") ||
    key.includes("sign") ||
    key.includes("cookie")
  );
}

/** 单个 JSON 值脱敏（递归） */
function redactJsonValue(key: string | null, value: unknown, depth: number): unknown {
  if (depth > 8) return value;
  if (Array.isArray(value)) {
    return value.map((v) => redactJsonValue(key, v, depth + 1));
  }
  if (value !== null && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      out[k] = isSensitiveKey(k)
        ? maskValue(typeof v === "string" ? v : JSON.stringify(v)) + " [已脱敏]"
        : redactJsonValue(k, v, depth + 1);
    }
    return out;
  }
  if (typeof value === "string" && key !== null && isSensitiveKey(key)) {
    return maskValue(value);
  }
  return value;
}

/** 请求体脱敏：JSON 递归打码敏感字段；非 JSON 匹配常见 token 模式 */
export function redactBody(body: string | null): string | null {
  if (!body) return body;
  // 二进制占位符原样保留
  if (body.startsWith("[二进制内容")) return body;
  try {
    const parsed = JSON.parse(body);
    return JSON.stringify(redactJsonValue(null, parsed, 0), null, 2);
  } catch {
    // 非 JSON：打码常见 token/密钥样式（jwt、base64、长 hex）
    return body.replace(
      /(\beyJ[A-Za-z0-9_\-./=]{20,}\b)|(\b(?:sk|token|secret|key)[-_][A-Za-z0-9_\-]{8,}\b)|([A-Fa-f0-9]{32,})/g,
      (m) => maskValue(m) + " [已脱敏]"
    );
  }
}

/** 完整脱敏：headers + body（分别处理请求和响应） */
export function redactForAI(record: {
  request_headers: [string, string][];
  request_body: string | null;
  response_headers: [string, string][];
  response_body: string | null;
}): {
  request_headers: [string, string][];
  request_body: string | null;
  response_headers: [string, string][];
  response_body: string | null;
} {
  return {
    request_headers: redactHeaders(record.request_headers),
    request_body: redactBody(record.request_body),
    response_headers: redactHeaders(record.response_headers),
    response_body: redactBody(record.response_body),
  };
}