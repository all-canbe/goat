import MessageList from './MessageList'
import InputPanel from '@/components/input/InputPanel'
import SkillInstallDialog from '@/components/skill/SkillInstallDialog'

export default function ChatArea() {
  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <MessageList />
      <SkillInstallDialog />
      <InputPanel />
    </div>
  )
}