import React, { useMemo } from "react";
import { Box, Text } from "ink";
import { marked } from "marked";
// marked-terminal v7 导出 markedTerminal 函数
import { markedTerminal } from "marked-terminal";

const mt = markedTerminal() as any;

// 修复：marked-terminal 的 text renderer 没有处理嵌套 tokens
// 导致 listitem 中的 **粗体** 等内联标记被原样输出
const customRenderer = {
  ...mt.renderer,
  text(token: any) {
    if (typeof token === "object" && token.tokens) {
      return this.parser.parseInline(token.tokens);
    }
    if (typeof token === "object") {
      token = token.text;
    }
    return mt.renderer.text.call(this, token);
  },
};

// 使用 marked.use() 注册 marked-terminal 渲染扩展
marked.use({ renderer: customRenderer });

interface MarkdownProps {
  content: string;
}

// 将 Markdown 渲染为带 ANSI 转义码的终端格式化文本
export default function Markdown({ content }: MarkdownProps) {
  const rendered = useMemo(() => {
    try {
      return marked.parse(content) as string;
    } catch {
      return content;
    }
  }, [content]);

  return (
    <Box flexDirection="column">
      <Text>{rendered}</Text>
    </Box>
  );
}
