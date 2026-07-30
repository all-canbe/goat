import MessageList from "./MessageList";
import InputPanel from "../input/InputPanel";

interface ChatAreaProps {
  onModeChange?: (mode: string) => void;
  onAddProvider?: () => void;
  onManageProviders?: () => void;
}

export default function ChatArea({ onModeChange, onAddProvider, onManageProviders }: ChatAreaProps) {
  return (
    <main className="relative flex-1 flex flex-col overflow-hidden">
      <MessageList />
      <InputPanel
        onModeChange={onModeChange}
        onAddProvider={onAddProvider}
        onManageProviders={onManageProviders}
      />
    </main>
  );
}
