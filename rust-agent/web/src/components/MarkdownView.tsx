import React from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'

interface Props {
  source: string
}

export const MarkdownView = React.memo(function MarkdownView({ source }: Props) {
  return (
    <div className="max-w-none [&_pre]:overflow-x-auto [&_code]:break-words [&_pre]:max-h-[32rem]">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
      >
        {source}
      </ReactMarkdown>
    </div>
  )
})
