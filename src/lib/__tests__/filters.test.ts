import { describe, it, expect } from "vitest";
import { filterRequests } from "../filters";
import { RequestMeta } from "../ipc";

const m = (over: Partial<RequestMeta>): RequestMeta => ({
  id: "x",
  method: "GET",
  url: "https://a.com/p",
  host: "a.com",
  scheme: "https",
  status: 200,
  duration_ms: 1,
  started_at: 1,
  is_websocket: false,
  ws_frame_count: 0,
  error: null,
  client_ip: null,
  client_process: null,
  request_body_size: 0,
  response_body_size: 0,
  content_type: null,
  is_replay: false,
  passthrough: false,
  ...over,
});

const reqs = [
  m({ id: "1", url: "https://api.a.com/users", host: "api.a.com", method: "GET", status: 200 }),
  m({ id: "2", url: "http://b.com/ads", host: "b.com", scheme: "http", method: "POST", status: 404 }),
  m({ id: "3", url: "wss://c.com/ws", host: "c.com", scheme: "wss", is_websocket: true, status: 101 }),
  m({ id: "4", url: "https://d.com/x", host: "d.com", status: 0, error: "转发失败" }),
];

describe("filterRequests", () => {
  it("关键字匹配 url/host/method/status", () => {
    expect(filterRequests(reqs, "api", "", "", "")).toHaveLength(1);
    expect(filterRequests(reqs, "POST", "", "", "")).toHaveLength(1);
    expect(filterRequests(reqs, "404", "", "", "")).toHaveLength(1);
    expect(filterRequests(reqs, "b.com", "", "", "")).toHaveLength(1);
  });

  it("方法筛选", () => {
    expect(filterRequests(reqs, "", "GET", "", "").map((r) => r.id)).toEqual(["1", "3", "4"]);
  });

  it("状态筛选（error 与 2xx/4xx）", () => {
    expect(filterRequests(reqs, "", "", "2", "").map((r) => r.id)).toEqual(["1"]);
    expect(filterRequests(reqs, "", "", "4", "").map((r) => r.id)).toEqual(["2"]);
    expect(filterRequests(reqs, "", "", "error", "").map((r) => r.id)).toEqual(["4"]);
  });

  it("协议筛选（ws 与 https）", () => {
    expect(filterRequests(reqs, "", "", "", "ws").map((r) => r.id)).toEqual(["3"]);
    expect(filterRequests(reqs, "", "", "", "https").map((r) => r.id)).toEqual(["1", "4"]);
  });

  it("组合条件 AND", () => {
    expect(filterRequests(reqs, "a.com", "GET", "2", "https").map((r) => r.id)).toEqual(["1"]);
    expect(filterRequests(reqs, "a.com", "POST", "2", "https")).toHaveLength(0);
  });

  it("空过滤返回全部", () => {
    expect(filterRequests(reqs, "", "", "", "")).toHaveLength(4);
  });

  it("收藏过滤（starOnly）", () => {
    const marks = {
      "1": { star: true, color: "red" },
      "3": { star: true, color: "" },
    };
    const out = filterRequests(reqs, "", "", "", "", true, marks);
    expect(out.map((r) => r.id)).toEqual(["1", "3"]);
    // starOnly=false 时 marks 不影响
    const all = filterRequests(reqs, "", "", "", "", false, marks);
    expect(all).toHaveLength(4);
  });

  it("来源应用筛选（进程名 / 设备 IP）", () => {
    const withProc = [
      m({ id: "p1", client_process: "chrome.exe", client_ip: "127.0.0.1" }),
      m({ id: "p2", client_process: "wechat.exe", client_ip: "127.0.0.1" }),
      m({ id: "p3", client_process: null, client_ip: "192.168.1.8" }),
      m({ id: "p4", client_process: null, client_ip: "192.168.1.8" }),
    ];
    expect(filterRequests(withProc, "", "", "", "", false, {}, "chrome.exe").map((r) => r.id)).toEqual(["p1"]);
    expect(filterRequests(withProc, "", "", "", "", false, {}, "📱 192.168.1.8").map((r) => r.id)).toEqual(["p3", "p4"]);
    // 空筛选返回全部
    expect(filterRequests(withProc, "", "", "", "", false, {}, "")).toHaveLength(4);
  });
});
