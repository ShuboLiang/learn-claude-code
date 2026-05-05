import { useCallback, useMemo, useState, useRef, useEffect } from 'react'
import { Tree, type NodeApi, type NodeRendererProps } from 'react-arborist'
import { ChevronRight, Folder, FolderOpen, File, Loader2 } from 'lucide-react'
import { useWorkspaceStore } from '@/store/workspace'
import type { BrowseEntry } from '@/api/client'
import { cn } from '@/lib/utils'

interface TreeNode {
  id: string
  name: string
  kind: 'file' | 'directory'
  size?: number
  children?: TreeNode[] | null // null = not loaded
}

function formatSize(bytes?: number): string {
  if (bytes === undefined || bytes === null) return ''
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function entryToNode(e: BrowseEntry): TreeNode {
  return {
    id: e.path,
    name: e.name,
    kind: e.kind,
    size: e.size,
    children: e.kind === 'directory' ? null : undefined,
  }
}

function NodeRenderer({ node, style, tree }: NodeRendererProps<TreeNode>) {
  const { kind, size } = node.data
  const loadingNodes = useWorkspaceStore((s) => s.loading)
  const treeNodes = useWorkspaceStore((s) => s.treeNodes)
  const loadChildren = useWorkspaceStore((s) => s.loadChildren)
  const isDir = kind === 'directory'
  const isLoading = loadingNodes[node.id]

  const handleClick = useCallback(async () => {
    if (!isDir) return
    if (node.isOpen) {
      tree.close(node.id)
    } else {
      // Load children if not yet loaded, then open
      if (!treeNodes[node.id]) {
        await loadChildren(node.id)
      }
      tree.open(node.id)
    }
  }, [isDir, node.id, node.isOpen, treeNodes, loadChildren, tree])

  return (
    <button
      onClick={handleClick}
      style={style}
      className={cn(
        'flex w-full items-center gap-1 py-0.5 text-xs transition-colors hover:bg-accent/60 rounded-sm text-left',
        node.isSelected && 'bg-accent/40 text-foreground',
        !isDir && 'cursor-default',
      )}
    >
      {/* Expand chevron */}
      {isDir ? (
        isLoading ? (
          <Loader2 className="h-3 w-3 shrink-0 animate-spin text-muted-foreground" />
        ) : (
          <ChevronRight
            className={cn(
              'h-3 w-3 shrink-0 text-muted-foreground transition-transform',
              node.isOpen && 'rotate-90',
            )}
          />
        )
      ) : (
        <span className="w-3 shrink-0" />
      )}

      {/* Icon */}
      {isDir ? (
        node.isOpen ? (
          <FolderOpen className="h-3.5 w-3.5 shrink-0 text-yellow-500/80" />
        ) : (
          <Folder className="h-3.5 w-3.5 shrink-0 text-yellow-500/80" />
        )
      ) : (
        <File className="h-3.5 w-3.5 shrink-0 text-muted-foreground/60" />
      )}

      {/* Name */}
      <span className="truncate">{node.data.name}</span>

      {/* Size */}
      {!isDir && size !== undefined && (
        <span className="ml-auto shrink-0 text-[10px] text-muted-foreground/50">
          {formatSize(size)}
        </span>
      )}
    </button>
  )
}

export function FileTree() {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const treeNodes = useWorkspaceStore((s) => s.treeNodes)
  const loading = useWorkspaceStore((s) => s.loading)
  const openFile = useWorkspaceStore((s) => s.openFile)
  const sessionId = useWorkspaceStore((s) => s.sessionId)
  const containerRef = useRef<HTMLDivElement>(null)
  const [height, setHeight] = useState(300)

  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const obs = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setHeight(entry.contentRect.height)
      }
    })
    obs.observe(el)
    return () => obs.disconnect()
  }, [])

  const data = useMemo<TreeNode[]>(() => {
    if (!rootPath) return []
    const entries = treeNodes[rootPath]
    if (!entries) return []
    return entries.map(entryToNode)
  }, [rootPath, treeNodes])

  const childrenAccessor = useCallback(
    (node: TreeNode): TreeNode[] | null => {
      if (node.kind !== 'directory') return null
      // Load children from store (null in store = not loaded yet)
      const entries = treeNodes[node.id]
      if (!entries) return null
      return entries.map(entryToNode)
    },
    [treeNodes],
  )

  const handleSelect = useCallback(
    (nodes: NodeApi<TreeNode>[]) => {
      const node = nodes[0]
      if (!node || node.data.kind !== 'file') return
      if (!sessionId) return
      openFile(node.id, sessionId)
    },
    [sessionId, openFile],
  )

  if (!rootPath) {
    return (
      <div className="flex flex-1 items-center justify-center p-4">
        <p className="text-xs text-muted-foreground">选择会话以浏览文件</p>
      </div>
    )
  }

  if (!data.length && loading[rootPath]) {
    return (
      <div className="flex items-center justify-center py-6">
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      </div>
    )
  }

  return (
    <div ref={containerRef} className="h-full overflow-hidden">
      <div className="mb-1 px-2 py-1.5">
        <p className="truncate text-[10px] font-medium text-muted-foreground/70">
          {shortPath(rootPath)}
        </p>
      </div>
      <div className="h-[calc(100%-28px)]">
        <Tree<TreeNode>
          data={data}
          childrenAccessor={childrenAccessor}
          onSelect={handleSelect}
          rowHeight={26}
          width="100%"
          height={Math.max(height - 28, 100)}
          indent={16}
          disableDrag
          disableDrop
        >
          {(props) => <NodeRenderer {...props} />}
        </Tree>
      </div>
    </div>
  )
}

function shortPath(path: string): string {
  if (!path || path === '.') return '.'
  const parts = path.replace(/\\/g, '/').split('/')
  if (parts.length <= 2) return path
  return '.../' + parts.slice(-2).join('/')
}
