//! 工具 JSON Schema 定义
//!
//! 定义所有可用工具的名称、描述和参数格式，用于传给 Claude API。

use serde_json::{Value, json};

/// 生成所有工具的 JSON Schema 定义列表
pub fn tool_schemas(allow_task: bool) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "bash",
            "description": "执行 shell 命令。",
            "input_schema": {
                "type": "object",
                "properties": { "command": { "type": "string", "description": "要执行的 shell 命令" } },
                "required": ["command"]
            }
        }),
        json!({
            "name": "read_file",
            "description": "读取文件内容。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "文件路径" },
                    "limit": { "type": "integer", "description": "读取行数限制" }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "write_file",
            "description": "将内容写入文件。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "目标文件路径" },
                    "content": { "type": "string", "description": "要写入的内容" }
                },
                "required": ["path", "content"]
            }
        }),
        json!({
            "name": "edit_file",
            "description": "在文件中精确替换一段文本（首次匹配）。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "文件路径" },
                    "old_text": { "type": "string", "description": "要被替换的原始文本" },
                    "new_text": { "type": "string", "description": "替换后的新文本" }
                },
                "required": ["path", "old_text", "new_text"]
            }
        }),
        json!({
            "name": "todo",
            "description": "更新任务列表。用于规划和跟踪多步骤任务的进度。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "任务唯一标识" },
                                "text": { "type": "string", "description": "任务描述" },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"],
                                    "description": "任务状态：待处理、进行中、已完成"
                                }
                            },
                            "required": ["id", "text", "status"]
                        }
                    }
                },
                "required": ["items"]
            }
        }),
        json!({
            "name": "load_skill",
            "description": "按名称加载已安装的技能知识。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "要加载的技能名称" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "glob",
            "description": "使用 glob 模式快速搜索匹配的文件路径。支持通配符如 **/*.rs、src/**/*.ts 等。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "glob 模式，如 **/*.rs、src/**/*.ts、*.toml" },
                    "path": { "type": "string", "description": "搜索的基准目录（可选，默认为工作区根目录）" }
                },
                "required": ["pattern"]
            }
        }),
        json!({
            "name": "grep",
            "description": "在文件内容中搜索匹配正则表达式的行。支持多种输出模式、上下文行、大小写忽略等。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "正则表达式搜索模式" },
                    "path": { "type": "string", "description": "搜索的文件或目录路径（可选，默认为工作区根目录）" },
                    "glob": { "type": "string", "description": "用于过滤文件的 glob 模式，如 *.rs（可选）" },
                    "output_mode": {
                        "type": "string",
                        "enum": ["files_with_matches", "content", "count"],
                        "description": "输出模式：files_with_matches 只返回文件路径，content 返回匹配行及行号，count 返回每个文件的匹配数（可选，默认 files_with_matches）"
                    },
                    "-i": { "type": "boolean", "description": "是否忽略大小写（可选，默认 false）" },
                    "-C": { "type": "integer", "description": "显示匹配行前后各多少行上下文（可选）" },
                    "head_limit": { "type": "integer", "description": "限制返回的最大结果数（可选，默认 250）" }
                },
                "required": ["pattern"]
            }
        }),
        json!({
            "name": "search_skillhub",
            "description": "搜索 SkillHub 技能商店中的可用技能。当本地没有安装所需技能时使用。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "queries": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "搜索关键词列表。每个关键词会单独搜索后合并结果。"
                    }
                },
                "required": ["queries"]
            }
        }),
        json!({
            "name": "install_skill",
            "description": "从 SkillHub 安装一个技能。每次调用只安装一个技能，不要批量安装。安装后技能即可使用。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "要安装的技能名称" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "list_skills",
            "description": "列出所有已安装技能的摘要信息（名称、描述、标签）。无需参数。",
            "input_schema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "curl",
            "description": "发起 HTTP 请求。默认返回响应 body，detailed=true 时返回完整信息（含 status/headers）。",
            "input_schema": {
                "type": "object",
                "required": ["url"],
                "properties": {
                    "url": { "type": "string", "description": "请求地址" },
                    "method": { "type": "string", "enum": ["GET", "POST", "PUT", "DELETE", "PATCH"], "default": "GET", "description": "HTTP 方法（可选，默认 GET）" },
                    "headers": { "type": "object", "description": "可选的请求头，键值对形式" },
                    "body": { "type": "string", "description": "原始 body 文本（与 json 参数互斥）" },
                    "json": { "type": "object", "description": "JSON body，自动设置 Content-Type: application/json（与 body 参数互斥）" },
                    "timeout": { "type": "integer", "description": "超时秒数（可选，默认 30）" },
                    "detailed": { "type": "boolean", "description": "返回完整响应信息（含 status/headers/body），可选，默认 false", "default": false }
                }
            }
        }),
        json!({
            "name": "compact",
            "description": "触发手动对话压缩。当上下文过长时使用，将对话历史压缩为摘要。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "focus": { "type": "string", "description": "摘要中需要重点保留的内容" }
                }
            }
        }),
    ];

    if allow_task {
        tools.push(json!({
            "name": "task",
            "description": "启动一个拥有独立上下文的子代理来执行子任务。子代理共享文件系统，但不共享对话历史。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "prompt": { "type": "string", "description": "子代理的任务描述" },
                    "description": { "type": "string", "description": "任务的简要标题" }
                },
                "required": ["prompt"]
            }
        }));
    }

    tools
}
