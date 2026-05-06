import { useState } from 'react'
import { Plus, Trash2, PanelLeftClose, PanelLeftOpen, Folder } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { cn } from '@/lib/utils'
import { useChatStore } from '@/store/chat'
import { DirectoryPicker } from '@/components/DirectoryPicker'

export function SessionList() {
  const [collapsed, setCollapsed] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [showNewDialog, setShowNewDialog] = useState(false)
  const sessions = useChatStore((s) => s.sessions)
  const currentId = useChatStore((s) => s.currentSessionId)
  const createSession = useChatStore((s) => s.createSession)
  const selectSession = useChatStore((s) => s.selectSession)
  const deleteSession = useChatStore((s) => s.deleteSession)

  const openNewDialog = () => setShowNewDialog(true)

  return (
    <aside
      className={cn(
        'flex shrink-0 flex-col border-r bg-sidebar transition-all duration-300',
        collapsed ? 'w-12' : 'w-60',
      )}
    >
      {/* Header */}
      <div className="flex items-center border-b border-border/50 px-1.5 py-1.5">
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 shrink-0 text-muted-foreground hover:text-foreground"
          onClick={() => setCollapsed(!collapsed)}
          title={collapsed ? '展开侧栏' : '收起侧栏'}
        >
          {collapsed ? (
            <PanelLeftOpen className="h-4 w-4" />
          ) : (
            <PanelLeftClose className="h-4 w-4" />
          )}
        </Button>

        {!collapsed && (
          <>
            <span className="ml-2 flex-1 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
              会话
            </span>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 text-muted-foreground hover:text-foreground"
              onClick={openNewDialog}
              title="新建会话"
            >
              <Plus className="h-3.5 w-3.5" />
            </Button>
          </>
        )}
      </div>

      {/* Content */}
      {collapsed ? (
        <div className="flex flex-1 flex-col items-center gap-1.5 py-3">
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 text-muted-foreground hover:text-foreground"
            onClick={openNewDialog}
            title="新建会话"
          >
            <Plus className="h-4 w-4" />
          </Button>
          <div className="my-1 h-px w-5 bg-border/50" />
          {sessions.map((s) => (
            <Tooltip key={s.id}>
              <TooltipTrigger asChild>
                <button
                  onClick={() => selectSession(s.id)}
                  className={cn(
                    'relative flex h-8 w-8 items-center justify-center rounded-lg text-[10px] font-medium transition-all hover:bg-accent/50 hover:text-foreground',
                    s.id === currentId
                      ? 'text-foreground'
                      : 'text-muted-foreground/60',
                  )}
                >
                  {s.id === currentId && (
                    <span className="absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full bg-primary" />
                  )}
                  {firstChar(s.preview)}
                </button>
              </TooltipTrigger>
              <TooltipContent side="right" className="max-w-56">
                <p className="truncate text-xs !text-white">
                  {s.preview || '新建会话'}
                </p>
                <p className="text-[10px] !text-white/70">
                  {s.message_count} 条消息 &middot;{relativeTime(s.last_active)}
                </p>
                <p className="mt-0.5 truncate text-[10px] !text-white/50">
                  {shortPath(s.working_dir)}
                </p>
              </TooltipContent>
            </Tooltip>
          ))}
        </div>
      ) : (
        <div className="flex-1 overflow-y-auto overflow-x-hidden">
          {sessions.length === 0 ? (
            <div className="px-3 py-10 text-center">
              <p className="text-xs text-muted-foreground">暂无会话</p>
              <Button
                variant="outline"
                size="sm"
                className="mt-3 h-8 text-xs"
                onClick={openNewDialog}
              >
                <Plus className="mr-1 h-3 w-3" />
                新建会话
              </Button>
            </div>
          ) : (
            <div className="space-y-0.5 p-2">
              {sessions.map((s) => (
                <div
                  key={s.id}
                  className={cn(
                    'group relative flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left transition-all hover:bg-accent/60',
                    s.id === currentId
                      ? 'bg-accent/40 text-foreground'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                >
                  {s.id === currentId && (
                    <span className="absolute left-0 top-1/2 h-5 w-0.5 -translate-y-1/2 rounded-full bg-primary" />
                  )}
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        onClick={() => selectSession(s.id)}
                        className="flex min-w-0 flex-1 items-center gap-2.5 text-left cursor-pointer"
                      >
                        <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-background text-[10px] font-semibold text-muted-foreground">
                          {firstChar(s.preview)}
                        </div>
                        <div className="min-w-0 flex-1">
                          <p className="line-clamp-2 break-words text-[11px] font-medium leading-snug text-foreground">
                            {s.preview || '新建会话'}
                          </p>
                          <p className="mt-0.5 text-[10px] text-muted-foreground/80">
                            {s.message_count} 条消息 &middot;{relativeTime(s.last_active)}
                          </p>
                          <p className="mt-0.5 flex items-center gap-1 text-[10px] text-muted-foreground/50">
                            <Folder className="h-2.5 w-2.5 shrink-0" />
                            <span className="truncate">{shortPath(s.working_dir)}</span>
                          </p>
                        </div>
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="right" className="max-w-64">
                      <p className="break-all text-xs !text-white">{s.preview || '新建会话'}</p>
                      <p className="mt-0.5 truncate text-[10px] !text-white/60">
                        {s.working_dir}
                      </p>
                    </TooltipContent>
                  </Tooltip>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6 shrink-0 text-muted-foreground/40 hover:text-destructive"
                    onClick={(e) => {
                      e.stopPropagation()
                      setDeleteTarget(s.id)
                    }}
                    title="删除"
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
      {/* Delete confirm dialog */}
      <Dialog open={deleteTarget !== null} onOpenChange={() => setDeleteTarget(null)}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>删除会话</DialogTitle>
            <DialogDescription>
              删除后无法恢复，确定要继续吗？
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2">
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
              取消
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                if (deleteTarget) {
                  deleteSession(deleteTarget)
                  setDeleteTarget(null)
                }
              }}
            >
              删除
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* New session dialog */}
      <DirectoryPicker
        open={showNewDialog}
        onOpenChange={setShowNewDialog}
        onSelect={(path) => createSession(path)}
      />
    </aside>
  )
}

function firstChar(s: string | null): string {
  if (!s) return '?'
  const trimmed = s.trim()
  if (!trimmed) return '?'
  const c = trimmed[0]
  return /[a-zA-Z]/.test(c) ? c.toUpperCase() : c
}

function relativeTime(iso: string): string {
  const then = new Date(iso).getTime()
  const now = Date.now()
  const diff = now - then
  const mins = Math.floor(diff / 60_000)
  if (mins < 1) return '刚刚'
  if (mins < 60) return `${mins}m`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days}d`
  return new Date(iso).toLocaleDateString()
}

function shortPath(path: string): string {
  if (!path || path === '.') return '.'
  const parts = path.replace(/\\/g, '/').split('/')
  if (parts.length <= 2) return path
  return '.../' + parts.slice(-2).join('/')
}
