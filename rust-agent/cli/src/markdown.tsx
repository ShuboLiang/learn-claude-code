import React, { useMemo } from "react";
import { Box, Text } from "ink";
import { marked } from "marked";
// marked-terminal v7 导出 markedTerminal 函数
import { markedTerminal } from "marked-terminal";

// 使用 marked.use() 注册 marked-terminal 渲染扩展
marked.use(markedTerminal() as any);

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
