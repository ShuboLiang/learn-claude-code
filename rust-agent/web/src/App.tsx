import { useEffect, useCallback } from 'react'
import { SessionList } from '@/components/SessionList'
import { ChatPane } from '@/components/ChatPane'
import { useChatStore } from '@/store/chat'

function App() {
  const loadSessions = useChatStore((s) => s.loadSessions)
  const currentSessionId = useChatStore((s) => s.currentSessionId)
  const createSession = useChatStore((s) => s.createSession)
  const clearCurrent = useChatStore((s) => s.clearCurrent)

  useEffect(() => {
    loadSessions()
  }, [loadSessions])

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey) {
        if (e.key === 'n' || e.key === 'N') {
          e.preventDefault()
          createSession()
        } else if (e.key === 'l' || e.key === 'L') {
          e.preventDefault()
          if (currentSessionId) clearCurrent()
        }
      }
    },
    [createSession, clearCurrent, currentSessionId],
  )

  useEffect(() => {
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [handleKeyDown])

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      {/* Thin header */}
      <header className="flex h-10 shrink-0 items-center border-b px-4">
        <h1 className="text-sm font-semibold tracking-tight">rust-agent</h1>
      </header>

      {/* Main content */}
      <div className="flex flex-1 overflow-hidden">
        <SessionList />
        {currentSessionId ? (
          <ChatPane />
        ) : (
          <div className="flex flex-1 items-center justify-center text-muted-foreground">
            <p className="text-sm">Create or select a session to start</p>
          </div>
        )}
      </div>
    </div>
  )
}

export default App
