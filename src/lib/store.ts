import { create } from "zustand";
import {
  ProxyState,
  RequestMeta,
  RequestRecord,
  ClientInfo,
  Rule,
  BreakpointInfo,
  BreakpointDecision,
  getProxyStatus,
  getRequests,
  getRequestDetail,
  startProxy,
  stopProxy,
  clearRequests,
  getClients,
  listRules,
  upsertRule,
  deleteRule,
  onTrafficNew,
  onTrafficUpdate,
  onClientsUpdate,
  onBreakpoint,
  listBreakpoints,
  resumeBreakpoint,
} from "./ipc";
import { filterRequests } from "./filters";

/** 前端内存中保留的最大条数（列表只展示这么多，更多靠后端存储） */
const MAX_FRONTEND_RECORDS = 10_000;

export type Theme = "dark" | "light";

interface AppStore {
  // 代理状态
  proxy: ProxyState;
  // 请求列表（最新在前）
  requests: RequestMeta[];
  // 当前选中请求的详情
  selectedId: string | null;
  selectedDetail: RequestRecord | null;
  // 已连接的客户端设备
  clients: ClientInfo[];
  // 规则列表（优先级降序）
  rules: Rule[];
  // 挂起中的断点
  breakpoints: BreakpointInfo[];
  // 请求标记：id -> 收藏/颜色
  marks: Record<string, RequestMark>;
  // 仅看收藏
  starOnly: boolean;
  // 来源应用筛选
  processFilter: string;
  // 主题
  theme: Theme;
  // 过滤条件
  filter: string;
  // 分类筛选
  methodFilter: string; // "" | "GET" | "POST" | ...
  statusFilter: string; // "" | "2xx" | "3xx" | "4xx" | "5xx" | "error"
  schemeFilter: string; // "" | "http" | "https" | "ws"
  // 加载状态
  loading: boolean;
  // 错误提示
  error: string | null;
  // 事件监听是否已初始化
  listenersReady: boolean;

  // actions
  init: () => Promise<void>;
  toggleProxy: () => Promise<void>;
  refreshProxyStatus: () => Promise<void>;
  refreshRequests: () => Promise<void>;
  selectRequest: (id: string) => Promise<void>;
  moveSelection: (delta: number) => void;
  clearAll: () => Promise<void>;
  setFilter: (f: string) => void;
  setMethodFilter: (m: string) => void;
  setStatusFilter: (s: string) => void;
  setSchemeFilter: (s: string) => void;
  setProcessFilter: (p: string) => void;
  setError: (e: string | null) => void;
  // 规则
  loadRules: () => Promise<void>;
  saveRule: (rule: Rule) => Promise<void>;
  removeRule: (id: string) => Promise<void>;
  // 断点
  resumeBreakpoint: (bpId: string, decision: BreakpointDecision) => Promise<void>;
  // 标记
  toggleStar: (id: string) => void;
  setMarkColor: (id: string, color: string) => void;
  toggleStarOnly: () => void;
  // 主题
  initTheme: () => void;
  toggleTheme: () => void;
}

/** 增量合并：新批次插入头部，同 id 去重（保留最新） */
function mergeNewRequests(existing: RequestMeta[], batch: RequestMeta[]): RequestMeta[] {
  if (batch.length === 0) return existing;
  const batchIds = new Set(batch.map((r) => r.id));
  const kept = existing.filter((r) => !batchIds.has(r.id));
  const merged = [...batch.reverse(), ...kept]; // batch 最新在后，反转后最新在前
  return merged.length > MAX_FRONTEND_RECORDS
    ? merged.slice(0, MAX_FRONTEND_RECORDS)
    : merged;
}

/** 更新合并：用更新后的 meta 替换列表中的同 id 项 */
function applyUpdates(existing: RequestMeta[], updates: RequestMeta[]): RequestMeta[] {
  if (updates.length === 0) return existing;
  const updateMap = new Map(updates.map((u) => [u.id, u]));
  return existing.map((r) => updateMap.get(r.id) ?? r);
}

/** 请求标记（收藏 + 标注色），localStorage 持久化 */
export interface RequestMark {
  star: boolean;
  /** 标注色键："" | "red" | "yellow" | "green" | "blue" | "purple" */
  color: string;
}

const MARKS_KEY = "paxi.marks.v1";

function loadMarks(): Record<string, RequestMark> {
  try {
    return JSON.parse(localStorage.getItem(MARKS_KEY) || "{}");
  } catch {
    return {};
  }
}

function saveMarks(marks: Record<string, RequestMark>) {
  localStorage.setItem(MARKS_KEY, JSON.stringify(marks));
}

