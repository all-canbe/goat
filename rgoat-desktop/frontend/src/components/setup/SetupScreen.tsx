import { Zap } from "lucide-react";
import { useConfigStore } from "../../stores/configStore";
import ProviderForm from "../provider/ProviderForm";

export default function SetupScreen() {
  const { checkConfigured } = useConfigStore();

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
        <div className="bg-surface border border-border rounded-lg p-6">
          <ProviderForm onSuccess={() => checkConfigured()} />
        </div>
      </div>
    </div>
  );
}
