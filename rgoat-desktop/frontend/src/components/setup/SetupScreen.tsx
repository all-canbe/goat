import { useState, useMemo, useCallback, FormEvent } from "react";
import { Loader2, Zap, Globe, Key, Cpu, Tag } from "lucide-react";
import { useConfigStore } from "../../stores/configStore";

export default function SetupScreen() {
  const { configureProvider, checkConfigured } = useConfigStore();

  const [baseUrl, setBaseUrl] = useState("https://api.openai.com/v1");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("gpt-4o");
  const [name, setName] = useState("OpenAI");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const providerType = useMemo(() => {
    const lower = baseUrl.toLowerCase();
    if (lower.includes("anthropic")) return "Anthropic";
    if (lower.includes("openai")) return "OpenAI Compatible";
    return "OpenAI Compatible";
  }, [baseUrl]);

  const handleSubmit = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      setError("");

      if (!baseUrl.trim() || !apiKey.trim() || !model.trim() || !name.trim()) {
        setError("All fields are required");
        return;
      }

      setLoading(true);
      try {
        await configureProvider(baseUrl.trim(), apiKey.trim(), model.trim(), name.trim());
        await checkConfigured();
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setError(msg || "Failed to configure provider");
      } finally {
        setLoading(false);
      }
    },
    [baseUrl, apiKey, model, name, configureProvider, checkConfigured]
  );

  return (
    <div className="h-screen bg-bg flex items-center justify-center p-4">
      <div className="w-full max-w-md">
        {/* Logo */}
        <div className="text-center mb-8">
          <div className="w-14 h-14 rounded-xl bg-primary flex items-center justify-center mx-auto mb-3">
            <Zap size={28} className="text-white" />
          </div>
          <h1 className="text-xl font-bold text-text">RGoat Desktop</h1>
          <p className="text-sm text-text-secondary mt-1">
            Configure your first AI provider to get started
          </p>
        </div>

        {/* Form */}
        <form
          onSubmit={handleSubmit}
          className="bg-surface border border-border rounded-lg p-6 space-y-4"
        >
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

          {/* Submit */}
          <button
            type="submit"
            disabled={loading}
            className="w-full py-2.5 rounded-md bg-primary text-white font-semibold text-sm hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
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
        </form>
      </div>
    </div>
  );
}
