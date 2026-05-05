import { useState, useRef, useCallback, type KeyboardEvent } from 'react'
import { Square, ArrowUp, Trash2, Bot } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useChatStore } from '@/store/chat'

const COMMANDS: { cmd: string; label: string; icon: React.ReactNode }[] = [
  { cmd: '/clear', label: 'Clear', icon: <Trash2 className="h-3 w-3" /> },
  { cmd: '/bots', label: 'Bots', icon: <Bot className="h-3 w-3" /> },
]

export function Composer() {
  const [text, setText] = useState('')
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const sendMessage = useChatStore((s) => s.sendMessage)
  const cancelStream = useChatStore((s) => s.cancelStream)
  const streaming = useChatStore((s) => s.streaming)
  const clearCurrent = useChatStore((s) => s.clearCurrent)
  const handleCommand = useChatStore((s) => s.handleCommand)

  const handleSend = useCallback(() => {
    const trimmed = text.trim()
    if (!trimmed || streaming?.active) return

    // Slash command handling
    if (trimmed.startsWith('/')) {
      const [cmd] = trimmed.split(/\s+/, 1)
      switch (cmd) {
        case '/clear':
          clearCurrent()
          break
        case '/bots':
          handleCommand('/bots')
          break
        default:
          sendMessage(trimmed)
      }
      setText('')
      return
    }

    sendMessage(trimmed)
    setText('')
  }, [text, streaming?.active, sendMessage, clearCurrent])

  const handleKeyDown = useCallback(
    (e: KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault()
        handleSend()
      }
    },
    [handleSend],
  )

  const handleInput = useCallback(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`
  }, [])

  const insertCommand = useCallback((cmd: string) => {
    setText(cmd + ' ')
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

        <div
          className={cn(
            'flex items-end rounded-2xl border bg-muted/60 px-2 py-2 shadow-sm transition-all duration-200',
            'focus-within:border-ring/30 focus-within:bg-background focus-within:shadow-md',
          )}
        >
          <textarea
            ref={textareaRef}
            value={text}
            onChange={(e) => {
              setText(e.target.value)
              handleInput()
            }}
            onKeyDown={handleKeyDown}
            placeholder="Send a message...  / for commands"
            rows={1}
            className="flex-1 resize-none bg-transparent px-2 py-1 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none"
          />

          {streaming?.active ? (
            <button
              onClick={cancelStream}
              title="Stop generating"
              className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-destructive text-destructive-foreground transition-colors hover:bg-destructive/90"
            >
              <Square className="h-3.5 w-3.5" />
            </button>
          ) : (
            <button
              onClick={handleSend}
              disabled={!hasText}
              title="Send message"
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

        <p className="mt-1.5 text-center text-[10px] text-muted-foreground/50">
          Enter to send &middot; Shift+Enter for new line
        </p>
      </div>
    </div>
  )
}
