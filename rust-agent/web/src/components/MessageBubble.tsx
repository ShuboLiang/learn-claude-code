import type { UIMessage, UIBlock, UIToolCall } from '@/types/ui'
import { MarkdownView } from '@/components/MarkdownView'
import { cn } from '@/lib/utils'
import { Zap, Bot, User } from 'lucide-react'

interface Props {
  message: UIMessage
  className?: string
}

export function MessageBubble({ message, className }: Props) {
  const isUser = message.role === 'user'

  return (
    <div
      className={cn(
        'flex gap-3',
        isUser ? 'flex-row-reverse' : 'flex-row',
        className,
      )}
    >
      {/* Avatar */}
      <div
        className={cn(
          'flex h-7 w-7 shrink-0 items-center justify-center rounded-full',
          isUser
            ? 'bg-primary/15 text-primary'
            : 'bg-secondary text-foreground/60',
        )}
      >
        {isUser ? (
          <User className="h-3.5 w-3.5" />
        ) : (
          <Bot className="h-3.5 w-3.5" />
        )}
      </div>

      {/* Bubble */}
      <div className="min-w-0 max-w-[75%] space-y-1">
        {isUser ? (
          <div className="rounded-2xl rounded-tr-md bg-primary px-4 py-2.5 text-sm leading-relaxed text-primary-foreground">
            <p className="whitespace-pre-wrap break-all">
              {message.content}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            <AssistantBlocks
              blocks={message.blocks}
            />
          </div>
        )}

        {/* Turn footer */}
        {message.apiCalls != null && message.apiCalls > 0 && (
          <div className="flex items-center gap-2 px-1 text-[10px] text-muted-foreground">
            <Zap className="h-3 w-3" />
            <span>{message.apiCalls} API call{message.apiCalls > 1 ? 's' : ''}</span>
            {message.tokenUsage && (
              <span>
                &middot; {message.tokenUsage.input} in / {message.tokenUsage.output} out
              </span>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

function AssistantBlocks({
  blocks,
}: {
  blocks: UIBlock[]
}) {
  const groups = groupParallelToolCalls(blocks)

  return (
    <>
      {groups.map((group, gi) => {
        if (group.kind === 'toolGroup' && group.toolCalls.length > 1) {
          return <ParallelToolGroup key={gi} toolCalls={group.toolCalls} />
        }
        if (group.kind === 'toolGroup') {
          return <ToolCallCard key={gi} toolCall={group.toolCalls[0]} />
        }
        return <BlockView key={gi} block={group.block!} />
      })}

    </>
  )
}

function ParallelToolGroup({ toolCalls }: { toolCalls: UIToolCall[] }) {
  return (
    <div className="rounded-xl border-2 border-primary/20 bg-primary/[0.03] px-4 py-3">
      <p className="mb-2 text-[10px] font-semibold text-primary/70 uppercase tracking-wider">
        Parallel &middot; {toolCalls.length} calls
      </p>
      <div className="space-y-1.5">
        {toolCalls.map((tc) => (
          <ToolCallCard key={tc.id} toolCall={tc} />
        ))}
      </div>
    </div>
  )
}

function BlockView({ block }: { block: UIBlock }) {
  switch (block.kind) {
    case 'text':
      return (
        <div className="text-sm leading-relaxed">
          <MarkdownView source={block.content} />
        </div>
      )
    case 'thinking':
      return (
        <details className="group">
          <summary className="cursor-pointer select-none text-[11px] font-medium text-muted-foreground hover:text-foreground">
            <span className="inline-flex items-center gap-1.5">
              <span className="text-[9px] transition-transform group-open:rotate-90">▶</span>
              Thinking
            </span>
          </summary>
          <div className="mt-2 whitespace-pre-wrap rounded-lg border-l-2 border-foreground/20 bg-secondary/50 px-3 py-2 text-xs text-foreground/80 font-mono leading-relaxed">
            {block.content}
          </div>
        </details>
      )
    case 'toolCall':
      return <ToolCallCard toolCall={block.toolCall} />
    case 'error':
      return (
        <div className="rounded-xl border border-destructive/30 bg-destructive/[0.04] px-4 py-3">
          <p className="text-xs font-semibold text-destructive">{block.code}</p>
          <p className="mt-1 text-xs text-destructive/80">{block.message}</p>
        </div>
      )
  }
}

function ToolCallCard({ toolCall }: { toolCall: UIToolCall }) {
  const statusConfig: Record<UIToolCall['status'], { dot: string; label: string }> = {
    running: { dot: 'bg-yellow-500 shadow-[0_0_6px_rgba(234,179,8,0.4)]', label: '运行中' },
    done: { dot: 'bg-emerald-500', label: '完成' },
    error: { dot: 'bg-red-500', label: '错误' },
  }

  const { dot, label } = statusConfig[toolCall.status]
  const isCallBot = toolCall.name === 'call_bot'
  const hasDetail = toolCall.input != null || toolCall.output != null || isCallBot

  const inner = (
    <div className="flex items-center gap-1.5 min-w-0">
      <span className={cn('h-1.5 w-1.5 rounded-full shrink-0', dot)} />
      <span className="text-[11px] font-semibold truncate text-foreground">{toolCall.name}</span>
      <span className="text-[10px] text-muted-foreground shrink-0">{label}</span>
      {toolCall.parallelIndex && toolCall.parallelIndex.total > 1 && (
        <span className="ml-auto text-[10px] text-muted-foreground shrink-0 tabular-nums">
          {toolCall.parallelIndex.index + 1}/{toolCall.parallelIndex.total}
        </span>
      )}
    </div>
  )

  if (!hasDetail) {
    return (
      <div className="rounded-xl bg-secondary/60 px-3 py-2 ring-1 ring-border">
        {inner}
      </div>
    )
  }

  return (
    <details
      className="rounded-xl bg-secondary/60 px-3 py-2 ring-1 ring-border group transition-colors hover:bg-secondary"
      open={isCallBot && toolCall.status === 'running'}
    >
      <summary className="cursor-pointer list-none flex items-center gap-1.5">
        <span className="text-[9px] text-muted-foreground transition-transform group-open:rotate-90 shrink-0">
          ▶
        </span>
        {inner}
      </summary>

      {toolCall.input != null && (
        <details className="mt-2 ml-3.5" open>
          <summary className="cursor-pointer text-[10px] font-medium text-muted-foreground hover:text-foreground">
            Input
          </summary>
          <pre className="mt-1.5 max-h-80 overflow-auto rounded-lg bg-muted p-2.5 text-[11px] font-mono leading-relaxed whitespace-pre-wrap break-all text-foreground/80">
            {formatDisplayJSON(toolCall.input)}
          </pre>
        </details>
      )}

      {toolCall.output && (
        <details className="mt-2 ml-3.5">
          <summary className="cursor-pointer text-[10px] font-medium text-muted-foreground hover:text-foreground">
            Output
          </summary>
          <pre className="mt-1.5 max-h-80 overflow-auto rounded-lg bg-muted p-2.5 text-[11px] font-mono leading-relaxed whitespace-pre-wrap break-all text-foreground/80">
            {formatEscape(toolCall.output)}
          </pre>
        </details>
      )}

      {/* Bot 实时思考过程 */}
      {toolCall.botThinking && (
        <details className="mt-2 ml-3.5" open>
          <summary className="cursor-pointer text-[10px] font-medium text-muted-foreground hover:text-foreground">
            Thinking
          </summary>
          <div className="mt-1.5 whitespace-pre-wrap rounded-lg border-l-2 border-foreground/20 bg-secondary/50 px-3 py-2 text-xs text-foreground/80 font-mono leading-relaxed">
            {toolCall.botThinking}
          </div>
        </details>
      )}

      {/* Bot 实时回复（流式期间）或最终输出（完成后） */}
      {(toolCall.botText || toolCall.output) && (
        <div className="mt-2 ml-3.5">
          <p className="text-[10px] font-medium text-muted-foreground mb-1">Bot 回复</p>
          <div className="text-sm leading-relaxed">
            <MarkdownView source={toolCall.botText || toolCall.output || ''} />
          </div>
        </div>
      )}

      {/* Bot 子代理内部的嵌套工具调用 */}
      {toolCall.children && toolCall.children.length > 0 && (
        <div className="mt-3 ml-3.5 space-y-1.5">
          <p className="text-[10px] font-medium text-muted-foreground/70 uppercase tracking-wider">
            Bot 工具调用
          </p>
          {toolCall.children.map((child) => (
            <NestedToolCall key={child.id} toolCall={child} />
          ))}
        </div>
      )}
    </details>
  )
}

/** 嵌套子工具调用卡片（用于 call_bot 内部的工具调用） */
function NestedToolCall({ toolCall }: { toolCall: UIToolCall }) {
  const statusConfig: Record<UIToolCall['status'], { dot: string; label: string }> = {
    running: { dot: 'bg-yellow-500 shadow-[0_0_6px_rgba(234,179,8,0.4)]', label: '运行中' },
    done: { dot: 'bg-emerald-500', label: '完成' },
    error: { dot: 'bg-red-500', label: '错误' },
  }

  const { dot, label } = statusConfig[toolCall.status]
  const hasDetail = toolCall.input != null || toolCall.output != null

  const inner = (
    <div className="flex items-center gap-1.5 min-w-0">
      <span className={cn('h-1.5 w-1.5 rounded-full shrink-0', dot)} />
      <span className="text-[11px] font-semibold truncate text-foreground">{toolCall.name}</span>
      <span className="text-[10px] text-muted-foreground shrink-0">{label}</span>
    </div>
  )

  if (!hasDetail) {
    return (
      <div className="rounded-lg bg-muted/50 px-2.5 py-1.5 ring-1 ring-border/60">
        {inner}
      </div>
    )
  }

  return (
    <details className="rounded-lg bg-muted/50 px-2.5 py-1.5 ring-1 ring-border/60 group">
      <summary className="cursor-pointer list-none flex items-center gap-1.5">
        <span className="text-[9px] text-muted-foreground transition-transform group-open:rotate-90 shrink-0">
          ▶
        </span>
        {inner}
      </summary>

      {toolCall.input != null && (
        <details className="mt-1.5 ml-3" open>
          <summary className="cursor-pointer text-[10px] font-medium text-muted-foreground hover:text-foreground">
            Input
          </summary>
          <pre className="mt-1 max-h-60 overflow-auto rounded bg-background p-2 text-[11px] font-mono leading-relaxed whitespace-pre-wrap break-all text-foreground/80">
            {formatDisplayJSON(toolCall.input)}
          </pre>
        </details>
      )}

      {toolCall.output && (
        <details className="mt-1.5 ml-3">
          <summary className="cursor-pointer text-[10px] font-medium text-muted-foreground hover:text-foreground">
            Output
          </summary>
          <pre className="mt-1 max-h-60 overflow-auto rounded bg-background p-2 text-[11px] font-mono leading-relaxed whitespace-pre-wrap break-all text-foreground/80">
            {formatEscape(toolCall.output)}
          </pre>
        </details>
      )}
    </details>
  )
}

function formatDisplayJSON(value: unknown): string {
  return JSON.stringify(value, null, 2)
    .replace(/\\n/g, '\n')
    .replace(/\\t/g, '\t')
    .replace(/\\r/g, '\r')
}

function formatEscape(text: string): string {
  return text
    .replace(/\\n/g, '\n')
    .replace(/\\t/g, '\t')
    .replace(/\\r/g, '\r')
}

// ── Parallel tool call grouper ──

type BlockGroup =
  | { kind: 'single'; block: UIBlock }
  | { kind: 'toolGroup'; toolCalls: UIToolCall[] }

function groupParallelToolCalls(blocks: UIBlock[]): BlockGroup[] {
  const groups: BlockGroup[] = []

  for (let i = 0; i < blocks.length; i++) {
    const block = blocks[i]
    if (block.kind !== 'toolCall') {
      groups.push({ kind: 'single', block })
      continue
    }

    const pi = block.toolCall.parallelIndex
    if (!pi || pi.total <= 1) {
      groups.push({ kind: 'single', block })
      continue
    }

    const toolCalls: UIToolCall[] = [block.toolCall]
    while (i + 1 < blocks.length) {
      const next = blocks[i + 1]
      if (next.kind !== 'toolCall' || next.toolCall.parallelIndex?.total !== pi.total) break
      i++
      toolCalls.push(next.toolCall)
    }
    groups.push({ kind: 'toolGroup', toolCalls })
  }

  return groups
}
