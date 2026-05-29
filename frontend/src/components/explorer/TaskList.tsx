import { useTaskStore } from '@/stores/taskStore'
import { Loader, CheckCircle, XCircle, Ban } from 'lucide-react'

const statusIcons: Record<string, React.ReactNode> = {
  running: <Loader size={12} className="animate-spin text-info" />,
  completed: <CheckCircle size={12} className="text-success" />,
  failed: <XCircle size={12} className="text-error" />,
  cancelled: <Ban size={12} className="text-text-darker" />,
}

export default function TaskList() {
  const tasks = useTaskStore((s) => s.tasks)

  if (tasks.length === 0) {
    return (
      <div className="px-3 py-2 text-xs text-text-darker">
        暂无后台任务
      </div>
    )
  }

  return (
    <div className="max-h-[120px] overflow-y-auto">
      {tasks.map((task) => (
        <div key={task.id} className="px-3 py-1.5">
          <div className="flex items-center gap-1.5">
            {statusIcons[task.status]}
            <span className="text-xs text-text-dim truncate">{task.name}</span>
          </div>
          {task.status === 'running' && (
            <div className="mt-1 w-full bg-surface-light rounded-full h-1">
              <div
                className="bg-info h-1 rounded-full transition-all duration-300"
                style={{ width: `${Math.min(task.progress, 100)}%` }}
              />
            </div>
          )}
        </div>
      ))}
    </div>
  )
}