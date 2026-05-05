import { useCallback, useRef, useEffect } from 'react'
import Editor, { type OnMount } from '@monaco-editor/react'
import { X, Save, Loader2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useWorkspaceStore } from '@/store/workspace'

export function FilePreview() {
  const selectedFile = useWorkspaceStore((s) => s.selectedFile)
  const fileContent = useWorkspaceStore((s) => s.fileContent)
  const fileDirty = useWorkspaceStore((s) => s.fileDirty)
  const fileLoading = useWorkspaceStore((s) => s.fileLoading)
  const closeFile = useWorkspaceStore((s) => s.closeFile)
  const updateFileContent = useWorkspaceStore((s) => s.updateFileContent)
  const saveFile = useWorkspaceStore((s) => s.saveFile)
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null)

  // Ctrl+S to save
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault()
        saveFile()
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [saveFile])

  const handleEditorMount: OnMount = useCallback((editor) => {
    editorRef.current = editor
  }, [])

  if (!selectedFile) {
    return (
      <div className="flex flex-1 items-center justify-center p-4">
        <p className="text-[10px] text-muted-foreground/60">
          点击文件以预览
        </p>
      </div>
    )
  }

  const fileName = selectedFile.replace(/\\/g, '/').split('/').pop() || selectedFile

  return (
    <div className="flex flex-col h-full min-h-0">
      {/* Preview header */}
      <div className="flex items-center gap-1 border-b border-border/50 px-1.5 py-1">
        <span className="flex-1 truncate text-[10px] font-medium text-muted-foreground">
          {fileName}
          {fileDirty && <span className="ml-1 text-primary">●</span>}
        </span>
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6"
          onClick={saveFile}
          disabled={!fileDirty}
          title="保存 (Ctrl+S)"
        >
          <Save className="h-3 w-3" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6"
          onClick={closeFile}
          title="关闭"
        >
          <X className="h-3 w-3" />
        </Button>
      </div>

      {/* Editor */}
      {fileLoading ? (
        <div className="flex flex-1 items-center justify-center">
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
        </div>
      ) : (
        <Editor
          height="100%"
          defaultLanguage={getLanguage(fileName)}
          value={fileContent || ''}
          onChange={updateFileContent}
          onMount={handleEditorMount}
          theme="vs-dark"
          options={{
            fontSize: 12,
            minimap: { enabled: false },
            scrollBeyondLastLine: false,
            lineNumbers: 'on',
            renderWhitespace: 'selection',
            tabSize: 4,
            automaticLayout: true,
            readOnly: false,
            wordWrap: 'on',
            padding: { top: 8 },
          }}
        />
      )}
    </div>
  )
}

const EXT_LANG: Record<string, string> = {
  rs: 'rust',
  ts: 'typescript',
  tsx: 'typescript',
  js: 'javascript',
  jsx: 'javascript',
  py: 'python',
  json: 'json',
  html: 'html',
  css: 'css',
  md: 'markdown',
  toml: 'ini',
  yml: 'yaml',
  yaml: 'yaml',
  xml: 'xml',
  sql: 'sql',
  sh: 'shell',
  bat: 'bat',
  txt: 'plaintext',
}

function getLanguage(filename: string): string {
  const ext = filename.split('.').pop()?.toLowerCase() || ''
  return EXT_LANG[ext] || 'plaintext'
}
