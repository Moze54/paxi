import { useState } from "react";
import { useAppStore } from "../lib/store";
import { Rule, RuleAction } from "../lib/ipc";
import {
  Plus,
  Trash2,
  Power,
  X,
  Save,
  Zap,
  ArrowRight,
  PauseCircle,
  Timer,
  Ban,
  Bug,
  WifiOff,
} from "lucide-react";

interface RulesPanelProps {
  onClose: () => void;
}

/** 动作类型选项 */
const ACTION_TYPES = [
  { value: "mock", label: "Mock 响应", icon: Zap },
  { value: "redirect", label: "重定向", icon: ArrowRight },
  { value: "delay", label: "请求延迟", icon: PauseCircle },
  { value: "delay_response", label: "响应延迟", icon: Timer },
  { value: "abort", label: "拦截请求", icon: Ban },
  { value: "breakpoint", label: "断点调试", icon: Bug },
  { value: "throttle", label: "弱网模拟", icon: WifiOff },
] as const;

type ActionType = (typeof ACTION_TYPES)[number]["value"];

const METHODS = ["", "GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD"];

/** 从 RuleAction 取类型标签 */
function actionTypeOf(a: RuleAction): ActionType {
  return a.type as ActionType;
}

/** 构造空规则 */
function emptyRule(): Rule {
  return {
    id: crypto.randomUUID(),
    name: "新规则",
    enabled: true,
    priority: 0,
    matcher: { host: "", path: "", method: "" },
    action: { type: "mock", params: { status: 200, content_type: "application/json", body: "{}" } },
    hits: 0,
    updated_at: Date.now(),
  };
}

