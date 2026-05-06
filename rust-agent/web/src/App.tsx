import { useEffect, useCallback } from 'react'
import { ChevronDown, MessageSquare, Sparkles } from 'lucide-react'
import { SessionList } from '@/components/SessionList'
import { ChatPane } from '@/components/ChatPane'
import { WorkspacePanel } from '@/components/WorkspacePanel'
import { useChatStore } from '@/store/chat'

function App() {
  const loadSessions = useChatStore((s) => s.loadSessions)
  const loadConfig = useChatStore((s) => s.loadConfig)
  const currentSessionId = useChatStore((s) => s.currentSessionId)
  const createSession = useChatStore((s) => s.createSession)
  const clearCurrent = useChatStore((s) => s.clearCurrent)
  const profiles = useChatStore((s) => s.profiles)
  const selectedProfile = useChatStore((s) => s.selectedProfile)
  const selectedModel = useChatStore((s) => s.selectedModel)
  const setSelectedProfile = useChatStore((s) => s.setSelectedProfile)
  const setSelectedModel = useChatStore((s) => s.setSelectedModel)

  const currentModels =
    profiles.find((p) => p.name === selectedProfile)?.models || []

  useEffect(() => {
    loadConfig()
  }, [loadConfig])

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
      {/* Header */}
      <header className="flex h-11 shrink-0 items-center gap-3 border-b bg-background/80 backdrop-blur px-4">
        <div className="flex items-center gap-2">
          <span className="flex h-5 w-5 items-center justify-center rounded-md bg-primary text-[10px] font-bold text-primary-foreground shadow-sm shadow-primary/25">
            R
          </span>
          <h1 className="text-sm font-semibold tracking-tight">
            rust<span className="text-primary">-agent</span>
          </h1>
        </div>

        {/* Profile + Model 选择器 */}
        <div className="flex items-center gap-2">
          <div className="relative">
            <select
              value={selectedProfile}
              onChange={(e) => setSelectedProfile(e.target.value)}
              className="h-7 rounded-md border bg-background px-2 pr-6 text-xs appearance-none cursor-pointer hover:border-primary/50 focus:outline-none focus:ring-1 focus:ring-primary"
            >
              {profiles.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}
                </option>
              ))}
            </select>
            <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground pointer-events-none" />
          </div>

          <div className="relative">
            <select
              value={selectedModel}
              onChange={(e) => setSelectedModel(e.target.value)}
              className="h-7 rounded-md border bg-background px-2 pr-6 text-xs appearance-none cursor-pointer hover:border-primary/50 focus:outline-none focus:ring-1 focus:ring-primary"
            >
              {currentModels.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
            <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground pointer-events-none" />
          </div>
        </div>

        <span className="ml-auto text-[10px] text-muted-foreground/60">
          Ctrl+N new &middot; Ctrl+L clear
        </span>
      </header>

      {/* Main content */}
      <div className="flex flex-1 overflow-hidden">
        <SessionList />
        {currentSessionId ? (
          <ChatPane />
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-4 text-muted-foreground">
            <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-muted shadow-sm">
              <Sparkles className="h-7 w-7 text-primary/60" />
            </div>
            <div className="text-center">
              <p className="text-lg font-semibold text-foreground">
                rust-agent
              </p>
              <p className="mt-1 text-sm text-muted-foreground">
                AI-powered coding assistant
              </p>
            </div>
            <button
              onClick={() => createSession()}
              className="mt-2 inline-flex items-center gap-2 rounded-xl bg-primary px-4 py-2.5 text-sm font-medium text-primary-foreground shadow-sm shadow-primary/20 transition-all hover:bg-primary/90 hover:shadow-md active:scale-[0.98]"
            >
              <MessageSquare className="h-4 w-4" />
              New Session
            </button>
          </div>
        )}
        <WorkspacePanel />
      </div>
    </div>
  )
}

export default App
