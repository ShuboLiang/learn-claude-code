import { ArrowDown } from 'lucide-react'
import { MessageBubble } from '@/components/MessageBubble'
import { Composer } from '@/components/Composer'
import { useChatStore } from '@/store/chat'
import { RetryBanner } from '@/components/RetryBanner'
import { useAutoScroll } from '@/hooks/useAutoScroll'
import { cn } from '@/lib/utils'

export function ChatPane() {
  const messages = useChatStore((s) => s.messages)
  const streaming = useChatStore((s) => s.streaming)

  const scrollDeps = [
    messages,
    streaming?.assistantText,
    streaming?.tools,
    streaming?.thinking,
  ]

  const [scrollRef, isAtBottom] = useAutoScroll(scrollDeps)

  return (
    <div className="flex flex-1 flex-col min-w-0 bg-background">
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
                  blocks: [
                    ...(streaming.thinking
                      ? [{ kind: 'thinking' as const, content: streaming.thinking }]
                      : []),
                    ...(streaming.assistantText
                      ? [{ kind: 'text' as const, content: streaming.assistantText }]
                      : []),
                    ...streaming.tools.map((tc) => ({
                      kind: 'toolCall' as const,
                      toolCall: tc,
                    })),
                    ...(streaming.error
                      ? [{ kind: 'error' as const, ...streaming.error }]
                      : []),
                  ],
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
          aria-label="Scroll to bottom"
        >
          <ArrowDown className="h-4 w-4" />
        </button>
      </div>

      <Composer />
    </div>
  )
}
