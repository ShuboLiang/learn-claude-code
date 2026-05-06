import { useState, useRef, useCallback, useMemo, type KeyboardEvent } from 'react'
import { Square, ArrowUp, Trash2, Bot, CornerDownLeft } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useChatStore } from '@/store/chat'

const COMMANDS: { cmd: string; label: string; desc: string; icon: React.ReactNode }[] = [
  { cmd: '/clear', label: '清空上下文', desc: '清空当前会话的对话历史', icon: <Trash2 className="h-3.5 w-3.5" /> },
  { cmd: '/bots', label: '查看机器人', desc: '列出所有可用的 Bot 助手', icon: <Bot className="h-3.5 w-3.5" /> },
]

export function Composer() {
  const [text, setText] = useState('')
  const [cmdIndex, setCmdIndex] = useState(0)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const sendMessage = useChatStore((s) => s.sendMessage)
  const cancelStream = useChatStore((s) => s.cancelStream)
  const streaming = useChatStore((s) => s.streaming)
  const clearCurrent = useChatStore((s) => s.clearCurrent)
  const handleCommand = useChatStore((s) => s.handleCommand)

  // 命令提示逻辑
  const isCmdMode = useMemo(() => {
    const t = text.trimStart()
    return t.startsWith('/') && !t.includes(' ') && COMMANDS.some((c) => c.cmd.startsWith(t))
  }, [text])

  const matchingCmds = useMemo(() => {
    if (!text.trimStart().startsWith('/')) return []
    const prefix = text.trimStart()
    return COMMANDS.filter((c) => c.cmd.startsWith(prefix))
  }, [text])

  const acceptCommand = useCallback((cmd: string) => {
    setText(cmd + ' ')
    setCmdIndex(0)
    textareaRef.current?.focus()
  }, [])

  const handleSend = useCallback(() => {
    const trimmed = text.trim()
    if (!trimmed || streaming?.active) return

    const formatted = trimmed.replace(/\\n/g, '\n').replace(/\\t/g, '\t')

    if (formatted.startsWith('/')) {
      const [cmd] = formatted.split(/\s+/, 1)
      switch (cmd) {
        case '/clear':
          clearCurrent()
          break
        case '/bots':
          handleCommand('/bots')
          break
        default:
          sendMessage(formatted)
      }
      setText('')
      setCmdIndex(0)
      return
    }

    sendMessage(formatted)
    setText('')
    setCmdIndex(0)
  }, [text, streaming?.active, sendMessage, clearCurrent, handleCommand])

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      if (isCmdMode && matchingCmds.length > 0) {
        if (e.key === 'ArrowDown') {
          e.preventDefault()
          setCmdIndex((i) => (i + 1) % matchingCmds.length)
          return
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault()
          setCmdIndex((i) => (i - 1 + matchingCmds.length) % matchingCmds.length)
          return
        }
        if (e.key === 'Tab' || e.key === 'Enter') {
          e.preventDefault()
          acceptCommand(matchingCmds[cmdIndex]?.cmd || matchingCmds[0].cmd)
          return
        }
      }
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault()
        handleSend()
      }
    },
    [handleSend, isCmdMode, matchingCmds, cmdIndex, acceptCommand],
  )

  const handleInput = useCallback(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`
  }, [])

  const handleChange = useCallback(
    (val: string) => {
      setText(val)
      setCmdIndex(0) // 输入变化时重置选中索引
      handleInput()
    },
    [handleInput],
  )

  const insertCommand = useCallback((cmd: string) => {
    setText(cmd + ' ')
    setCmdIndex(0)
    textareaRef.current?.focus()
  }, [])

  const hasText = text.trim().length > 0

  return (
    <div className="shrink-0 px-4 pb-4 pt-3">
      <div className="mx-auto max-w-2xl">
        {/* Command chips */}
        <div className="mb-2 flex items-center gap-1">
          {COMMANDS.map(({ cmd, label, icon }) => (
            <button
              key={cmd}
              onClick={() => insertCommand(cmd)}
              className="inline-flex items-center gap-1 rounded-md border border-border/60 bg-muted/50 px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:border-ring/30 hover:bg-accent hover:text-foreground"
            >
              {icon}
              {label}
            </button>
          ))}
        </div>

        <div className="relative">
          {/* 命令提示弹窗 */}
          {isCmdMode && matchingCmds.length > 0 && (
            <div className="absolute bottom-full left-0 right-0 mb-1 rounded-lg border bg-popover shadow-lg overflow-hidden">
              {matchingCmds.map((cmd, i) => (
                <button
                  key={cmd.cmd}
                  onClick={() => acceptCommand(cmd.cmd)}
                  onMouseEnter={() => setCmdIndex(i)}
                  className={cn(
                    'flex w-full items-center gap-2.5 px-3 py-2 text-left transition-colors',
                    i === cmdIndex
                      ? 'bg-accent text-accent-foreground'
                      : 'hover:bg-muted/60 text-foreground/80',
                  )}
                >
                  <span className="shrink-0 text-muted-foreground">{cmd.icon}</span>
                  <div className="flex-1 min-w-0">
                    <div className="text-xs font-medium">{cmd.label}</div>
                    <div className="text-[10px] text-muted-foreground">{cmd.desc}</div>
                  </div>
                  <div className="flex items-center gap-1 shrink-0">
                    <span className="text-[10px] text-muted-foreground/60">{cmd.cmd}</span>
                    <CornerDownLeft className="h-3 w-3 text-muted-foreground/40" />
                  </div>
                </button>
              ))}
            </div>
          )}

          <div
            className={cn(
              'flex items-end rounded-2xl border bg-muted/60 px-2 py-2 shadow-sm transition-all duration-200',
              'focus-within:border-ring/30 focus-within:bg-background focus-within:shadow-md',
            )}
          >
            <textarea
              ref={textareaRef}
              value={text}
              onChange={(e) => handleChange(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="输入消息... / 使用命令"
              rows={1}
              className="flex-1 resize-none bg-transparent px-2 py-1 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none"
            />

            {streaming?.active ? (
              <button
                onClick={cancelStream}
                title="停止生成"
                className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-destructive text-destructive-foreground transition-colors hover:bg-destructive/90"
              >
                <Square className="h-3.5 w-3.5" />
              </button>
            ) : (
              <button
                onClick={handleSend}
                disabled={!hasText}
                title="发送消息"
                className={cn(
                  'flex h-8 w-8 shrink-0 items-center justify-center rounded-lg transition-all',
                  hasText
                    ? 'bg-foreground text-background hover:bg-foreground/80'
                    : 'cursor-default bg-muted-foreground/20 text-muted-foreground/40',
                )}
              >
                <ArrowUp className="h-4 w-4" />
              </button>
            )}
          </div>
        </div>

        <p className="mt-1.5 text-center text-[10px] text-muted-foreground/50">
          回车发送 &middot; Shift+回车换行
        </p>
      </div>
    </div>
  )
}
