import { ReactNode } from 'react'
import Sidebar from './Sidebar'
import StatusBar from './StatusBar'

interface AppLayoutProps {
  children: ReactNode
}

export default function AppLayout({ children }: AppLayoutProps) {
  return (
    <div className="h-screen flex flex-col bg-bg">
      <div className="flex-1 flex overflow-hidden">
        <Sidebar />
        <main className="flex-1 flex flex-col overflow-hidden">
          {children}
        </main>
      </div>
      <StatusBar />
    </div>
  )
}