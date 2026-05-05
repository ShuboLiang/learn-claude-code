import { useCallback, useRef, useEffect, useState } from 'react'
import Editor, { type OnMount } from '@monaco-editor/react'
import { X, Save, Loader2, Maximize2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
} from '@/components/ui/dialog'
import { useWorkspaceStore } from '@/store/workspace'

function base64ToBlobUrl(base64: string, mime: string): string {
  const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0))
  const blob = new Blob([bytes], { type: mime })
  return URL.createObjectURL(blob)
}

function MediaModal({
  open,
  onOpenChange,
  fileName,
  fileContent,
  isPdf,
  isImage,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  fileName: string
  fileContent: string | null
  isPdf: boolean
  isImage: boolean
}) {
  const [blobUrl, setBlobUrl] = useState<string | null>(null)

  const ext = fileName.split('.').pop()?.toLowerCase() || 'png'
  const mimeMap: Record<string, string> = {
    pdf: 'application/pdf',
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    gif: 'image/gif',
    svg: 'image/svg+xml',
    webp: 'image/webp',
    bmp: 'image/bmp',
    ico: 'image/x-icon',
  }
  const mime = mimeMap[ext] || 'application/octet-stream'

  useEffect(() => {
    if (!open || !fileContent) {
      setBlobUrl(null)
      return
    }
    const url = base64ToBlobUrl(fileContent, mime)
    setBlobUrl(url)
    return () => URL.revokeObjectURL(url)
  }, [open, fileContent, mime])

  if (!fileContent || !blobUrl) return null

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[90vw] max-h-[90vh] w-[90vw] h-[90vh] p-2">
        <div className="flex items-center gap-2 px-2 py-1 border-b border-border/50">
          <span className="flex-1 truncate text-xs font-medium text-muted-foreground">
            {fileName}
          </span>
        </div>
        <div className="flex-1 min-h-0 h-[calc(90vh-40px)]">
          {isPdf ? (
            <iframe
              src={blobUrl}
              className="w-full h-full border-0 rounded"
              title={fileName}
            />
          ) : isImage ? (
            <div className="flex items-center justify-center h-full bg-neutral-900/50 rounded">
              <img
                src={blobUrl}
                alt={fileName}
                className="max-w-full max-h-full object-contain"
              />
            </div>
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  )
}

export function FilePreview() {
  const selectedFile = useWorkspaceStore((s) => s.selectedFile)
  const fileContent = useWorkspaceStore((s) => s.fileContent)
  const fileBinary = useWorkspaceStore((s) => s.fileBinary)
  const fileDirty = useWorkspaceStore((s) => s.fileDirty)
  const fileLoading = useWorkspaceStore((s) => s.fileLoading)
  const closeFile = useWorkspaceStore((s) => s.closeFile)
  const updateFileContent = useWorkspaceStore((s) => s.updateFileContent)
  const saveFile = useWorkspaceStore((s) => s.saveFile)
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null)
  const [mediaOpen, setMediaOpen] = useState(false)

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

  // Auto-open media modal for PDFs/images
  useEffect(() => {
    if (selectedFile && fileBinary && !fileLoading) {
      const name = selectedFile.replace(/\\/g, '/').split('/').pop() || selectedFile
      if (/\.(pdf|png|jpe?g|gif|svg|webp|bmp|ico)$/i.test(name)) {
        setMediaOpen(true)
      }
    }
  }, [selectedFile, fileBinary, fileLoading])

  // Close modal when file closes
  useEffect(() => {
    if (!selectedFile) {
      setMediaOpen(false)
    }
  }, [selectedFile])

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
  const isPdf = /\.pdf$/i.test(fileName)
  const isImage = /\.(png|jpe?g|gif|svg|webp|bmp|ico)$/i.test(fileName)
  const isMedia = isPdf || isImage

  const renderContent = () => {
    if (fileLoading) {
      return (
        <div className="flex flex-1 items-center justify-center">
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
        </div>
      )
    }

    if (fileBinary) {
      if (isMedia) {
        return (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 p-4">
            <p className="text-[10px] text-muted-foreground text-center">
              {isPdf ? 'PDF 文件' : '图片文件'}
            </p>
            <Button
              variant="outline"
              size="sm"
              className="h-7 text-xs"
              onClick={() => setMediaOpen(true)}
            >
              <Maximize2 className="mr-1 h-3 w-3" />
              打开预览
            </Button>
          </div>
        )
      }
      return (
        <div className="flex flex-1 items-center justify-center p-4">
          <p className="text-xs text-muted-foreground">二进制文件，无法预览</p>
        </div>
      )
    }

    return (
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
    )
  }

  return (
    <>
      <div className="flex flex-col h-full min-h-0">
        {/* Preview header */}
        <div className="flex items-center gap-1 border-b border-border/50 px-1.5 py-1">
          <span className="flex-1 truncate text-[10px] font-medium text-muted-foreground">
            {fileName}
            {fileDirty && <span className="ml-1 text-primary">●</span>}
          </span>
          {!fileBinary && (
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
          )}
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

        {renderContent()}
      </div>

      {/* Full-size media modal */}
      {isMedia && fileContent && (
        <MediaModal
          open={mediaOpen}
          onOpenChange={setMediaOpen}
          fileName={fileName}
          fileContent={fileContent}
          isPdf={isPdf}
          isImage={isImage}
        />
      )}
    </>
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
