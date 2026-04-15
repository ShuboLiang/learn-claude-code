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

  const handleSubmit = (text: string) => {
    if (!text.trim()) return;
    const lower = text.trim().toLowerCase();
    if (lower === 'q' || lower === 'quit' || lower === 'exit' || lower === '/exit') {
      onQuit(text);
      return;
    }
    if (lower === '/clear') {
      onClear();
      setValue('');
      return;
    }
    setValue('');
    onSubmit(text);
  };

  const placeholder = isLoading
    ? '(等待响应中...)'
    : model
      ? `[${model}] 输入消息，Enter 提交...`
      : '输入消息，Enter 提交...';

  return (
    <Box borderStyle="single" paddingLeft={1}>
      <TextInput
        value={value}
        onChange={setValue}
        onSubmit={handleSubmit}
        placeholder={placeholder}
      />
    </Box>
  );
}
