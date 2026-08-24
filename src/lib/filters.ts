import { RequestMeta } from "./ipc";

/**
 * 列表过滤逻辑（列表组件与键盘导航共用）。
 * 全部条件为 AND 关系。
 */
export function filterRequests(
  requests: RequestMeta[],
  filter: string,
  methodFilter: string,
  statusFilter: string,
  schemeFilter: string,
  starOnly = false,
  marks: Record<string, { star: boolean; color: string }> = {},
  /** 来源应用筛选：进程名 或 设备 IP（"" = 全部） */
  processFilter = ""
): RequestMeta[] {
  return requests.filter((r) => {
    // 仅看收藏
    if (starOnly && !marks[r.id]?.star) return false;
    // 来源应用
    if (processFilter) {
      const source = r.client_process ?? `📱 ${r.client_ip ?? "unknown"}`;
      if (source !== processFilter) return false;
    }
    // 关键字搜索
    if (filter) {
      const f = filter.toLowerCase();
      const match =
        r.url.toLowerCase().includes(f) ||
        r.method.toLowerCase().includes(f) ||
        String(r.status).includes(f) ||
        r.host.toLowerCase().includes(f);
      if (!match) return false;
    }
    // 方法筛选
    if (methodFilter && r.method.toUpperCase() !== methodFilter) return false;
    // 状态筛选
    if (statusFilter) {
      if (statusFilter === "error") {
        if (r.status !== 0) return false;
      } else {
        const prefix = statusFilter[0]; // "2" / "3" / "4" / "5"
        if (Math.floor(r.status / 100) !== Number(prefix)) return false;
      }
    }
    // 协议筛选
    if (schemeFilter) {
      if (schemeFilter === "ws") {
        if (!r.is_websocket) return false;
      } else if (r.scheme !== schemeFilter) {
        return false;
      }
    }
    return true;
  });
}
