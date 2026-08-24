import { RequestRecord } from "./ipc";

/**
 * 从抓包记录生成各语言/工具的请求代码。
 * 生成的是"将要发出的请求"，因此以 request_headers / request_body 为源。
 */

/** bash 单引号转义：' → '\'' */
function shQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** Windows cmd 双引号转义 */
function winQuote(s: string): string {
  return `"${s.replace(/"/g, '""')}"`;
}

/** 从头部列表取值 */
function getHeader(headers: [string, string][], name: string): string | undefined {
  return headers.find(([k]) => k.toLowerCase() === name)?.[1];
}

/** 跳过 hop-by-hop 头 */
const SKIP_HEADERS = new Set([
  "host",
  "connection",
  "proxy-connection",
  "content-length",
  "keep-alive",
  "transfer-encoding",
  "upgrade",
]);

function usefulHeaders(record: RequestRecord): [string, string][] {
  return record.request_headers.filter(
    ([k]) => !SKIP_HEADERS.has(k.toLowerCase())
  );
}

/** 生成 cURL 命令 */
export function toCurl(record: RequestRecord, windows = false): string {
  const q = windows ? winQuote : shQuote;
  const parts: string[] = [`curl -X ${record.method}`];

  for (const [k, v] of usefulHeaders(record)) {
    parts.push(`-H ${q(`${k}: ${v}`)}`);
  }

  if (record.request_body && !record.request_body.startsWith("[二进制内容")) {
    parts.push(`-d ${q(record.request_body)}`);
  }

  parts.push(q(record.url));
  const joined = parts.join(windows ? " ^\n  " : " \\\n  ");
  return windows ? joined : joined;
}

/** 生成 fetch（JavaScript）代码 */
export function toFetch(record: RequestRecord): string {
  const headers: Record<string, string> = {};
  for (const [k, v] of usefulHeaders(record)) headers[k] = v;

  const hasBody = !!record.request_body && !record.request_body.startsWith("[二进制内容");
  const ct = getHeader(record.request_headers, "content-type") ?? "";

  let bodyArg = "";
  if (hasBody && record.request_body) {
    if (ct.includes("json")) {
      bodyArg = `JSON.stringify(${tryPrettyJson(record.request_body)})`;
    } else {
      bodyArg = JSON.stringify(record.request_body);
    }
  }

  const lines = [
    `fetch(${JSON.stringify(record.url)}, {`,
    `  method: ${JSON.stringify(record.method)},`,
    `  headers: ${JSON.stringify(headers, null, 2).replace(/\n/g, "\n  ")},`,
  ];
  if (bodyArg) {
    lines.push(`  body: ${bodyArg},`);
  }
  lines.push(`});`);
  return lines.join("\n");
}

/** 生成 axios（JavaScript）代码 */
export function toAxios(record: RequestRecord): string {
  const headers: Record<string, string> = {};
  for (const [k, v] of usefulHeaders(record)) headers[k] = v;

  const urlObj = safeUrl(record.url);
  const lines = [
    `import axios from "axios";`,
    ``,
    `const { data } = await axios({`,
    `  url: ${JSON.stringify(urlObj?.pathname ?? record.url)},`,
  ];
  if (urlObj?.search) lines.push(`  params: ${JSON.stringify(urlObj.search.slice(1))},`);
  lines.push(`  method: ${JSON.stringify(record.method.toLowerCase())},`);
  if (Object.keys(headers).length > 0) {
    lines.push(`  headers: ${JSON.stringify(headers, null, 2).replace(/\n/g, "\n  ")},`);
  }
  const ct = getHeader(record.request_headers, "content-type") ?? "";
  if (record.request_body && !record.request_body.startsWith("[二进制内容")) {
    if (ct.includes("json")) {
      lines.push(`  data: ${tryPrettyJson(record.request_body)},`);
    } else {
      lines.push(`  data: ${JSON.stringify(record.request_body)},`);
    }
  }
  lines.push(`});`);
  return lines.join("\n");
}

/** 生成 Python requests 代码 */
export function toPython(record: RequestRecord): string {
  const headers: Record<string, string> = {};
  for (const [k, v] of usefulHeaders(record)) headers[k] = v;

  const pyDict = (o: Record<string, string>) =>
    JSON.stringify(o, null, 4)
      .replace(/\n/g, "\n    ")
      .replace(/\{/g, "{")
      .replace(/\}/g, "}");

  const lines = [
    `import requests`,
    ``,
    `url = ${JSON.stringify(record.url)}`,
  ];
  if (Object.keys(headers).length > 0) {
    lines.push(`headers = ${pyDict(headers)}`);
  }

  const ct = getHeader(record.request_headers, "content-type") ?? "";
  if (record.request_body && !record.request_body.startsWith("[二进制内容")) {
    if (ct.includes("json")) {
      lines.push(`json = ${tryPrettyJson(record.request_body)}`);
      lines.push(``);
      lines.push(
        Object.keys(headers).length > 0
          ? `resp = requests.${record.method.toLowerCase()}(url, headers=headers, json=json)`
          : `resp = requests.${record.method.toLowerCase()}(url, json=json)`
      );
    } else {
      lines.push(`data = ${JSON.stringify(record.request_body)}`);
      lines.push(``);
      lines.push(
        Object.keys(headers).length > 0
          ? `resp = requests.${record.method.toLowerCase()}(url, headers=headers, data=data)`
          : `resp = requests.${record.method.toLowerCase()}(url, data=data)`
      );
    }
  } else {
    lines.push(``);
    lines.push(
      Object.keys(headers).length > 0
        ? `resp = requests.${record.method.toLowerCase()}(url, headers=headers)`
        : `resp = requests.${record.method.toLowerCase()}(url)`
    );
  }
  lines.push(`print(resp.status_code)`);
  lines.push(`print(resp.text)`);
  return lines.join("\n");
}

/** JSON 字符串 → 紧凑对象字面量；解析失败返回原字符串字面量 */
function tryPrettyJson(body: string): string {
  try {
    const parsed = JSON.parse(body);
    return JSON.stringify(parsed, null, 2).replace(/\n/g, "\n  ");
  } catch {
    return JSON.stringify(body);
  }
}

function safeUrl(url: string): URL | null {
  try {
    return new URL(url);
  } catch {
    return null;
  }
}
