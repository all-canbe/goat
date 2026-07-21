import { useState, useMemo, useCallback, FormEvent } from "react";
import { Loader2, Globe, Key, Cpu, Tag } from "lucide-react";
import { useConfigStore } from "../../stores/configStore";

interface ProviderFormProps {
  /** 提交成功后调用（例如关闭弹窗或刷新 UI）。 */
  onSuccess?: () => void;
  /** 取消按钮回调；不提供则不显示取消按钮。 */
  onCancel?: () => void;
  /** 初始字段值。 */
  initialValues?: {
    baseUrl?: string;
    apiKey?: string;
    model?: string;
    name?: string;
  };
}

const RESERVED_NAMES = ["deepseek", "openai", "anthropic"];

export default function ProviderForm({
  onSuccess,
  onCancel,
  initialValues,
}: ProviderFormProps) {
  const { configureProvider } = useConfigStore();

  const [baseUrl, setBaseUrl] = useState(
    initialValues?.baseUrl ?? "https://api.openai.com/v1"
  );
  const [apiKey, setApiKey] = useState(initialValues?.apiKey ?? "");
  const [model, setModel] = useState(initialValues?.model ?? "gpt-4o");
  const [name, setName] = useState(initialValues?.name ?? "OpenAI");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const providerType = useMemo(() => {
    const lower = baseUrl.toLowerCase();
    if (lower.includes("anthropic")) return "Anthropic";
    return "OpenAI Compatible";
  }, [baseUrl]);

  const validate = useCallback((nameValue: string): string => {
    if (!nameValue.trim()) return "Provider name is required";
    if (RESERVED_NAMES.includes(nameValue.trim().toLowerCase())) {
      return `Name "${nameValue}" is reserved for environment/fallback providers`;
    }
    return "";
  }, []);

  const handleSubmit = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      setError("");

      if (!baseUrl.trim() || !apiKey.trim() || !model.trim()) {
        setError("All fields are required");
        return;
      }
      const nameErr = validate(name);
      if (nameErr) {
        setError(nameErr);
        return;
      }

      setLoading(true);
      try {
        await configureProvider(
          baseUrl.trim(),
          apiKey.trim(),
          model.trim(),
          name.trim()
        );
        onSuccess?.();
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setError(msg || "Failed to configure provider");
      } finally {
        setLoading(false);
      }
    },
    [baseUrl, apiKey, model, name, configureProvider, validate, onSuccess]
  );

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      {/* Base URL */}
      <div>
        <label className="flex items-center gap-1.5 text-xs font-medium text-text-secondary mb-1.5">
          <Globe size={12} />
          Base URL
        </label>
        <input
          type="text"
          className="w-full px-3 py-2 rounded-md bg-bg border border-border text-text text-sm outline-none focus:border-primary transition-colors"
          placeholder="https://api.openai.com/v1"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
        />
      </div>

      {/* API Key */}
      <div>
        <label className="flex items-center gap-1.5 text-xs font-medium text-text-secondary mb-1.5">
          <Key size={12} />
          API Key
        </label>
        <input
          type="password"
          className="w-full px-3 py-2 rounded-md bg-bg border border-border text-text text-sm outline-none focus:border-primary transition-colors"
          placeholder="sk-..."
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
        />
      </div>

      {/* Model Name */}
      <div>
        <label className="flex items-center gap-1.5 text-xs font-medium text-text-secondary mb-1.5">
          <Cpu size={12} />
          Model Name
        </label>
        <input
          type="text"
          className="w-full px-3 py-2 rounded-md bg-bg border border-border text-text text-sm outline-none focus:border-primary transition-colors"
          placeholder="gpt-4o"
          value={model}
          onChange={(e) => setModel(e.target.value)}
        />
      </div>

      {/* Provider Name */}
      <div>
        <label className="flex items-center gap-1.5 text-xs font-medium text-text-secondary mb-1.5">
          <Tag size={12} />
          Provider Name
        </label>
        <input
          type="text"
          className="w-full px-3 py-2 rounded-md bg-bg border border-border text-text text-sm outline-none focus:border-primary transition-colors"
          placeholder="OpenAI"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      </div>

      {/* Provider type indicator */}
      <div className="text-xs text-text-secondary">
        Detected type:{" "}
        <span className="text-brand font-medium">{providerType}</span>
      </div>

      {/* Error */}
      {error && (
        <div className="text-xs text-error bg-error-subtle rounded-md px-3 py-2">
          {error}
        </div>
      )}

      {/* Actions */}
      <div className="flex items-center gap-2">
        <button
          type="submit"
          disabled={loading}
          className="flex-1 py-2.5 rounded-md bg-primary text-white font-semibold text-sm hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
        >
          {loading ? (
            <>
              <Loader2 size={16} className="animate-spin" />
              Configuring...
            </>
          ) : (
            "Save & Continue"
          )}
        </button>
        {onCancel && (
          <button
            type="button"
            onClick={onCancel}
            disabled={loading}
            className="px-4 py-2.5 rounded-md border border-border text-text-secondary text-sm hover:bg-surface-hover transition-colors"
          >
            Cancel
          </button>
        )}
      </div>
    </form>
  );
}
