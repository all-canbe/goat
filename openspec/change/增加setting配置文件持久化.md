# 增加 setting.json 配置文件持久化

## 变更内容

### 1. 新增 setting.json 配置持久化

- 添加 `SETTINGS_FILE` 常量，指向项目根目录的 `setting.json`
- 新增 `_load_settings()` 方法 — 从 `setting.json` 读取已保存的配置
- 新增 `_save_settings()` 方法 — 将配置写入 `setting.json`

### 2. 启动时优先读取配置

- 启动时先尝试从 `setting.json` 中读取已有配置
- 若存在且 `api_key` 完整有效，则跳过手动输入，直接加载
- 若不存在或 `api_key` 为空，则回退到手动初始化流程

### 3. 首次初始化后自动保存

- 手动完成初始化后，自动将 `base_url`、`api_key`、`model`、`max_concurrent`、`max_depth` 写入 `setting.json`
- 后续启动直接复用该配置，无需重复输入

### 4. setting.json 格式

```json
{
  "base_url": "https://api.openai.com/v1",
  "api_key": "sk-xxx",
  "model": "gpt-4o",
  "max_concurrent": 10,
  "max_depth": 3
}
```

## 修改的文件

- `main.py` — 添加配置读写方法、修改初始化流程

## 修改意义

- **避免重复配置**：首次初始化后自动保存，后续启动秒级进入交互
- **安全校验**：只有 `api_key` 非空时才自动加载，避免无效配置导致启动异常
- **透明可控**：配置以明文 JSON 存储，用户可以随时查看和手动修改 `setting.json`
