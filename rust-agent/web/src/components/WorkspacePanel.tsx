import { useEffect, useState, useCallback, useRef } from 'react'
import { PanelRightClose, PanelRightOpen } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useChatStore } from '@/store/chat'
import { useWorkspaceStore } from '@/store/workspace'
import { FileTree } from '@/components/FileTree'
import { FilePreview } from '@/components/FilePreview'

export function WorkspacePanel() {
  const [collapsed, setCollapsed] = useState(true)
  const [splitRatio, setSplitRatio] = useState(0.45)
  const resizeRef = useRef<HTMLDivElement>(null)
  const prevRootPath = useRef<string | null>(null)
  const currentSessionId = useChatStore((s) => s.currentSessionId)
  const sessions = useChatStore((s) => s.sessions)
  const setRootPath = useWorkspaceStore((s) => s.setRootPath)
  const startWatch = useWorkspaceStore((s) => s.startWatch)
  const stopWatch = useWorkspaceStore((s) => s.stopWatch)
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const selectedFile = useWorkspaceStore((s) => s.selectedFile)

  // Sync rootPath when session changes
  useEffect(() => {
    if (!currentSessionId) {
      setRootPath(null, null)
      return
    }
    const session = sessions.find((s) => s.id === currentSessionId)
    if (session?.working_dir) {
      setRootPath(session.working_dir, currentSessionId)
    } else {
      setRootPath(null, null)
    }
  }, [currentSessionId, sessions, setRootPath])

  // Auto-expand when rootPath first becomes available
  useEffect(() => {
    if (rootPath && rootPath !== prevRootPath.current) {
      setCollapsed(false)
    }
    prevRootPath.current = rootPath
  }, [rootPath])

  // Start/stop watch based on collapsed and session
  useEffect(() => {
    if (!collapsed && currentSessionId && rootPath) {
      startWatch(currentSessionId)
    } else {
      stopWatch()
    }
  }, [collapsed, currentSessionId, rootPath, startWatch, stopWatch])

  // Resize divider drag
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    const startY = e.clientY
    const startRatio = splitRatio

    const onMouseMove = (ev: MouseEvent) => {
      const panel = resizeRef.current
      if (!panel) return
      const totalHeight = panel.getBoundingClientRect().height
      if (totalHeight === 0) return
      const delta = ev.clientY - startY
      const newRatio = startRatio + delta / totalHeight
      setSplitRatio(Math.max(0.2, Math.min(0.7, newRatio)))
    }

    const onMouseUp = () => {
      document.removeEventListener('mousemove', onMouseMove)
      document.removeEventListener('mouseup', onMouseUp)
    }

    document.addEventListener('mousemove', onMouseMove)
    document.addEventListener('mouseup', onMouseUp)
  }, [splitRatio])

  return (
    <aside
      className={cn(
        'flex shrink-0 flex-col border-l bg-sidebar transition-all duration-300',
        collapsed ? 'w-10' : 'w-64',
      )}
    >
      {/* Header */}
      <div className="flex items-center border-b border-border/50 px-1.5 py-1.5">
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 shrink-0 text-muted-foreground hover:text-foreground"
          onClick={() => setCollapsed(!collapsed)}
          title={collapsed ? '展开工作区' : '收起工作区'}
        >
          {collapsed ? (
            <PanelRightOpen className="h-4 w-4" />
          ) : (
            <PanelRightClose className="h-4 w-4" />
          )}
        </Button>

        {!collapsed && (
          <span className="ml-2 flex-1 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
            文件
          </span>
        )}
      </div>

      {/* Content */}
      {collapsed ? null : (
        <div ref={resizeRef} className="flex flex-col flex-1 min-h-0">
          {/* File tree area */}
          <div
            style={{
              height: selectedFile ? `${splitRatio * 100}%` : '100%',
              minHeight: 0,
            }}
            className="overflow-hidden"
          >
            {!currentSessionId ? (
              <div className="flex flex-1 items-center justify-center p-4">
                <p className="text-xs text-muted-foreground">
                  选择会话以浏览文件
                </p>
              </div>
            ) : (
              <FileTree />
            )}
          </div>

          {/* Resize handle */}
          {selectedFile && (
            <>
              <div
                className="h-1 shrink-0 cursor-row-resize hover:bg-primary/30 transition-colors group flex items-center justify-center"
                onMouseDown={handleMouseDown}
              >
                <div className="h-px w-8 bg-border group-hover:bg-primary/50 transition-colors" />
              </div>

              {/* File preview area */}
              <div className="flex-1 min-h-0 overflow-hidden">
                <FilePreview />
              </div>
            </>
          )}
        </div>
      )}
    </aside>
  )
}
