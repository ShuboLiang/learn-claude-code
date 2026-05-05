import { ArrowDown } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { MessageBubble } from '@/components/MessageBubble'
import { Composer } from '@/components/Composer'
import { RetryBanner } from '@/components/RetryBanner'
import { useChatStore } from '@/store/chat'
import { useAutoScroll } from '@/hooks/useAutoScroll'

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
    <div className="flex flex-1 flex-col min-w-0">
      {/* Messages area */}
      <div className="relative flex-1 overflow-hidden">
        <div ref={scrollRef} className="h-full overflow-y-auto">
          <div className="mx-auto max-w-2xl px-4 py-4 space-y-4">
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
                      ? [
                          {
                            kind: 'thinking' as const,
                            content: streaming.thinking,
                          },
                        ]
                      : []),
                    ...(streaming.assistantText
                      ? [
                          {
                            kind: 'text' as const,
                            content: streaming.assistantText,
                          },
                        ]
                      : []),
                    ...streaming.tools.map((tc) => ({
                      kind: 'toolCall' as const,
                      toolCall: tc,
                    })),
                    ...(streaming.error
                      ? [
                          {
                            kind: 'error' as const,
                            ...streaming.error,
                          },
                        ]
                      : []),
                  ],
                }}
              />
            )}
          </div>
        </div>

        {!isAtBottom && (
          <Button
            variant="secondary"
            size="icon"
            className="absolute bottom-3 right-4 h-8 w-8 rounded-full shadow"
            onClick={() => {
              const el = scrollRef.current
              if (el) el.scrollTop = el.scrollHeight
            }}
          >
            <ArrowDown className="h-4 w-4" />
          </Button>
        )}
      </div>

      <Composer />
    </div>
  )
}
