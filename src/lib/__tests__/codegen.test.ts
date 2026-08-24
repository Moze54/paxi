import { describe, it, expect } from "vitest";
import { toCurl, toFetch, toAxios, toPython } from "../codegen";
import { RequestRecord } from "../ipc";

const rec: RequestRecord = {
  id: "t1",
  client_ip: "127.0.0.1",
  client_process: "test-app.exe",
  method: "POST",
  url: "https://api.example.com/v1/users?page=2",
  host: "api.example.com",
  scheme: "https",
  status: 200,
  duration_ms: 10,
  started_at: 1700000000000,
  is_websocket: false,
  ws_frame_count: 0,
  error: null,
  request_body_size: 18,
  response_body_size: 5,
  content_type: "application/json",
  is_replay: false,
  passthrough: false,
  request_body: '{"name":"张三"}',
  response_body: '{"ok":true}',
  request_headers: [
    ["content-type", "application/json"],
    ["authorization", "Bearer abc"],
    ["host", "api.example.com"],
    ["connection", "keep-alive"],
    ["content-length", "18"],
  ],
  response_headers: [],
  matched_rule: null,
};

describe("toCurl", () => {
  it("bash 版转义并跳过 hop-by-hop 头", () => {
    const s = toCurl(rec, false);
    expect(s).toContain("curl -X POST");
    expect(s).toContain("-H 'content-type: application/json'");
    expect(s).toContain("-H 'authorization: Bearer abc'");
    // host/connection/content-length 应被剔除
    expect(s).not.toContain("host:");
    expect(s).not.toContain("connection:");
    expect(s).not.toContain("content-length");
    expect(s).toMatch(/-d '\\?\{"name":/);
    expect(s).toContain("'https://api.example.com/v1/users?page=2'");
  });

  it("windows 版用双引号与 ^ 续行", () => {
    const s = toCurl(rec, true);
    expect(s).toContain('-X POST');
    expect(s).toContain("^");
    expect(s).toContain('"content-type: application/json"');
  });

  it("二进制 body 不放入 -d", () => {
    const bin = { ...rec, request_body: "[二进制内容，10 字节]" };
    expect(toCurl(bin, false)).not.toContain("-d ");
  });
});

describe("toFetch", () => {
  it("生成 JSON.stringify 的 body", () => {
    const s = toFetch(rec);
    expect(s).toContain('fetch("https://api.example.com/v1/users?page=2"');
    expect(s).toContain('method: "POST"');
    expect(s).toContain("JSON.stringify(");
    expect(s).toContain('"authorization": "Bearer abc"');
  });
});

describe("toAxios", () => {
  it("拆分 pathname 与 params", () => {
    const s = toAxios(rec);
    expect(s).toContain('url: "/v1/users"');
    expect(s).toContain('params: "page=2"');
    expect(s).toContain('method: "post"');
  });
});

describe("toPython", () => {
  it("json 与 requests 调用", () => {
    const s = toPython(rec);
    expect(s).toContain("import requests");
    expect(s).toContain("url = ");
    expect(s).toContain("headers = ");
    expect(s).toContain("json = ");
    expect(s).toContain("requests.post(url, headers=headers, json=json)");
  });

  it("无 body 时省略 json 参数", () => {
    const get = { ...rec, method: "GET", request_body: null };
    const s = toPython(get);
    expect(s).toContain("requests.get(url, headers=headers)");
    expect(s).not.toContain("json = ");
  });
});
