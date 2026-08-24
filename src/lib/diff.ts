/**
 * 简单行级 diff（LCS 算法）。
 * 限制行数以保护性能；超过限制时退化为"整体不同"。
 */

export interface DiffLine {
  type: "same" | "add" | "del";
  text: string;
}

const MAX_LINES = 2000;

/** 计算两个文本的行 diff（a = 旧/源，b = 新/重放） */
export function diffLines(a: string, b: string): DiffLine[] {
  const aLines = a.split("\n");
  const bLines = b.split("\n");

  if (aLines.length > MAX_LINES || bLines.length > MAX_LINES) {
    // 超限退化：直接展示两段
    return [
      ...aLines.map((text) => ({ type: "del" as const, text })),
      ...bLines.map((text) => ({ type: "add" as const, text })),
    ];
  }

  // LCS 动态规划表（(n+1)×(m+1)）
  const n = aLines.length;
  const m = bLines.length;
  // 为控制内存，超大概率相同的超大文本直接比对
  if (n * m > 4_000_000) {
    return a === b
      ? aLines.map((text) => ({ type: "same" as const, text }))
      : [
          ...aLines.map((text) => ({ type: "del" as const, text })),
          ...bLines.map((text) => ({ type: "add" as const, text })),
        ];
  }

  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = aLines[i] === bLines[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  // 回溯生成 diff
  const result: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (aLines[i] === bLines[j]) {
      result.push({ type: "same", text: aLines[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      result.push({ type: "del", text: aLines[i] });
      i++;
    } else {
      result.push({ type: "add", text: bLines[j] });
      j++;
    }
  }
  while (i < n) {
    result.push({ type: "del", text: aLines[i] });
    i++;
  }
  while (j < m) {
    result.push({ type: "add", text: bLines[j] });
    j++;
  }
  return result;
}

/** 统计 diff：新增/删除行数 */
export function diffStat(diff: DiffLine[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const d of diff) {
    if (d.type === "add") added++;
    else if (d.type === "del") removed++;
  }
  return { added, removed };
}
