import React, { useState } from 'react';
import { Box, Text } from 'ink';
import TextInput from 'ink-text-input';

interface InputProps {
  onSubmit: (text: string) => void;
  onQuit: (text: string) => void;
  onClear: () => void;
  isLoading: boolean;
  model?: string;
}

export default function Input({ onSubmit, onQuit, onClear, isLoading, model }: InputProps) {
  const [value, setValue] = useState('');
  const [isMultiline, setIsMultiline] = useState(false);
  const [buffer, setBuffer] = useState('');

  const handleSubmit = (text: string) => {
    if (!text.trim() && !isMultiline) return;
    const lower = text.trim().toLowerCase();

    // 多行模式
    if (isMultiline) {
      if (lower === '/send') {
        onSubmit(buffer);
        setIsMultiline(false);
        setBuffer('');
        setValue('');
      } else if (lower === '/cancel') {
        setIsMultiline(false);
        setBuffer('');
        setValue('');
      } else {
        setBuffer(prev => (prev ? prev + '\n' + text : text));
        setValue('');
      }
      return;
    }

    // 普通模式下的命令
    if (lower === 'q' || lower === 'quit' || lower === 'exit' || lower === '/exit') {
      onQuit(text);
      return;
    }
    if (lower === '/clear') {
      onClear();
      setValue('');
      return;
    }
    if (lower === '/multiline' || lower === '/m') {
      setIsMultiline(true);
      setValue('');
      return;
    }

    // 普通提交：支持 \n 转义为真实换行
    setValue('');
    onSubmit(text.replace(/\\n/g, '\n'));
  };

  const placeholder = isLoading
    ? '(等待响应中...)'
    : isMultiline
      ? '多行模式: 输入 /send 提交, /cancel 取消...'
      : model
        ? `[${model}] 输入消息, Enter 提交, /m 多行, \\n 换行...`
        : '输入消息, Enter 提交, /m 多行, \\n 换行...';

  return (
    <Box flexDirection="column">
      {isMultiline && buffer && (
        <Box flexDirection="column" borderStyle="single" paddingLeft={1} paddingRight={1}>
          <Text color="gray" dimColor>已输入内容预览:</Text>
          {buffer.split('\n').map((line, i) => (
            <Text key={i} color="cyan">{line || ' '}</Text>
          ))}
        </Box>
      )}
      <Box borderStyle="single" paddingLeft={1}>
        <TextInput
          value={value}
          onChange={setValue}
          onSubmit={handleSubmit}
          placeholder={placeholder}
        />
      </Box>
    </Box>
  );
}