export default function RulesPanel({ onClose }: RulesPanelProps) {
  const { rules, saveRule, removeRule } = useAppStore();
  const [editing, setEditing] = useState<Rule | null>(null);
  const [saving, setSaving] = useState(false);

  const handleToggle = (rule: Rule) => {
    saveRule({ ...rule, enabled: !rule.enabled, updated_at: Date.now() });
  };

  const handleDelete = (id: string) => {
    if (editing?.id === id) setEditing(null);
    removeRule(id);
  };

  const handleSave = async () => {
    if (!editing) return;
    setSaving(true);
    await saveRule({ ...editing, updated_at: Date.now() });
    setSaving(false);
    setEditing(null);
  };

  const patchAction = (partial: Record<string, unknown>) => {
    if (!editing) return;
    if (!("params" in editing.action)) return; // abort 无参数
    setEditing({
      ...editing,
      action: {
        ...editing.action,
        params: { ...editing.action.params, ...partial },
      } as RuleAction,
    });
  };

  return (
    <div className="rules-overlay" onClick={onClose}>
      <div className="rules-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h3>
            <Zap size={16} /> 规则引擎
          </h3>
          <button className="btn btn-icon" onClick={onClose}>
            <X size={14} />
          </button>
        </div>

        <div className="rules-body">
          {/* 规则列表 */}
          <div className="rules-list-pane">
            <div className="rules-list-toolbar">
              <button
                className="btn btn-primary btn-mini"
                onClick={() => setEditing(emptyRule())}
              >
                <Plus size={13} /> 新建规则
              </button>
              <span className="count-badge">
                {rules.length} 条 · {rules.filter((r) => r.enabled).length} 启用
              </span>
            </div>

            {rules.length === 0 ? (
              <div className="rules-empty">
                <p>暂无规则</p>
                <p className="hint">
                  新建规则可 Mock 响应、重定向、延迟、拦截请求
                </p>
              </div>
            ) : (
              <div className="rules-list">
                {rules.map((r) => (
                  <div
                    key={r.id}
                    className={`rule-item ${r.enabled ? "" : "disabled"} ${
                      editing?.id === r.id ? "editing" : ""
                    }`}
                    onClick={() => setEditing(structuredClone(r))}
                  >
                    <button
                      className={`rule-power ${r.enabled ? "on" : ""}`}
                      title={r.enabled ? "点击禁用" : "点击启用"}
                      onClick={(e) => {
                        e.stopPropagation();
                        handleToggle(r);
                      }}
                    >
                      <Power size={13} />
                    </button>
                    <div className="rule-info">
                      <div className="rule-name">{r.name}</div>
                      <div className="rule-desc">
                        {[
                          r.matcher.host,
                          r.matcher.method,
                          r.matcher.path,
                        ]
                          .filter(Boolean)
                          .join(" · ") || "匹配全部请求"}
                        <span className="rule-action-tag">
                          {ACTION_TYPES.find((a) => a.value === actionTypeOf(r.action))?.label}
                        </span>
                      </div>
                    </div>
                    <span className="rule-hits" title="命中次数">
                      {r.hits}
                    </span>
                    <button
                      className="btn btn-icon btn-mini"
                      title="删除"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleDelete(r.id);
                      }}
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* 编辑器 */}
          <div className="rule-editor-pane">
            {!editing ? (
              <div className="rules-empty">
                <p>选择左侧规则编辑，或新建一条</p>
              </div>
            ) : (
              <div className="rule-editor">
                <div className="editor-row">
                  <label>名称</label>
                  <input
                    value={editing.name}
                    onChange={(e) => setEditing({ ...editing, name: e.target.value })}
                  />
                </div>

                <div className="editor-row">
                  <label>优先级</label>
                  <input
                    type="number"
                    value={editing.priority}
                    onChange={(e) =>
                      setEditing({ ...editing, priority: Number(e.target.value) || 0 })
                    }
                  />
                  <span className="hint">数值越大越先匹配</span>
                </div>

                <div className="editor-section">匹配条件（留空 = 不限制）</div>
                <div className="editor-row">
                  <label>域名</label>
                  <input
                    placeholder="*.example.com"
                    value={editing.matcher.host ?? ""}
                    onChange={(e) =>
                      setEditing({
                        ...editing,
                        matcher: { ...editing.matcher, host: e.target.value },
                      })
                    }
                  />
                </div>
                <div className="editor-row">
                  <label>路径</label>
                  <input
                    placeholder="/api/v1/*"
                    value={editing.matcher.path ?? ""}
                    onChange={(e) =>
                      setEditing({
                        ...editing,
                        matcher: { ...editing.matcher, path: e.target.value },
                      })
                    }
                  />
                </div>
                <div className="editor-row">
                  <label>方法</label>
                  <select
                    value={editing.matcher.method ?? ""}
                    onChange={(e) =>
                      setEditing({
                        ...editing,
                        matcher: { ...editing.matcher, method: e.target.value },
                      })
                    }
                  >
                    {METHODS.map((m) => (
                      <option key={m} value={m}>
                        {m || "全部"}
                      </option>
                    ))}
                  </select>
                </div>

                <div className="editor-section">动作</div>
                <div className="action-type-row">
                  {ACTION_TYPES.map(({ value, label, icon: Icon }) => (
                    <button
                      key={value}
                      className={`action-chip ${actionTypeOf(editing.action) === value ? "active" : ""}`}
                      onClick={() => {
                        const params = defaultParamsFor(value);
                        setEditing({ ...editing, action: { type: value, params } as RuleAction });
                      }}
                    >
                      <Icon size={13} /> {label}
                    </button>
                  ))}
                </div>

                {/* 动作参数 */}
                {editing.action.type === "mock" && (
                  <>
                    <div className="editor-row">
                      <label>状态码</label>
                      <input
                        type="number"
                        value={editing.action.params.status}
                        onChange={(e) =>
                          patchAction({ status: Number(e.target.value) || 200 })
                        }
                      />
                    </div>
                    <div className="editor-row">
                      <label>Content-Type</label>
                      <input
                        value={editing.action.params.content_type}
                        onChange={(e) => patchAction({ content_type: e.target.value })}
                      />
                    </div>
                    <div className="editor-row col">
                      <label>响应体</label>
                      <textarea
                        rows={6}
                        className="mono-input"
                        value={editing.action.params.body}
                        onChange={(e) => patchAction({ body: e.target.value })}
                      />
                    </div>
                  </>
                )}

                {editing.action.type === "redirect" && (
                  <>
                    <div className="editor-row">
                      <label>目标 URL</label>
                      <input
                        placeholder="https://example.com/redirected"
                        value={editing.action.params.to}
                        onChange={(e) => patchAction({ to: e.target.value })}
                      />
                    </div>
                    <div className="editor-row">
                      <label>状态码</label>
                      <select
                        value={editing.action.params.status}
                        onChange={(e) => patchAction({ status: Number(e.target.value) })}
                      >
                        {[301, 302, 307, 308].map((s) => (
                          <option key={s} value={s}>
                            {s}
                          </option>
                        ))}
                      </select>
                    </div>
                  </>
                )}

                {(editing.action.type === "delay" ||
                  editing.action.type === "delay_response") && (
                  <div className="editor-row">
                    <label>延迟 (ms)</label>
                    <input
                      type="number"
                      value={editing.action.params.ms}
                      onChange={(e) => patchAction({ ms: Number(e.target.value) || 0 })}
                    />
                  </div>
                )}

                {editing.action.type === "abort" && (
                  <p className="hint">
                    命中的请求将直接返回 403，不会转发到真实服务器。
                  </p>
                )}

                {editing.action.type === "breakpoint" && (
                  <p className="hint">
                    命中的请求将<strong>挂起</strong>，弹出断点面板：可查看/修改请求后放行，
                    或直接拦截（403）。同一时间多个断点按先后排队处理，挂起超过 5 分钟自动放行。
                  </p>
                )}

                {editing.action.type === "throttle" && (
                  <>
                    <div className="editor-row">
                      <label>带宽上限 (KB/s，0 不限)</label>
                      <input
                        type="number"
                        value={editing.action.params.kbps}
                        onChange={(e) => patchAction({ kbps: Number(e.target.value) || 0 })}
                      />
                    </div>
                    <div className="editor-row">
                      <label>首字节延迟 (ms)</label>
                      <input
                        type="number"
                        value={editing.action.params.delay_ms}
                        onChange={(e) => patchAction({ delay_ms: Number(e.target.value) || 0 })}
                      />
                    </div>
                    <div className="editor-row">
                      <label>丢包率 (%)</label>
                      <input
                        type="number"
                        min={0}
                        max={100}
                        value={editing.action.params.drop_pct}
                        onChange={(e) =>
                          patchAction({ drop_pct: Math.min(100, Math.max(0, Number(e.target.value) || 0)) })
                        }
                      />
                    </div>
                    <p className="hint">
                      限速按响应体大小模拟传输耗时；丢包按概率截断响应体（客户端会收到不完整数据），
                      用于测试弱网下的容错。
                    </p>
                  </>
                )}

                <div className="editor-actions">
                  <button className="btn btn-primary" onClick={handleSave} disabled={saving}>
                    <Save size={14} /> {saving ? "保存中…" : "保存规则"}
                  </button>
                  <button className="btn btn-ghost" onClick={() => setEditing(null)}>
                    取消
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

/** 切换动作类型时的默认参数 */
function defaultParamsFor(type: ActionType): Record<string, unknown> {
  switch (type) {
    case "mock":
      return { status: 200, content_type: "application/json", body: "{}" };
    case "redirect":
      return { to: "", status: 302 };
    case "delay":
    case "delay_response":
      return { ms: 1000 };
    case "abort":
      return {};
    case "breakpoint":
      return {};
    case "throttle":
      return { kbps: 64, delay_ms: 500, drop_pct: 0 };
  }
}
