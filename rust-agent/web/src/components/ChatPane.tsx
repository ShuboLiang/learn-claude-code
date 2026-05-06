import { ArrowDown, ChevronDown } from 'lucide-react'
import { useMemo } from 'react'
import { MessageBubble } from '@/components/MessageBubble'
import { Composer } from '@/components/Composer'
import { useChatStore, buildStreamingBlocks } from '@/store/chat'
import { RetryBanner } from '@/components/RetryBanner'
import { useAutoScroll } from '@/hooks/useAutoScroll'
import { cn } from '@/lib/utils'

export function ChatPane() {
  const messages = useChatStore((s) => s.messages)
  const streaming = useChatStore((s) => s.streaming)
  const profiles = useChatStore((s) => s.profiles)
  const selectedProfile = useChatStore((s) => s.selectedProfile)
  const selectedModel = useChatStore((s) => s.selectedModel)
  const setSelectedProfile = useChatStore((s) => s.setSelectedProfile)
  const setSelectedModel = useChatStore((s) => s.setSelectedModel)

  const currentModels =
    profiles.find((p) => p.name === selectedProfile)?.models || []

  const streamingBlocks = useMemo(() => {
    if (!streaming) return []
    return buildStreamingBlocks(streaming)
  }, [streaming])

  const scrollDeps = [messages, streamingBlocks]

  const [scrollRef, isAtBottom] = useAutoScroll(scrollDeps)

  return (
    <div className="flex flex-1 flex-col min-w-0 bg-background">
      {/* 会话级配置栏 */}
      <div className="flex items-center gap-2 px-5 py-1.5 border-b bg-muted/30">
        <span className="text-[10px] text-muted-foreground mr-1">配置</span>

        <div className="relative">
          <select
            value={selectedProfile}
            onChange={(e) => setSelectedProfile(e.target.value)}
            className="h-6 rounded border bg-background px-1.5 pr-5 text-[11px] appearance-none cursor-pointer hover:border-primary/40 focus:outline-none focus:ring-1 focus:ring-primary"
          >
            {profiles.map((p) => (
              <option key={p.name} value={p.name}>{p.name}</option>
            ))}
          </select>
          <ChevronDown className="absolute right-1 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground pointer-events-none" />
        </div>

        <div className="relative">
          <select
            value={selectedModel}
            onChange={(e) => setSelectedModel(e.target.value)}
            className="h-6 rounded border bg-background px-1.5 pr-5 text-[11px] appearance-none cursor-pointer hover:border-primary/40 focus:outline-none focus:ring-1 focus:ring-primary"
          >
            {currentModels.map((m) => (
              <option key={m} value={m}>{m}</option>
            ))}
          </select>
          <ChevronDown className="absolute right-1 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground pointer-events-none" />
        </div>
      </div>

      {/* Messages area */}
      <div className="relative flex-1 overflow-hidden">
        <div ref={scrollRef} className="h-full overflow-y-auto">
          <div className="mx-auto max-w-3xl px-6 py-6 space-y-5">
            {messages.map((msg) => (
              <MessageBubble key={msg.id} message={msg} />
            ))}

            {streaming?.retrying && (
              <RetryBanner
                attempt={streaming.retrying.attempt}
                maxRetries={streaming.retrying.maxRetries}
                waitSeconds={streaming.retrying.waitSeconds}
                detail={streaming.retrying.detail}
              />
            )}

            {streaming?.active && (
              <MessageBubble
                message={{
                  id: '__streaming__',
                  role: 'assistant',
                  content: '',
                  blocks: streamingBlocks,
                }}
              />
            )}

            {/* Bottom spacer for scroll comfort */}
            <div className="h-4" />
          </div>
        </div>

        {/* Jump-to-bottom button */}
        <button
          onClick={() => {
            const el = scrollRef.current
            if (el) el.scrollTop = el.scrollHeight
          }}
          className={cn(
            'absolute bottom-4 right-4 flex h-8 w-8 items-center justify-center rounded-full border bg-background shadow-md transition-all hover:shadow-lg',
            isAtBottom ? 'pointer-events-none translate-y-2 opacity-0' : 'opacity-100',
          )}
          aria-label="滚动到底部"
        >
          <ArrowDown className="h-4 w-4" />
        </button>
      </div>

      <Composer />
    </div>
  )
}