export const useAppStore = create<AppStore>((set, get) => ({
  proxy: { running: false, port: 8888, local_ip: "" },
  requests: [],
  selectedId: null,
  selectedDetail: null,
  clients: [],
  rules: [],
  theme: "dark",
  filter: "",
  methodFilter: "",
  statusFilter: "",
  schemeFilter: "",
  loading: false,
  error: null,
  listenersReady: false,
  breakpoints: [],
  marks: loadMarks(),
  starOnly: false,
  processFilter: "",

  init: async () => {
    if (get().listenersReady) return;
    set({ listenersReady: true });

    // 订阅事件
    const unsubs: (() => void)[] = [];
    unsubs.push(
      await onTrafficNew((batch) => {
        set((s) => ({ requests: mergeNewRequests(s.requests, batch) }));
      })
    );
    unsubs.push(
      await onTrafficUpdate((batch) => {
        set((s) => ({ requests: applyUpdates(s.requests, batch) }));
      })
    );
    unsubs.push(
      await onClientsUpdate((clients) => {
        set({ clients });
      })
    );

    // 断点命中：加入挂起列表（事件可能早于面板打开到达，先收集）
    unsubs.push(
      await onBreakpoint((info) => {
        set((s) => ({ breakpoints: [...s.breakpoints.filter((b) => b.bp_id !== info.bp_id), info] }));
      })
    );

    // 初始数据
    await get().refreshProxyStatus();
    await get().refreshRequests();
    await get().loadRules();
    try {
      set({ clients: await getClients() });
    } catch {
      /* 忽略 */
    }
    // 应用重启后可能仍有挂起断点（代理进程内状态不会跨重启，这里兜底拉一次）
    try {
      set({ breakpoints: await listBreakpoints() });
    } catch {
      /* 忽略 */
    }
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    unsubs; // 持有引用避免被 GC（应用生命周期内不退订）
  },

  toggleProxy: async () => {
    const { proxy } = get();
    set({ loading: true, error: null });
    try {
      if (proxy.running) {
        await stopProxy();
        set({ proxy: { ...proxy, running: false } });
      } else {
        const state = await startProxy(proxy.port || 8888);
        set({ proxy: state });
      }
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ loading: false });
    }
  },

  refreshProxyStatus: async () => {
    try {
      const state = await getProxyStatus();
      set({ proxy: state });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  refreshRequests: async () => {
    try {
      const list = await getRequests();
      set({ requests: list });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  selectRequest: async (id) => {
    set({ selectedId: id });
    try {
      const detail = await getRequestDetail(id);
      // 若用户在加载期间又选了别的请求，丢弃过期结果
      if (get().selectedId !== id) return;
      set({ selectedDetail: detail });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  moveSelection: (delta) => {
    const { requests, filter, methodFilter, statusFilter, schemeFilter, selectedId } = get();
    const filtered = filterRequests(requests, filter, methodFilter, statusFilter, schemeFilter);
    if (filtered.length === 0) return;
    const idx = filtered.findIndex((r) => r.id === selectedId);
    // 未选中或找不到时，向上跳到第一条、向下跳到最后一条附近的当前项
    let next: number;
    if (idx === -1) {
      next = delta > 0 ? 0 : filtered.length - 1;
    } else {
      next = Math.min(filtered.length - 1, Math.max(0, idx + delta));
    }
    if (next !== idx) {
      get().selectRequest(filtered[next].id);
    }
  },

  clearAll: async () => {
    try {
      await clearRequests();
      set({ requests: [], selectedId: null, selectedDetail: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setFilter: (f) => set({ filter: f }),
  setMethodFilter: (m) => set({ methodFilter: m }),
  setStatusFilter: (s) => set({ statusFilter: s }),
  setSchemeFilter: (s) => set({ schemeFilter: s }),
  setProcessFilter: (p) => set({ processFilter: p }),
  setError: (e) => set({ error: e }),

  // ===== 规则 =====
  loadRules: async () => {
    try {
      set({ rules: await listRules() });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  saveRule: async (rule) => {
    try {
      await upsertRule(rule);
      await get().loadRules();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  removeRule: async (id) => {
    try {
      await deleteRule(id);
      await get().loadRules();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  // ===== 断点 =====
  resumeBreakpoint: async (bpId, decision) => {
    // 先本地移除，再调后端（乐观更新；失败重新拉列表兜底）
    set((s) => ({ breakpoints: s.breakpoints.filter((b) => b.bp_id !== bpId) }));
    try {
      await resumeBreakpoint(bpId, decision);
    } catch (e) {
      set({ error: String(e) });
      try {
        set({ breakpoints: await listBreakpoints() });
      } catch {
        /* 忽略 */
      }
    }
  },

  // ===== 标记 =====
  toggleStar: (id) => {
    set((s) => {
      const marks = { ...s.marks };
      const m = marks[id] ?? { star: false, color: "" };
      marks[id] = { ...m, star: !m.star };
      saveMarks(marks);
      return { marks };
    });
  },

  setMarkColor: (id, color) => {
    set((s) => {
      const marks = { ...s.marks };
      const m = marks[id] ?? { star: false, color: "" };
      marks[id] = { ...m, color };
      saveMarks(marks);
      return { marks };
    });
  },

  toggleStarOnly: () => set((s) => ({ starOnly: !s.starOnly })),

  // ===== 主题 =====
  initTheme: () => {
    const saved = localStorage.getItem("paxi.theme") as Theme | null;
    const theme: Theme =
      saved ?? (window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
    set({ theme });
    document.documentElement.dataset.theme = theme;
  },

  toggleTheme: () => {
    const next: Theme = get().theme === "dark" ? "light" : "dark";
    localStorage.setItem("paxi.theme", next);
    document.documentElement.dataset.theme = next;
    set({ theme: next });
  },
}));
