import { useMemo, useState } from "react";
import { RequestRecord } from "../lib/ipc";
import { toCurl, toFetch, toAxios, toPython } from "../lib/codegen";
import { Code2, Copy, Check, X } from "lucide-react";

interface CodegenDialogProps {
  record: RequestRecord;
  onClose: () => void;
}

type Lang = "curl" | "curl-win" | "fetch" | "axios" | "python";

const LANGS: { value: Lang; label: string }[] = [
  { value: "curl", label: "cURL (bash)" },
  { value: "curl-win", label: "cURL (Windows)" },
  { value: "fetch", label: "fetch" },
  { value: "axios", label: "axios" },
  { value: "python", label: "Python" },
];

export default function CodegenDialog({ record, onClose }: CodegenDialogProps) {
  const [lang, setLang] = useState<Lang>("curl");
  const [copied, setCopied] = useState(false);

  const code = useMemo(() => {
    switch (lang) {
      case "curl":
        return toCurl(record, false);
      case "curl-win":
        return toCurl(record, true);
      case "fetch":
        return toFetch(record);
      case "axios":
        return toAxios(record);
      case "python":
        return toPython(record);
    }
  }, [lang, record]);

  const copy = async () => {
    await navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="rules-overlay" onClick={onClose}>
      <div className="codegen-panel" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <h3>
            <Code2 size={15} /> 生成代码
          </h3>
          <button className="btn btn-icon" onClick={onClose}>
            <X size={14} />
          </button>
        </div>

        <div className="codegen-toolbar">
          <div className="action-type-row">
            {LANGS.map((l) => (
              <button
                key={l.value}
                className={`action-chip ${lang === l.value ? "active" : ""}`}
                onClick={() => setLang(l.value)}
              >
                {l.label}
              </button>
            ))}
          </div>
          <button className="btn btn-ghost btn-mini" onClick={copy}>
            {copied ? <Check size={13} /> : <Copy size={13} />}
            {copied ? "已复制" : "复制"}
          </button>
        </div>

        <pre className="codegen-code">{code}</pre>
      </div>
    </div>
  );
}
