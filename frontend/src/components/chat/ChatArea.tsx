import MessageList from './MessageList'
import InputPanel from '@/components/input/InputPanel'

export default function ChatArea() {
  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <MessageList />
      <InputPanel />
    </div>
  )
}