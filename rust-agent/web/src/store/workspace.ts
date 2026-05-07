import { create } from 'zustand'
import { immer } from 'zustand/middleware/immer'
import type { BrowseEntry } from '@/api/client'
import { browseDirectory, streamWatchEvents, readFile, writeFile } from '@/api/workspace'
import type { FileReadResult } from '@/api/workspace'

interface WorkspaceState {
  rootPath: string | null
  treeNodes: Record<string, BrowseEntry[]>
  expandedDirs: Record<string, boolean>
  loading: Record<string, boolean>
  watchActive: boolean
  error: string | null
  // File preview
  selectedFile: string | null
  fileContent: string | null
  fileBinary: boolean
  fileDirty: boolean
  fileLoading: boolean
  sessionId: string | null
}

interface WorkspaceActions {
  setRootPath: (path: string | null, sid?: string | null) => void
  loadChildren: (dirPath: string) => Promise<void>
  toggleExpand: (dirPath: string) => void
  startWatch: (sessionId: string) => void
  stopWatch: () => void
  refreshExpanded: () => void
  openFile: (filePath: string, sid: string) => Promise<void>
  closeFile: () => void
  updateFileContent: (content: string | undefined) => void
  saveFile: () => Promise<void>
}

let watchAbort: AbortController | null = null

export const useWorkspaceStore = create<WorkspaceState & WorkspaceActions>()(
  immer((set, get) => ({
    rootPath: null,
    treeNodes: {},
    expandedDirs: {},
    loading: {},
    watchActive: false,
    error: null,
    selectedFile: null,
    fileContent: null,
    fileBinary: false,
    fileDirty: false,
    fileLoading: false,
    sessionId: null,

    setRootPath(path, sid) {
      get().stopWatch()
      set((s) => {
        s.rootPath = path
        s.sessionId = sid ?? null
        s.treeNodes = {}
        s.expandedDirs = {}
        s.loading = {}
        s.watchActive = false
        s.error = null
        s.selectedFile = null
        s.fileContent = null
        s.fileBinary = false
        s.fileDirty = false
      })
      if (path) {
        get().loadChildren(path)
      }
    },

    async loadChildren(dirPath) {
      const state = get()
      if (state.loading[dirPath]) return

      set((s) => {
        s.loading[dirPath] = true
      })

      try {
        const result = await browseDirectory(dirPath)
        set((s) => {
          s.treeNodes[dirPath] = result.entries
          s.loading[dirPath] = false
          s.error = null
        })
      } catch (err) {
        set((s) => {
          s.loading[dirPath] = false
          s.error = err instanceof Error ? err.message : 'Failed to load'
        })
      }
    },

    toggleExpand(dirPath) {
      set((s) => {
        const wasExpanded = s.expandedDirs[dirPath]
        if (wasExpanded) {
          delete s.expandedDirs[dirPath]
        } else {
          s.expandedDirs[dirPath] = true
        }
      })
      if (get().expandedDirs[dirPath] && !get().treeNodes[dirPath]) {
        get().loadChildren(dirPath)
      }
    },

    startWatch(sessionId) {
      get().stopWatch()
      watchAbort = new AbortController()

      set((s) => {
        s.watchActive = true
      })

      streamWatchEvents(
        sessionId,
        (evt) => {
          const state = get()
          const rootPath = state.rootPath
          if (!rootPath) return
          const path = evt.data.path
          const lastSep = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'))
          const parentDir = lastSep > 0 ? path.substring(0, lastSep) : ''
          // 刷新父目录（如果已加载过）
          if (parentDir && state.treeNodes[parentDir] !== undefined) {
            get().loadChildren(parentDir)
          }
          // 始终刷新根目录
          get().loadChildren(rootPath)
        },
        watchAbort.signal,
      ).catch(() => {
        // Stream ended or aborted
      }).finally(() => {
        set((s) => {
          s.watchActive = false
        })
      })
    },

    stopWatch() {
      if (watchAbort) {
        watchAbort.abort()
        watchAbort = null
      }
      set((s) => {
        s.watchActive = false
      })
    },

    refreshExpanded() {
      const { expandedDirs } = get()
      for (const dir of Object.keys(expandedDirs)) {
        get().loadChildren(dir)
      }
    },

    async openFile(filePath, sid) {
      set((s) => {
        s.selectedFile = filePath
        s.fileContent = null
        s.fileBinary = false
        s.fileDirty = false
        s.fileLoading = true
        s.sessionId = sid
      })
      try {
        const result = await readFile(sid, filePath)
        set((s) => {
          s.fileContent = result.content
          s.fileBinary = result.binary
          s.fileLoading = false
        })
      } catch (err) {
        set((s) => {
          s.fileContent = `// Error: ${err instanceof Error ? err.message : 'Failed to read'}`
          s.fileBinary = false
          s.fileLoading = false
        })
      }
    },

    closeFile() {
      set((s) => {
        s.selectedFile = null
        s.fileContent = null
        s.fileBinary = false
        s.fileDirty = false
        s.fileLoading = false
      })
    },

    updateFileContent(content) {
      if (content === undefined) return
      if (get().fileBinary) return // Don't allow editing binary files
      set((s) => {
        s.fileContent = content
        s.fileDirty = true
      })
    },

    async saveFile() {
      const { selectedFile, fileContent, sessionId, fileBinary } = get()
      if (!selectedFile || !sessionId || fileContent === null || fileBinary) return

      try {
        await writeFile(sessionId, selectedFile, fileContent)
        set((s) => {
          s.fileDirty = false
          s.error = null
        })
      } catch (err) {
        set((s) => {
          s.error = err instanceof Error ? err.message : 'Save failed'
        })
      }
    },
  })),
)
