import React, { useRef, useState } from "react";
import { Box, Text, useInput } from "ink";
import TextInput from "ink-text-input";

interface InputProps {
  onSubmit: (text: string) => void;
  onQuit: (text: string) => void;
  onClear: () => void;
  isLoading: boolean;
  model?: string;
}

export default function Input({
  onSubmit,
  onQuit,
  onClear,
  isLoading,
  model,
}: InputProps) {
  const [value, setValue] = useState("");
  const [isMultiline, setIsMultiline] = useState(false);
  const [buffer, setBuffer] = useState("");
  const historyRef = useRef<string[]>([]);
  const historyIndexRef = useRef<number>(-1);

  useInput((_input, key) => {
    if (key.escape) {
      // ESC 取消当前输入
      if (isMultiline) {
        setIsMultiline(false);
        setBuffer("");
      }
      setValue("");
      historyIndexRef.current = -1;
    } else if (key.upArrow) {
      // 上键翻阅历史
      const history = historyRef.current;
      if (history.length === 0 || isMultiline) return;
      if (historyIndexRef.current === -1) {
        historyIndexRef.current = history.length - 1;
      } else if (historyIndexRef.current > 0) {
        historyIndexRef.current--;
      }
      setValue(history[historyIndexRef.current]);
    } else if (key.downArrow) {
      // 下键翻阅历史
      const history = historyRef.current;
      if (history.length === 0 || isMultiline || historyIndexRef.current === -1)
        return;
      if (historyIndexRef.current < history.length - 1) {
        historyIndexRef.current++;
        setValue(history[historyIndexRef.current]);
      } else {
        historyIndexRef.current = -1;
        setValue("");
      }
    }
  });

  const handleSubmit = (text: string) => {
    if (!text.trim() && !isMultiline) return;
    const lower = text.trim().toLowerCase();

    // 多行模式
    if (isMultiline) {
      if (lower === "/send") {
        onSubmit(buffer);
        historyRef.current = [...historyRef.current, buffer];
        historyIndexRef.current = -1;
        setIsMultiline(false);
        setBuffer("");
        setValue("");
      } else if (lower === "/cancel") {
        setIsMultiline(false);
        setBuffer("");
        setValue("");
      } else {
        setBuffer((prev) => (prev ? prev + "\n" + text : text));
        setValue("");
      }
      return;
    }

    // 普通模式下的命令
    if (
      lower === "q" ||
      lower === "quit" ||
      lower === "exit" ||
      lower === "/exit"
    ) {
      onQuit(text);
      return;
    }
    if (lower === "/clear") {
      onClear();
      setValue("");
      return;
    }
    if (lower === "/multiline" || lower === "/m") {
      setIsMultiline(true);
      setValue("");
      return;
    }

    // 普通提交：支持 \n 转义为真实换行
    setValue("");
    historyRef.current = [...historyRef.current, text];
    historyIndexRef.current = -1;
    onSubmit(text.replace(/\\n/g, "\n"));
  };

  const placeholder = isLoading
    ? "(等待响应中...)"
    : isMultiline
      ? "多行模式: 输入 /send 提交, ESC 或 /cancel 取消..."
      : model
        ? `[${model}] 输入消息, Enter 提交, /@bot 委派任务, /bots 查看bot, /m 多行...`
        : "输入消息, Enter 提交, /@bot 委派任务, /m 多行...";

  return (
    <Box flexDirection="column">
      {isMultiline && buffer && (
        <Box
          flexDirection="column"
          borderStyle="single"
          paddingLeft={1}
          paddingRight={1}
        >
          <Text color="gray" dimColor>
            已输入内容预览:
          </Text>
          {buffer.split("\n").map((line, i) => (
            <Text key={i} color="cyan">
              {line || " "}
            </Text>
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
