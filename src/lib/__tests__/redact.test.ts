import { describe, it, expect } from "vitest";
import { redactHeaders, redactBody, redactForAI, maskValue } from "../redact";

describe("maskValue", () => {
  it("保留首尾字符", () => {
    expect(maskValue("abc12345")).toBe("ab****45");
  });
  it("短值整体打码", () => {
    expect(maskValue("ab")).toBe("****");
  });
});

describe("redactHeaders", () => {
  it("打码敏感头", () => {
    const out = redactHeaders([
      ["authorization", "Bearer secret-token-value"],
      ["cookie", "session=abc"],
      ["x-api-key", "key123"],
      ["accept", "application/json"],
    ]);
    expect(out[0][1]).toContain("已脱敏");
    expect(out[0][1]).not.toContain("secret-token-value");
    expect(out[1][1]).toContain("已脱敏");
    expect(out[2][1]).toContain("已脱敏");
    expect(out[3]).toEqual(["accept", "application/json"]);
  });
});

describe("redactBody", () => {
  it("JSON 递归打码敏感字段", () => {
    const body = JSON.stringify({
      token: "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc",
      password: "p@ssw0rd",
      config: { api_key: "k123", url: "https://ok.com" },
      count: 3,
      nested: { deep: { userToken: "xyz" } },
    });
    const redacted = JSON.parse(redactBody(body)!);
    expect(redacted.token).toContain("已脱敏");
    expect(redacted.password).toContain("已脱敏");
    expect(redacted.config.api_key).toContain("已脱敏");
    expect(redacted.nested.deep.userToken).toContain("已脱敏");
    // 非敏感字段原样
    expect(redacted.count).toBe(3);
    expect(redacted.config.url).toBe("https://ok.com");
  });

  it("非 JSON 打码 token 样式", () => {
    const text =
      "Authorization header: Bearer sk-abcdef1234567890 long key a81c9d2e4f6a1b2c3d4e5f60718293a4b";
    const out = redactBody(text)!;
    expect(out).toContain("已脱敏");
    expect(out).not.toContain("sk-abcdef1234567890");
    expect(out).not.toContain("a81c9d2e4f6a1b2c3d4e5f60718293a4b");
  });

  it("二进制占位与空 body 原样", () => {
    expect(redactBody("[二进制内容，10 字节]")).toBe("[二进制内容，10 字节]");
    expect(redactBody(null)).toBeNull();
  });
});

describe("redactForAI", () => {
  it("整体脱敏", () => {
    const out = redactForAI({
      request_headers: [["token", "abc"]],
      request_body: "{\"secret\":\"v\"}",
      response_headers: [["set-cookie", "sid=1"]],
      response_body: "plain text no secrets",
    });
    expect(out.request_headers[0][1]).toContain("已脱敏");
    expect(out.request_body).toContain("已脱敏");
    expect(out.response_headers[0][1]).toContain("已脱敏");
    expect(out.response_body).toBe("plain text no secrets");
  });
});