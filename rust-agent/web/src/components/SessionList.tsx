import { Plus, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useChatStore } from '@/store/chat'

export function SessionList() {
  const sessions = useChatStore((s) => s.sessions)
  const currentId = useChatStore((s) => s.currentSessionId)
  const createSession = useChatStore((s) => s.createSession)
  const selectSession = useChatStore((s) => s.selectSession)
  const deleteSession = useChatStore((s) => s.deleteSession)

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r">
      <div className="flex items-center justify-between border-b px-3 py-2">
        <span className="text-xs font-medium text-muted-foreground">
          Sessions
        </span>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={() => createSession()}
          title="New session"
        >
          <Plus className="h-4 w-4" />
        </Button>
      </div>

      <ScrollArea className="flex-1">
        {sessions.length === 0 ? (
          <p className="px-3 py-6 text-center text-xs text-muted-foreground">
            No sessions yet
          </p>
        ) : (
          sessions.map((s) => (
            <div
              key={s.id}
              onClick={() => selectSession(s.id)}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => { if (e.key === 'Enter') selectSession(s.id) }}
              className={
                'group flex w-full items-center justify-between px-3 py-2 text-left text-sm transition-colors hover:bg-accent ' +
                (s.id === currentId ? 'bg-accent' : '')
              }
            >
              <div className="min-w-0 flex-1">
                <p className="truncate text-xs">
                  {s.preview || 'New session'}
                </p>
                <p className="text-[10px] text-muted-foreground">
                  {s.message_count} msgs &middot;{' '}
                  {relativeTime(s.last_active)}
                </p>
              </div>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100"
                onClick={(e) => {
                  e.stopPropagation()
                  deleteSession(s.id)
                }}
                title="Delete session"
              >
                <Trash2 className="h-3 w-3" />
              </Button>
            </div>
          ))
        )}
      </ScrollArea>
    </aside>
  )
}

function relativeTime(iso: string): string {
  const then = new Date(iso).getTime()
  const now = Date.now()
  const diff = now - then
  const mins = Math.floor(diff / 60_000)
  if (mins < 1) return 'just now'
  if (mins < 60) return `${mins}m ago`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days}d ago`
  return new Date(iso).toLocaleDateString()
}
