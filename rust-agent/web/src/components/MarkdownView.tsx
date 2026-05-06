import React from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'

interface Props {
  source: string
}

export const MarkdownView = React.memo(function MarkdownView({ source }: Props) {
  return (
    <div className="max-w-none [&_pre]:overflow-x-auto [&_code]:break-words [&_pre]:max-h-[32rem] [&_table]:w-full [&_table]:border-collapse [&_table]:my-2 [&_table]:text-sm [&_th]:border [&_th]:border-border [&_th]:px-3 [&_th]:py-2 [&_th]:text-center [&_th]:bg-muted/50 [&_th]:min-w-[120px] [&_td]:border [&_td]:border-border [&_td]:px-3 [&_td]:py-2 [&_td]:text-center [&_td]:min-w-[120px] [&_td]:break-words">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
      >
        {source}
      </ReactMarkdown>
    </div>
  )
})
