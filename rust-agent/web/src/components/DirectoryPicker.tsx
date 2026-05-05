import { useState, useEffect, useMemo } from 'react'
import { Folder, ChevronUp, HardDrive, Clock } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { cn } from '@/lib/utils'
import type { BrowseEntry } from '@/api/client'
import { browseDirectory } from '@/api/client'

const RECENT_KEY = 'rust-agent-recent-dirs'
const MAX_RECENT = 5

function loadRecent(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY)
    return raw ? (JSON.parse(raw) as string[]) : []
  } catch {
    return []
  }
}

function saveRecent(path: string) {
  const recent = loadRecent().filter((p) => p !== path)
  recent.unshift(path)
  localStorage.setItem(RECENT_KEY, JSON.stringify(recent.slice(0, MAX_RECENT)))
}

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSelect: (path: string) => void
}

export function DirectoryPicker({ open, onOpenChange, onSelect }: Props) {
  const [currentPath, setCurrentPath] = useState('')
  const [parent, setParent] = useState<string | null>(null)
  const [entries, setEntries] = useState<BrowseEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const recentDirs = useMemo(() => loadRecent(), [open])

  const loadDir = async (dirPath?: string) => {
    setLoading(true)
    setError(null)
    try {
      const result = await browseDirectory(dirPath)
      setCurrentPath(result.path)
      setParent(result.parent)
      setEntries(result.entries)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to browse')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (open) loadDir()
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  const navigateTo = (path: string) => loadDir(path)

  const handleSelect = () => {
    saveRecent(currentPath)
    onSelect(currentPath)
    onOpenChange(false)
  }

  const isDriveRoot = !parent && entries.length > 0 && entries[0]?.path?.match(/^[A-Z]:\\$/)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>选择工作目录</DialogTitle>
          <DialogDescription>
            选择会话的工作目录，shell 命令将在此目录执行
          </DialogDescription>
        </DialogHeader>

        {/* Path breadcrumb */}
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground bg-muted/50 rounded-lg px-3 py-2 min-h-8">
          <HardDrive className="h-3 w-3 shrink-0" />
          <span className="truncate text-foreground font-medium">
            {currentPath || '我的电脑'}
          </span>
        </div>

        {/* Recent directories */}
        {recentDirs.length > 0 && (
          <div className="space-y-1">
            <p className="text-[10px] font-medium text-muted-foreground/70 uppercase tracking-wide">最近使用</p>
            <div className="flex flex-col gap-0.5">
              {recentDirs.map((dir) => (
                <button
                  key={dir}
                  onClick={() => {
                    navigateTo(dir)
                    setCurrentPath(dir)
                  }}
                  className={cn(
                    'flex items-center gap-2 rounded-md px-2.5 py-1.5 text-xs transition-colors hover:bg-accent',
                    dir === currentPath
                      ? 'bg-accent/60 text-foreground font-medium'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                >
                  <Clock className="h-3 w-3 shrink-0" />
                  <span className="truncate">{shortDir(dir)}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Directory listing */}
        <div className="max-h-56 overflow-y-auto rounded-lg border">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <span className="text-xs text-muted-foreground">加载中...</span>
            </div>
          ) : error ? (
            <div className="flex flex-col items-center gap-2 py-8">
              <p className="text-xs text-destructive">{error}</p>
              <Button
                variant="outline"
                size="sm"
                className="h-7 text-xs"
                onClick={() => loadDir(currentPath)}
              >
                重试
              </Button>
            </div>
          ) : (
            <div className="py-1">
              {/* Parent directory */}
              {parent && (
                <button
                  onClick={() => navigateTo(parent)}
                  className="flex w-full items-center gap-2.5 px-3 py-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
                >
                  <ChevronUp className="h-3.5 w-3.5" />
                  <span>..</span>
                </button>
              )}

              {/* Drive list */}
              {isDriveRoot && (
                <div>
                  {entries.map((e) => (
                    <button
                      key={e.path}
                      onClick={() => navigateTo(e.path)}
                      className="flex w-full items-center gap-2.5 px-3 py-2 text-xs text-foreground hover:bg-accent transition-colors"
                    >
                      <HardDrive className="h-3.5 w-3.5 text-primary/60" />
                      <span className="font-medium">{e.name}</span>
                    </button>
                  ))}
                </div>
              )}

              {/* Subdirectories */}
              {!isDriveRoot && entries.length === 0 && (
                <p className="px-3 py-8 text-center text-xs text-muted-foreground">
                  没有子目录
                </p>
              )}
              {!isDriveRoot &&
                entries
                  .filter((e) => e.kind === 'directory')
                  .map((e) => (
                    <button
                      key={e.path}
                      onClick={() => navigateTo(e.path)}
                      className="flex w-full items-center gap-2.5 px-3 py-2 text-xs text-foreground hover:bg-accent transition-colors"
                    >
                      <Folder className="h-3.5 w-3.5 shrink-0 text-yellow-500/80" />
                      <span className="truncate">{e.name}</span>
                    </button>
                  ))}
            </div>
          )}
        </div>

        <DialogFooter className="gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button
            onClick={handleSelect}
            disabled={loading || !currentPath}
          >
            选择此目录
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function shortDir(path: string): string {
  const parts = path.replace(/\\/g, '/').split('/').filter(Boolean)
  if (parts.length <= 2) return path
  return parts.slice(0, 2).join('/') + '/.../' + parts.slice(-2).join('/')
}
