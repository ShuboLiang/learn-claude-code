1. 白名单 vs 黑名单 — Claude Code 只压缩 Read, Bash, Grep, Glob, WebSearch, WebFetch, FileEdit, FileWrite 这 8 种工具的结果。你现在是除了 read_file
   什么都压缩，如果将来加了不希望被压缩的工具（比如用户自定义工具），可能会误伤。
2. 压缩后重新注入 — Claude Code 在 auto_compact 后会自动把最近读过的文件、计划文件、技能文件重新注入上下文。你没有这个，压缩后模型可能"失忆"。
