import { useEffect, useCallback } from 'react'
import { Bot, Code2, FolderTree, Zap } from 'lucide-react'
import { motion, useReducedMotion } from 'framer-motion'
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
  const prefersReducedMotion = useReducedMotion()

  const floatAnimation = prefersReducedMotion
    ? undefined
    : {
        y: [0, -8, 0],
        rotate: [0, -1.5, 1.5, 0],
      }

  const blinkAnimation = prefersReducedMotion
    ? undefined
    : {
        scaleY: [1, 1, 1, 0.12, 1, 1, 1, 1, 0.12, 1],
      }

  return (
    <motion.div
      className="relative mx-auto mb-6 flex h-32 w-32 items-center justify-center"
      animate={floatAnimation}
      transition={{
        duration: 4.6,
        repeat: Infinity,
        ease: 'easeInOut',
      }}
      whileHover={
        prefersReducedMotion
          ? undefined
          : {
              scale: 1.04,
              rotate: 2,
            }
      }
    >
      <motion.div
        className="absolute inset-3 rounded-[2.4rem] bg-primary/20 blur-2xl"
        animate={
          prefersReducedMotion
            ? undefined
            : {
                scale: [0.92, 1.08, 0.92],
                opacity: [0.42, 0.72, 0.42],
              }
        }
        transition={{ duration: 3.8, repeat: Infinity, ease: 'easeInOut' }}
      />
      <motion.div
        className="absolute inset-0 rounded-[2.8rem] border border-primary/15 bg-gradient-to-br from-primary/10 via-background to-amber-200/20 shadow-[0_24px_60px_rgba(30,41,59,0.14)]"
        animate={
          prefersReducedMotion
            ? undefined
            : {
                boxShadow: [
                  '0 24px 60px rgba(30,41,59,0.10)',
                  '0 30px 72px rgba(30,41,59,0.18)',
                  '0 24px 60px rgba(30,41,59,0.10)',
                ],
              }
        }
        transition={{ duration: 4.2, repeat: Infinity, ease: 'easeInOut' }}
      />

      <motion.div
        className="absolute -left-2 top-5 h-3 w-3 rounded-full bg-amber-300/70 blur-[1px]"
        animate={
          prefersReducedMotion
            ? undefined
            : {
                y: [0, -8, 0],
                opacity: [0.4, 0.9, 0.4],
              }
        }
        transition={{ duration: 2.8, repeat: Infinity, ease: 'easeInOut' }}
      />
      <motion.div
        className="absolute -right-1 bottom-6 h-2.5 w-2.5 rounded-full bg-primary/50 blur-[1px]"
        animate={
          prefersReducedMotion
            ? undefined
            : {
                y: [0, 7, 0],
                opacity: [0.25, 0.7, 0.25],
              }
        }
        transition={{ duration: 3.4, repeat: Infinity, ease: 'easeInOut' }}
      />

      <motion.svg
        viewBox="0 0 120 120"
        className="relative h-24 w-24 text-primary drop-shadow-[0_10px_20px_rgba(37,99,235,0.18)]"
        fill="none"
        aria-hidden="true"
      >
        <motion.g
          animate={
            prefersReducedMotion
              ? undefined
              : {
                  rotate: [0, -5, 2, 0],
                }
          }
          transition={{ duration: 2.6, repeat: Infinity, ease: 'easeInOut' }}
          style={{ transformOrigin: '36px 28px' }}
        >
          <line x1="36" y1="39" x2="26" y2="20" stroke="currentColor" strokeWidth="5" strokeLinecap="round" />
          <circle cx="26" cy="20" r="6" fill="currentColor" />
          <circle cx="26" cy="20" r="2.5" fill="white" />
        </motion.g>

        <motion.g
          animate={
            prefersReducedMotion
              ? undefined
              : {
                  rotate: [0, 4, -2, 0],
                }
          }
          transition={{ duration: 2.8, repeat: Infinity, ease: 'easeInOut', delay: 0.2 }}
          style={{ transformOrigin: '84px 28px' }}
        >
          <line x1="84" y1="39" x2="94" y2="20" stroke="currentColor" strokeWidth="5" strokeLinecap="round" />
          <circle cx="94" cy="20" r="6" fill="currentColor" />
          <circle cx="94" cy="20" r="2.5" fill="white" />
        </motion.g>

        <circle cx="18" cy="66" r="7" fill="currentColor" opacity="0.92" />
        <circle cx="102" cy="66" r="7" fill="currentColor" opacity="0.92" />

        <rect x="26" y="40" width="68" height="56" rx="20" fill="currentColor" />
        <rect x="32" y="50" width="56" height="36" rx="14" fill="white" />

        <motion.ellipse
          cx="42"
          cy="74"
          rx="5.5"
          ry="3.4"
          fill="#fb7185"
          animate={
            prefersReducedMotion
              ? undefined
              : {
                  opacity: [0.32, 0.7, 0.32],
                  scale: [0.96, 1.08, 0.96],
                }
          }
          transition={{ duration: 2.4, repeat: Infinity, ease: 'easeInOut' }}
        />
        <motion.ellipse
          cx="78"
          cy="74"
          rx="5.5"
          ry="3.4"
          fill="#fb7185"
          animate={
            prefersReducedMotion
              ? undefined
              : {
                  opacity: [0.32, 0.7, 0.32],
                  scale: [0.96, 1.08, 0.96],
                }
          }
          transition={{ duration: 2.4, repeat: Infinity, ease: 'easeInOut', delay: 0.18 }}
        />

        <motion.g
          animate={blinkAnimation}
          transition={{
            duration: 5.2,
            repeat: Infinity,
            ease: 'easeInOut',
            times: [0, 0.3, 0.58, 0.62, 0.66, 0.8, 0.88, 0.92, 0.96, 1],
          }}
          style={{ transformOrigin: '60px 64px' }}
        >
          <circle cx="46" cy="64" r="5.5" fill="currentColor" />
          <circle cx="47.8" cy="62.2" r="1.9" fill="white" />
          <circle cx="74" cy="64" r="5.5" fill="currentColor" />
          <circle cx="75.8" cy="62.2" r="1.9" fill="white" />
        </motion.g>

        <motion.path
          d="M 52 76 Q 60 82 68 76"
          stroke="currentColor"
          strokeWidth="3"
          strokeLinecap="round"
          animate={
            prefersReducedMotion
              ? undefined
              : {
                  d: [
                    'M 52 76 Q 60 82 68 76',
                    'M 52 76 Q 60 85 68 76',
                    'M 52 76 Q 60 82 68 76',
                  ],
                }
          }
          transition={{ duration: 2.8, repeat: Infinity, ease: 'easeInOut' }}
        />

        <rect x="36" y="93" width="16" height="10" rx="5" fill="currentColor" />
        <rect x="68" y="93" width="16" height="10" rx="5" fill="currentColor" />
      </motion.svg>
    </motion.div>
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
  const loadBots = useChatStore((s) => s.loadBots)
  const goHome = useChatStore((s) => s.goHome)
  const currentSessionId = useChatStore((s) => s.currentSessionId)
  const clearCurrent = useChatStore((s) => s.clearCurrent)

  useEffect(() => {
    loadConfig()
    loadSkills()
    loadBots()
  }, [loadConfig, loadSkills, loadBots])

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
        <button
          type="button"
          onClick={goHome}
          className="flex items-center gap-2 rounded-md px-1 py-0.5 transition-colors hover:bg-accent/60"
          title="返回封面页"
        >
          <MiniRobotIcon className="h-5 w-5 text-primary" />
          <h1 className="text-sm font-semibold tracking-tight">
            rust<span className="text-primary">-agent</span>
          </h1>
        </button>
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
