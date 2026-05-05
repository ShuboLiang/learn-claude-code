import { create } from 'zustand'
import { immer } from 'zustand/middleware/immer'
import type { BrowseEntry } from '@/api/client'
import { browseDirectory, streamWatchEvents, readFile, writeFile } from '@/api/workspace'

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
          const { rootPath, expandedDirs } = get()
          if (!rootPath) return
          const path = evt.data.path
          const parentDir = path.substring(0, path.lastIndexOf('\\'))
          if (parentDir && expandedDirs[parentDir]) {
            get().loadChildren(parentDir)
          }
          if (expandedDirs[rootPath]) {
            get().loadChildren(rootPath)
          }
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
        s.fileDirty = false
        s.fileLoading = true
        s.sessionId = sid
      })
      try {
        const content = await readFile(sid, filePath)
        set((s) => {
          s.fileContent = content
          s.fileLoading = false
        })
      } catch (err) {
        set((s) => {
          s.fileContent = `// Error: ${err instanceof Error ? err.message : 'Failed to read'}`
          s.fileLoading = false
        })
      }
    },

    closeFile() {
      set((s) => {
        s.selectedFile = null
        s.fileContent = null
        s.fileDirty = false
        s.fileLoading = false
      })
    },

    updateFileContent(content) {
      if (content === undefined) return
      set((s) => {
        s.fileContent = content
        s.fileDirty = true
      })
    },

    async saveFile() {
      const { selectedFile, fileContent, sessionId } = get()
      if (!selectedFile || !sessionId || fileContent === null) return

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
