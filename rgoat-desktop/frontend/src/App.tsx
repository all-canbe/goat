import { useEffect } from "react";
import AppLayout from "./components/layout/AppLayout";
import SetupScreen from "./components/setup/SetupScreen";
import { useConfigStore } from "./stores/configStore";

function App() {
  const { hasConfiguredProvider, checkConfigured, loading } = useConfigStore();

  useEffect(() => {
    checkConfigured();
  }, [checkConfigured]);

  if (loading) {
    return (
      <div className="h-screen bg-bg flex items-center justify-center">
        <div className="text-text-secondary text-sm">Loading...</div>
      </div>
    );
  }

  if (!hasConfiguredProvider) {
    return <SetupScreen />;
  }

  return <AppLayout />;
}

export default App;
