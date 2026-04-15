import React, { useMemo } from 'react';
import { Box, Text } from 'ink';
import { marked } from 'marked';
// @ts-ignore marked-terminal 没有 TypeScript 类型导出
import TerminalRenderer from 'marked-terminal';

// 初始化 marked-terminal 渲染器
marked.setOptions({
  // @ts-expect-error marked 的 renderer 类型与 TerminalRenderer 不完全兼容
  renderer: new TerminalRenderer(),
});

interface MarkdownProps {
  content: string;
}

// 将 Markdown 渲染为带 ANSI 转义码的文本
export default function Markdown({ content }: MarkdownProps) {
  const rendered = useMemo(() => {
    try {
      return marked.parse(content, { async: false }) as string;
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
