import type { UIMessage, UIBlock, UIToolCall } from '@/types/ui'
import { MarkdownView } from '@/components/MarkdownView'
import { cn } from '@/lib/utils'
import { Zap } from 'lucide-react'

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
        isUser ? 'justify-end' : 'justify-start',
        className,
      )}
    >
      <div
        className={cn(
          'max-w-[85%] min-w-0 rounded-lg px-4 py-2.5 text-sm leading-relaxed overflow-hidden',
          isUser
            ? 'bg-primary text-primary-foreground'
            : 'bg-muted text-foreground',
        )}
      >
        {isUser && (
          <p className="whitespace-pre-wrap break-all overflow-wrap-anywhere">{message.content}</p>
        )}

        {!isUser && (
          <AssistantBlocks
            blocks={message.blocks}
            apiCalls={message.apiCalls}
            tokenUsage={message.tokenUsage}
          />
        )}

        {!isUser && message.blocks.length === 0 && (
          <span className="animate-pulse text-muted-foreground">...</span>
        )}
      </div>
    </div>
  )
}

function AssistantBlocks({
  blocks,
  apiCalls,
  tokenUsage,
}: {
  blocks: UIBlock[]
  apiCalls?: number
  tokenUsage?: UIMessage['tokenUsage']
}) {
  // Group adjacent toolCall blocks by same parallel_index.total
  const groups = groupParallelToolCalls(blocks)

  return (
    <div className="space-y-2">
      {groups.map((group, gi) => {
        if (group.kind === 'toolGroup' && group.toolCalls.length > 1) {
          return (
            <ParallelToolGroup key={gi} toolCalls={group.toolCalls} />
          )
        }
        if (group.kind === 'toolGroup') {
          return (
            <ToolCallCard key={gi} toolCall={group.toolCalls[0]} />
          )
        }
        return <BlockView key={gi} block={group.block!} />
      })}

      {/* Turn footer */}
      {(apiCalls != null && apiCalls > 0) || tokenUsage ? (
        <div className="flex items-center gap-3 text-[10px] text-muted-foreground border-t pt-1.5 mt-2">
          {apiCalls != null && apiCalls > 0 && (
            <span className="inline-flex items-center gap-1">
              <Zap className="h-3 w-3" />
              {apiCalls} API call{apiCalls > 1 ? 's' : ''}
            </span>
          )}
          {tokenUsage && (
            <span>
              {tokenUsage.input} in / {tokenUsage.output} out tokens
            </span>
          )}
        </div>
      ) : null}
    </div>
  )
}

function ParallelToolGroup({ toolCalls }: { toolCalls: UIToolCall[] }) {
  return (
    <div className="rounded border-2 border-blue-500/30 px-3 py-2">
      <p className="mb-1.5 text-[10px] font-semibold text-blue-600 dark:text-blue-400 uppercase tracking-wide">
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
      return <MarkdownView source={block.content} />
    case 'thinking':
      return (
        <details className="mb-1">
          <summary className="cursor-pointer text-xs font-medium text-muted-foreground">
            Thinking
          </summary>
          <div className="mt-1 whitespace-pre-wrap text-xs font-mono text-muted-foreground border-l-2 border-muted-foreground/30 pl-3">
            {block.content}
          </div>
        </details>
      )
    case 'toolCall':
      return <ToolCallCard toolCall={block.toolCall} />
    case 'error':
      return (
        <div className="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          <p className="font-semibold">{block.code}</p>
          <p>{block.message}</p>
        </div>
      )
  }
}

function ToolCallCard({ toolCall }: { toolCall: UIToolCall }) {
  const statusColors: Record<UIToolCall['status'], string> = {
    running: 'bg-yellow-500',
    done: 'bg-green-500',
    error: 'bg-red-500',
  }

  const statusLabel: Record<UIToolCall['status'], string> = {
    running: 'Running',
    done: 'Done',
    error: 'Error',
  }

  const hasDetail = toolCall.input != null || toolCall.output != null

  const inner = (
    <div className="flex items-center gap-1.5 min-w-0">
      <span
        className={cn('h-1.5 w-1.5 rounded-full shrink-0', statusColors[toolCall.status])}
      />
      <span className="text-xs font-semibold truncate">{toolCall.name}</span>
      <span className="text-[10px] text-muted-foreground shrink-0">
        {statusLabel[toolCall.status]}
      </span>
      {toolCall.parallelIndex && toolCall.parallelIndex.total > 1 && (
        <span className="text-[10px] text-muted-foreground/60 shrink-0">
          {toolCall.parallelIndex.index + 1}/{toolCall.parallelIndex.total}
        </span>
      )}
    </div>
  )

  if (!hasDetail) {
    return (
      <div className="my-0.5 rounded bg-background/40 px-2.5 py-1">
        {inner}
      </div>
    )
  }

  return (
    <details className="my-0.5 rounded bg-background/40 px-2.5 py-1 group">
      <summary className="cursor-pointer list-none flex items-center gap-1.5">
        <span className="text-[10px] transition-transform group-open:rotate-90 shrink-0">
          ▶
        </span>
        {inner}
      </summary>

      {toolCall.input != null && (
        <details className="mt-1.5 ml-4" open>
          <summary className="cursor-pointer text-[10px] text-muted-foreground">
            Input
          </summary>
          <pre className="mt-1 max-h-32 overflow-auto rounded bg-muted p-2 text-[10px] font-mono whitespace-pre-wrap break-all overflow-x-auto">
            {JSON.stringify(toolCall.input, null, 2)}
          </pre>
        </details>
      )}

      {toolCall.output && (
        <details className="mt-1.5 ml-4">
          <summary className="cursor-pointer text-[10px] text-muted-foreground">
            Output
          </summary>
          <pre className="mt-1 max-h-40 overflow-auto rounded bg-muted p-2 text-[10px] font-mono whitespace-pre-wrap break-all overflow-x-auto">
            {toolCall.output}
          </pre>
        </details>
      )}
    </details>
  )
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

    // Collect adjacent toolCalls with same parallelIndex.total
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
