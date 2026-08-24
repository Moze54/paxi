import { describe, it, expect } from "vitest";
import { diffLines, diffStat } from "../diff";

describe("diffLines", () => {
  it("相同文本全部 same", () => {
    const d = diffLines("a\nb\nc", "a\nb\nc");
    expect(d.every((x) => x.type === "same")).toBe(true);
    expect(d).toHaveLength(3);
  });

  it("纯新增", () => {
    const d = diffLines("", "x\ny");
    expect(d.filter((x) => x.type === "add")).toHaveLength(2);
  });

  it("纯删除", () => {
    const d = diffLines("x\ny", "");
    expect(d.filter((x) => x.type === "del")).toHaveLength(2);
  });

  it("中间行修改 = 一删一增", () => {
    const d = diffLines("a\nb\nc", "a\nB\nc");
    expect(d).toEqual([
      { type: "same", text: "a" },
      { type: "del", text: "b" },
      { type: "add", text: "B" },
      { type: "same", text: "c" },
    ]);
  });

  it("diffStat 统计正确", () => {
    const { added, removed } = diffStat(diffLines("a\nb", "a\nB\nnew"));
    expect(added).toBe(2); // B + new
    expect(removed).toBe(1); // b
  });

  it("超长文本退化不崩溃", () => {
    const big = Array.from({ length: 3000 }, (_, i) => `l${i}`).join("\n");
    const d = diffLines(big, big + "\nextra");
    expect(d.length).toBeGreaterThan(0);
  });
});
