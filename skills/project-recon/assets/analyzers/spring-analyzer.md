---
name: spring-analyzer
description: Spring Boot 后端项目分析器插件。自包含的分析方案，由编排器加载执行。
version: 1.0.0
---

# Spring Boot / 后端分析器

<!-- @类型: 分析器插件 -->
<!-- @目的: 对 Spring Boot / Java 后端项目进行完整分析 -->

## 适用识别条件

当项目包含以下任一条件时由编排器加载：

- `pom.xml` 中存在 `spring-boot` 相关依赖
- `build.gradle` 中存在 `org.springframework.boot` 插件
- 存在 `src/main/java/` 目录结构

## 分析维度

### 维度 1：框架与核心组件

**执行动作：**
- 读取 `pom.xml` 或 `build.gradle`，提取 Spring Boot 版本
- 识别 Web 框架（Spring MVC / Spring WebFlux）
- 识别 ORM（MyBatis / MyBatis-Plus / JPA / Hibernate）
- 识别数据库类型（MySQL / PostgreSQL / MongoDB 等）
- 识别缓存方案（Redis / Caffeine 等）

**输出格式：**
```
框架: Spring Boot v{版本}
Web: {Spring MVC / WebFlux}
ORM: {MyBatis-Plus v{版本} / JPA}
数据库: {MySQL / PostgreSQL / ...}
```

### 维度 2：项目分层结构

**执行动作：**
- 扫描 `src/main/java/` 下的包结构
- 识别分层模式（Controller → Service → Mapper/Repository → Entity）
- 识别包组织方式（按功能域 vs 按技术层）
- 定位配置文件（`application.yml`、`application.properties`）

### 维度 3：API 端点

**执行动作：**
- 扫描所有 `@RestController` 或 `@Controller` 类
- 提取 `@RequestMapping`、`@GetMapping`、`@PostMapping` 等注解
- 按 Controller 分组列出 API 端点
- 识别统一响应格式（如 `Result<T>`、`{ code, data, msg }`）

### 维度 4：数据模型

**执行动作：**
- 扫描 Entity / Model 类
- 识别数据库表映射
- 识别 DTO / VO 转换策略
- 列出关键业务模型

### 维度 5：横切关注点

**执行动作：**
- 识别认证/授权方案（Spring Security / Sa-Token / JWT）
- 识别全局异常处理（`@ControllerAdvice`）
- 识别日志方案（Logback / Log4j2）
- 识别定时任务（`@Scheduled` / XXL-Job）
- 识别消息队列（RabbitMQ / Kafka / RocketMQ）

### 维度 6：部署配置

**执行动作：**
- 检查 Dockerfile / docker-compose.yml
- 检查 CI/CD 配置（`.github/workflows/`、`Jenkinsfile`）
- 检查多环境配置（application-dev.yml 等）

### 维度 7：开发约定

**执行动作：**
- 读取 `.trae/rules/` 目录下所有规则文件
- 识别代码规范（Checkstyle / SpotBugs）
- 识别测试框架（JUnit / Mockito）

## 模块提取策略

1. 按 Controller 分组推断业务模块（如 UserController → 用户模块）
2. 按 Service 层补充模块职责
3. 按包结构确认模块边界
4. 按 Entity 关系推断数据模块

## 输出格式

返回给编排器的结构化数据：

```
## 分析器输出

### 项目基本信息
{框架/ORM/数据库/构建工具}

### 目录结构
{包 → 职责映射表}

### 模块地图
| 模块名 | 职责 | 关键文件 | 依赖关系 |
|--------|------|---------|---------|

### 核心依赖
| 依赖 | 版本 | 用途 |
|------|------|------|

### API 端点清单
| Controller | 端点 | 方法 | 说明 |
|-----------|------|------|------|

### 数据模型
{Entity 列表和关系}

### 开发约定
{规则文件摘要}

### 入口索引
{入口文件路径和说明}
```

---

## 版本历史

- **v1.0.0** (2026-05-29) - 初始版本
