import { create } from "zustand";
import {
  ProxyState,
  RequestMeta,
  RequestRecord,
  getProxyStatus,
  getRequests,
  getRequestDetail,
  startProxy,
  stopProxy,
  clearRequests,
} from "./ipc";

interface AppStore {
  // 代理状态
  proxy: ProxyState;
  // 请求列表
  requests: RequestMeta[];
  // 当前选中请求的详情
  selectedId: string | null;
  selectedDetail: RequestRecord | null;
  // 过滤条件
  filter: string;
  // 加载状态
  loading: boolean;
  // 错误提示
  error: string | null;

  // actions
  toggleProxy: () => Promise<void>;
  refreshProxyStatus: () => Promise<void>;
  refreshRequests: () => Promise<void>;
  selectRequest: (id: string) => Promise<void>;
  clearAll: () => Promise<void>;
  setFilter: (f: string) => void;
  setError: (e: string | null) => void;
}

export const useAppStore = create<AppStore>((set, get) => ({
  proxy: { running: false, port: 8888, local_ip: "" },
  requests: [],
  selectedId: null,
  selectedDetail: null,
  filter: "",
  loading: false,
  error: null,

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

  selectRequest: async (id: string) => {
    set({ selectedId: id });
    try {
      const detail = await getRequestDetail(id);
      set({ selectedDetail: detail });
    } catch (e) {
      set({ error: String(e) });
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
  setError: (e) => set({ error: e }),
}));
