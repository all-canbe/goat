import MessageList from "./MessageList";
import InputPanel from "../input/InputPanel";

interface ChatAreaProps {
  onModeChange?: (mode: string) => void;
}

export default function ChatArea({ onModeChange }: ChatAreaProps) {
  return (
    <main className="flex-1 flex flex-col overflow-hidden">
      <MessageList />
      <InputPanel onModeChange={onModeChange} />
    </main>
  );
}
