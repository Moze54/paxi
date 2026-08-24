import { useEffect, useState } from "react";
import { RequestRecord, aiChat } from "../lib/ipc";
import { Sparkles, X, Loader2 } from "lucide-react";

interface AiConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
}

const CONFIG_KEY = "paxi.ai.config";

function loadConfig(): AiConfig {
  try {
    const raw = localStorage.getItem(CONFIG_KEY);
    if (raw) return JSON.parse(raw);
  } catch {}
  return { baseUrl: "https://api.deepseek.com", apiKey: "", model: "deepseek-chat" };
}

interface AiPanelProps {
  record: RequestRecord | null;
  onClose: () => void;
}

export default function AiPanel({ record, onClose }: AiPanelProps) {
  const [config, setConfig] = useState<AiConfig>(loadConfig);
  const [showSettings, setShowSettings] = useState(false);
  const [result, setResult] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string>("");

  useEffect(() => {
    setResult("");
    setError("");
  }, [record]);

  const saveConfig = (c: AiConfig) => {
    setConfig(c);
    localStorage.setItem(CONFIG_KEY, JSON.stringify(c));
  };

  const buildPrompt = (r: RequestRecord): string => {
    return `请分析以下这个网络请求，帮我理解它的用途和特征：

方法：${r.method}
URL：${r.url}
协议：${r.scheme.toUpperCase()}
状态码：${r.status}
耗时：${r.duration_ms}ms

请求头：
${r.request_headers.map(([k, v]) => `  ${k}: ${v}`).join("\n")}

请求体：
${r.request_body || "（无）"}

响应头：
${r.response_headers.map(([k, v]) => `  ${k}: ${v}`).join("\n")}

响应体（可能被截断）：
${r.response_body || "（无）"}

请从以下几个角度分析：
1. 这个接口的用途是什么（属于什么业务功能）
2. 请求/响应中的关键参数和字段含义
3. 是否存在明显的加密、签名或鉴权机制
4. 响应数据类型和结构特征
5. 是否存在潜在的安全问题或值得注意的信息`;
  };

  const analyze = async () => {
    if (!record) return;
    setLoading(true);
    setError("");
    setResult("");
    try {
      const content = await aiChat({
        base_url: config.baseUrl,
        api_key: config.apiKey,
        model: config.model,
        messages: [
          {
            role: "system",
            content:
              "你是一名精通网络协议与逆向分析的专家，擅长分析 HTTP 请求抓包数据。",
          },
          { role: "user", content: buildPrompt(record) },
        ],
      });
      setResult(content);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="ai-panel">
      <div className="ai-header">
        <span className="ai-title">
          <Sparkles size={16} /> AI 分析
        </span>
        <button className="btn btn-icon" onClick={onClose}>
          <X size={16} />
        </button>
      </div>

      {!record ? (
        <p className="hint">没有选中的请求</p>
      ) : (
        <>
          <div className="ai-record-summary">
            <span className="method-tag" style={{ background: "#3498db" }}>
              {record.method}
            </span>
            <span className="summary-url">{record.url}</span>
          </div>

          <div className="ai-actions">
            <button
              className="btn btn-ai"
              onClick={analyze}
              disabled={loading || !config.apiKey}
              title={!config.apiKey ? "请先在下方配置 API Key" : ""}
            >
              {loading ? <Loader2 size={14} className="spin" /> : <Sparkles size={14} />}
              {loading ? "分析中…" : "开始分析"}
            </button>
            <button
              className="btn btn-ghost"
              onClick={() => setShowSettings(!showSettings)}
            >
              {showSettings ? "收起设置" : "AI 设置"}
            </button>
          </div>

          {showSettings && (
            <div className="ai-settings">
              <label>
                API Base URL
                <input
                  type="text"
                  value={config.baseUrl}
                  placeholder="https://api.deepseek.com"
                  onChange={(e) =>
                    setConfig({ ...config, baseUrl: e.target.value })
                  }
                />
              </label>
              <label>
                API Key
                <input
                  type="password"
                  value={config.apiKey}
                  placeholder="sk-..."
                  onChange={(e) =>
                    setConfig({ ...config, apiKey: e.target.value })
                  }
                />
              </label>
              <label>
                模型名
                <input
                  type="text"
                  value={config.model}
                  placeholder="deepseek-chat"
                  onChange={(e) =>
                    setConfig({ ...config, model: e.target.value })
                  }
                />
              </label>
              <button
                className="btn btn-primary"
                onClick={() => saveConfig(config)}
              >
                保存配置
              </button>
            </div>
          )}

          {error && <div className="ai-error">{error}</div>}

          {result && (
            <div className="ai-result">
              <pre>{result}</pre>
            </div>
          )}
        </>
      )}
    </div>
  );
}
