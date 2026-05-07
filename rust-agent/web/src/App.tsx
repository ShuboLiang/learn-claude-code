import { useEffect, useCallback } from 'react'
import { Bot, Code2, FolderTree, Zap, Sparkles } from 'lucide-react'
import { SessionList } from '@/components/SessionList'
import { ChatPane } from '@/components/ChatPane'
import { WorkspacePanel } from '@/components/WorkspacePanel'
import { useChatStore } from '@/store/chat'

function MiniRobotIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 100 100" className={className} fill="none">
      <rect x="20" y="35" width="60" height="50" rx="18" fill="currentColor" />
      <rect x="26" y="45" width="48" height="32" rx="12" fill="white" />
      <circle cx="38" cy="58" r="5" fill="currentColor" />
      <circle cx="39.5" cy="56.5" r="1.8" fill="white" />
      <circle cx="62" cy="58" r="5" fill="currentColor" />
      <circle cx="63.5" cy="56.5" r="1.8" fill="white" />
      <path d="M 45 68 Q 50 72 55 68" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" fill="none" />
      <line x1="32" y1="35" x2="24" y2="20" stroke="currentColor" strokeWidth="4" strokeLinecap="round" />
      <circle cx="24" cy="20" r="5" fill="currentColor" />
      <circle cx="24" cy="20" r="2" fill="white" />
      <line x1="68" y1="35" x2="76" y2="20" stroke="currentColor" strokeWidth="4" strokeLinecap="round" />
      <circle cx="76" cy="20" r="5" fill="currentColor" />
      <circle cx="76" cy="20" r="2" fill="white" />
      <ellipse cx="32" cy="65" rx="4" ry="2.5" fill="currentColor" opacity="0.25" />
      <ellipse cx="68" cy="65" rx="4" ry="2.5" fill="currentColor" opacity="0.25" />
      <circle cx="12" cy="55" r="6" fill="currentColor" />
      <circle cx="88" cy="55" r="6" fill="currentColor" />
      <rect x="30" y="82" width="14" height="10" rx="5" fill="currentColor" />
      <rect x="56" y="82" width="14" height="10" rx="5" fill="currentColor" />
    </svg>
  )
}

function CuteLogo() {
  return (
    <div className="relative mx-auto mb-6 flex h-24 w-24 items-center justify-center">
      <div className="absolute inset-0 rounded-[2rem] bg-primary/15 blur-2xl" />
      <MiniRobotIcon className="relative h-full w-full text-primary drop-shadow-lg" />
    </div>
  )
}

const FEATURES = [
  {
    icon: <Code2 className="h-5 w-5 text-primary" />,
    title: '智能代码生成',
    desc: '基于上下文理解，自动生成高质量代码片段',
  },
  {
    icon: <Bot className="h-5 w-5 text-primary" />,
    title: 'Bot 子代理',
    desc: '专业 Bot 处理特定任务，代码审查、架构设计',
  },
  {
    icon: <FolderTree className="h-5 w-5 text-primary" />,
    title: '实时文件浏览',
    desc: '内置工作区文件树，实时监听文件变更',
  },
  {
    icon: <Zap className="h-5 w-5 text-primary" />,
    title: '流式响应',
    desc: 'SSE 实时推送，思考过程与工具调用可视化',
  },
]

function App() {
  const loadSessions = useChatStore((s) => s.loadSessions)
  const loadConfig = useChatStore((s) => s.loadConfig)
  const loadSkills = useChatStore((s) => s.loadSkills)
  const currentSessionId = useChatStore((s) => s.currentSessionId)
  const clearCurrent = useChatStore((s) => s.clearCurrent)

  useEffect(() => {
    loadConfig()
    loadSkills()
  }, [loadConfig, loadSkills])

  useEffect(() => {
    loadSessions()
  }, [loadSessions])

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey) {
        if (e.key === 'l' || e.key === 'L') {
          e.preventDefault()
          if (currentSessionId) clearCurrent()
        }
      }
    },
    [clearCurrent, currentSessionId],
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
          <MiniRobotIcon className="h-5 w-5 text-primary" />
          <h1 className="text-sm font-semibold tracking-tight">
            rust<span className="text-primary">-agent</span>
          </h1>
        </div>
      </header>

      {/* Main content */}
      <div className="flex flex-1 overflow-hidden">
        <SessionList />
        {currentSessionId ? (
          <ChatPane />
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center overflow-y-auto px-8 py-12">
            <div className="mx-auto max-w-xl text-center">
              {/* 品牌 */}
              <CuteLogo />
              <div className="mb-2 text-4xl font-bold tracking-tight">
                rust<span className="text-primary">-agent</span>
              </div>
              <p className="mb-10 text-base text-muted-foreground">
                专为开发者打造的 AI 编程助手，支持多会话管理、Bot 子代理、实时文件浏览
              </p>

              {/* 特性卡片 */}
              <div className="grid grid-cols-2 gap-4">
                {FEATURES.map((f) => (
                  <div
                    key={f.title}
                    className="rounded-xl border bg-card p-4 text-left shadow-sm transition-colors hover:border-primary/30 hover:bg-accent/40"
                  >
                    <div className="mb-2">{f.icon}</div>
                    <h3 className="text-sm font-semibold">{f.title}</h3>
                    <p className="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
                      {f.desc}
                    </p>
                  </div>
                ))}
              </div>

              <p className="mt-8 text-xs text-muted-foreground/60">
                点击左侧"新建会话"开始对话
              </p>
            </div>
          </div>
        )}
        <WorkspacePanel />
      </div>
    </div>
  )
}

export default App
