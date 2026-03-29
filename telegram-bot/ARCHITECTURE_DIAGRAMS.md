# telegram-bot 架构图与流程图

## 1. 模块依赖关系

```mermaid
graph TB
    subgraph entry["入口"]
        main["main.rs"]
        lib["lib.rs"]
    end

    subgraph infra["基础设施"]
        config["config/"]
        logging["logging.rs"]
        error["error.rs"]
        constants["constants.rs"]
        utils["utils.rs"]
    end

    subgraph core["核心运行时"]
        bot["bot.rs<br/>BotManager"]
        router["router.rs"]
        handler_deps["handler_deps.rs"]
        health["health.rs"]
        metrics["metrics.rs"]
    end

    subgraph pipeline_layer["处理管线"]
        pipeline["pipeline/"]
        command["command/"]
    end

    subgraph abstractions["抽象层"]
        traits["traits.rs"]
    end

    subgraph implementations["实现层"]
        agent["agent.rs<br/>LoomAgentRunner"]
        sender["sender.rs<br/>TeloxideSender"]
        session["session.rs<br/>SqliteSessionManager"]
        download["download.rs<br/>TeloxideDownloader"]
        model_sel["model_selection.rs"]
    end

    subgraph streaming_layer["流式系统"]
        s_agent["streaming/agent.rs"]
        s_mapper["streaming/event_mapper.rs"]
        s_handler["streaming/message_handler.rs"]
        s_retry["streaming/retry.rs"]
    end

    subgraph ui["格式化"]
        formatting["formatting/"]
    end

    subgraph test["测试"]
        mock["mock.rs"]
    end

    main --> config
    main --> logging
    main --> lib
    lib --> bot
    lib --> router
    lib --> handler_deps
    lib --> pipeline
    lib --> command
    lib --> agent
    lib --> sender
    lib --> session
    lib --> download
    lib --> model_sel
    lib --> streaming_layer
    lib --> formatting
    lib --> traits
    lib --> error
    lib --> constants
    lib --> utils
    lib --> health
    lib --> metrics
    lib --> mock

    bot --> config
    bot --> router
    bot --> health

    router --> handler_deps
    router --> pipeline

    handler_deps --> traits
    handler_deps --> agent
    handler_deps --> sender
    handler_deps --> session
    handler_deps --> download
    handler_deps --> model_sel

    pipeline --> command
    pipeline --> agent
    pipeline --> download
    pipeline --> traits

    agent --> sender
    agent --> s_agent

    s_agent --> s_mapper
    s_agent --> s_handler

    s_mapper --> s_handler
    s_handler --> sender
    s_handler --> formatting
    s_handler --> s_retry

    sender --> formatting
    sender --> s_retry

    command --> model_sel
    command --> session
    command --> handler_deps

    mock --> traits

    session --> download
    model_sel --> constants

    classDef entry fill:#4A90D9,stroke:#2E6BAD,color:#fff
    classDef infra fill:#7B68EE,stroke:#5A4FCF,color:#fff
    classDef core fill:#2ECC71,stroke:#27AE60,color:#fff
    classDef pipeline_layer fill:#E67E22,stroke:#D35400,color:#fff
    classDef abstractions fill:#E74C3C,stroke:#C0392B,color:#fff
    classDef implementations fill:#1ABC9C,stroke:#16A085,color:#fff
    classDef streaming_layer fill:#F39C12,stroke:#E67E22,color:#fff
    classDef ui fill:#9B59B6,stroke:#8E44AD,color:#fff
    classDef test fill:#95A5A6,stroke:#7F8C8D,color:#fff
```

---

## 2. 启动流程

```mermaid
flowchart TD
    A["main()"] --> B["config::load_and_apply_with_report()"]
    B --> B1["加载 ~/.loom/config.toml + .env"]
    B1 --> B2["设置环境变量<br/>OPENAI_API_KEY, MODEL 等"]

    B2 --> C["load_config()"]
    C --> C1{"找到<br/>telegram-bot.toml?"}
    C1 -->|No| C2["打印帮助信息<br/>process::exit(1)"]
    C1 -->|Yes| C3["解析 TelegramBotConfig<br/>+ 环境变量插值"]

    C3 --> D["setup_logging()"]
    D --> D1{"配置了<br/>log_file?"}
    D1 -->|Yes| D2["双输出<br/>stdout + 文件"]
    D1 -->|No| D3["仅 stdout"]

    D2 --> E
    D3 --> E["run_with_config(config)"]

    E --> F["BotManager::new()"]
    F --> G["start_health_server()<br/>axum /health /ready"]

    G --> H{"遍历 bots"}
    H --> I{"bot.enabled?"}
    I -->|No| H
    I -->|Yes| J["spawn run_bot()"]
    J --> K["创建 teloxide Bot"]
    K --> L["构建 Dispatcher"]
    L --> M["注册 default_handler"]
    M --> N["长轮询 loop"]
    N --> H

    H -->|全部完成| O["等待所有 task"]
```

---

## 3. 消息处理主流程

```mermaid
flowchart TD
    A["Telegram Server<br/>Long Polling"] -->|"Update"| B["teloxide Dispatcher"]
    B --> C["default_handler()"]
    C --> D["HandlerDeps::production()"]
    D --> E["handle_message_with_deps(deps, msg)"]

    E --> F{"msg.kind?"}
    F -->|Common| G["pipeline::handle_common_message()"]
    F -->|Other| Z["忽略"]

    G --> H["ensure_download_dir()"]
    H --> I{"提取内容"}

    I -->|"文本"| J["handle_text_message()"]
    I -->|"媒体"| K["handle_media_message()"]
    I -->|"无内容"| Z

    subgraph cmd["命令处理"]
        J --> L["strip_bot_mention()"]
        L --> M["CommandDispatcher::dispatch()"]
        M --> M1{"/reset?"}
        M1 -->|Yes| M2["SessionManager::reset()"]
        M1 -->|No| M3{"/status?"}
        M3 -->|Yes| M4["返回状态信息"]
        M3 -->|No| M5{"/model?"}
        M5 -->|Yes| M6["ModelSelectionService"]
        M5 -->|No| M7["无匹配,继续"]
    end

    M7 --> N{"群聊?"}
    N -->|Yes| O{"被 @mention<br/>或 reply?"}
    O -->|No| Z
    O -->|Yes| P["build_prompt_with_reply()"]
    N -->|No| P

    P --> Q["run_agent_for_chat()"]
    Q --> R{"ChatRunRegistry<br/>并发守卫"}
    R -->|被占用| R1["等待 / 返回错误"]
    R -->|获取锁| S["AgentRunner::run()"]

    K --> K1["TeloxideDownloader::download_*()"]
    K1 --> K2["保存到 download_dir"]

    subgraph agent_run["Agent 执行"]
        S --> S1["run_loom_agent_streaming()"]
        S1 --> S2["详见 流式Agent响应流程"]
    end

    style cmd fill:#FFF3E0,stroke:#E65100
    style agent_run fill:#E8F5E9,stroke:#2E7D32
```

---

## 4. 流式 Agent 响应流程

```mermaid
flowchart TD
    A["run_loom_agent_streaming()"] --> B["mpsc::channel&lt;StreamCommand&gt;(100)"]
    B --> C["spawn stream_message_handler()"]
    B --> D["创建 StreamEventMapper"]

    D --> E["loom::run_agent_with_options()"]
    E --> F["Loom Agent 内部执行"]

    subgraph producer["事件生产 (Mapper)"]
        F --> G["AnyStreamEvent"]
        G --> H["StreamEventMapper"]
        H --> I{"映射为 StreamCommand"}

        I -->|"ThinkStart"| J1["StartThink (Critical)"]
        I -->|"ThinkDelta"| J2["ThinkContent (BestEffort)"]
        I -->|"ActStart"| J3["StartAct (Critical)"]
        I -->|"ActDelta"| J4["ActContent (BestEffort)"]
        I -->|"ToolCallStart"| J5["ToolStart (Critical)"]
        I -->|"ToolCallEnd"| J6["ToolEnd (Critical)"]
    end

    subgraph backpressure["背压策略"]
        J1 & J3 & J5 & J6 --> K1["Critical → tx.send().await<br/>阻塞等待"]
        J2 & J4 --> K2["BestEffort → tx.try_send()<br/>满则丢弃"]
    end

    K1 & K2 -->|"channel"| L

    subgraph consumer["事件消费 (Handler)"]
        C --> L["stream_message_handler_with_context()"]
        L --> M{"select!"}
        M -->|"rx.recv()"| N["process_command()"]
        M -->|"edit_throttle.tick()"| O["flush_pending_edit()"]

        N --> N1["更新 MessageState"]
        N1 --> N2{"需要刷新?"}
        N2 -->|Yes| P["format_current_display()"]
        N2 -->|No| M

        O --> P
        P --> P1["sender.edit_formatted()"]
        P1 -->|"失败"| P2["retry with backoff"]
        P1 -->|"成功"| M
    end

    F -->|"完成"| Q["tx.send(Flush)"]
    Q --> R["handler_task.await"]
    R --> S["返回 final_text"]

    style producer fill:#E3F2FD,stroke:#1565C0
    style backpressure fill:#FFF8E1,stroke:#F57F17
    style consumer fill:#FCE4EC,stroke:#C62828
```

---

## 5. StreamCommand 状态转换

```mermaid
stateDiagram-v2
    [*] --> Idle : 消息到达

    Idle --> Thinking : StartThink
    Thinking --> Thinking : ThinkContent
    Thinking --> Acting : StartAct

    Acting --> Acting : ActContent
    Acting --> ToolExecuting : ToolStart
    ToolExecuting --> Acting : ToolEnd
    Acting --> Thinking : StartThink (新一轮)

    ToolExecuting --> ToolExecuting : ToolStart (嵌套)
    Acting --> Completed : Flush
    Completed --> [*]

    note right of Thinking
        渲染:
        💭 Thinking... (n)
        <累积文本>
    end note

    note right of Acting
        渲染:
        ⚡ Acting... (n)
        <累积文本>
        [Tool blocks]
    end note

    note right of ToolExecuting
        渲染:
        🔧 Tool: <name>
        Running... / Result
    end note
```

---

## 6. 依赖注入与测试切换

```mermaid
classDiagram
    class HandlerDeps {
        +bot: Bot
        +settings: Arc~Settings~
        +bot_username: Arc~String~
        +run_registry: Arc~ChatRunRegistry~
        +agent_runner: Box~AgentRunner~
        +sender: Arc~MessageSender~
        +session_manager: Arc~SessionManager~
        +file_downloader: Arc~FileDownloader~
        +model_service: Arc~ModelSelectionService~
        +production() HandlerDeps
        +mock() HandlerDeps
    }

    class AgentRunner {
        <<trait>>
        +run(prompt, chat_id, context) Result~String~
    }
    class LoomAgentRunner {
        +bot: Bot
        +settings: Settings
        +run() Result~String~
    }
    class MockAgentRunner {
        +run() Result~String~
    }

    class MessageSender {
        <<trait>>
        +send_text_returning_id() Result~i32~
        +edit_text() Result~()~
        +send_formatted() Result~i32~
        +edit_formatted() Result~()~
        +delete_message() Result~()~
        +set_reaction() Result~()~
    }
    class TeloxideSender {
        +bot: Bot
    }
    class MockSender {
        +messages: RwLock~Vec~
    }

    class SessionManager {
        <<trait>>
        +reset(thread_id) Result~usize~
        +exists(thread_id) Result~bool~
    }
    class SqliteSessionManager {
        +reset() Result~usize~
        +exists() Result~bool~
    }

    class FileDownloader {
        <<trait>>
        +download_photo() Result~FileMetadata~
        +download_video() Result~FileMetadata~
        +download_document() Result~FileMetadata~
    }
    class TeloxideDownloader {
        +bot: Bot
    }
    class MockFileDownloader {
        +downloads: RwLock~Vec~
    }

    HandlerDeps --> AgentRunner : depends on
    HandlerDeps --> MessageSender : depends on
    HandlerDeps --> SessionManager : depends on
    HandlerDeps --> FileDownloader : depends on

    AgentRunner <|.. LoomAgentRunner : prod
    AgentRunner <|.. MockAgentRunner : test
    MessageSender <|.. TeloxideSender : prod
    MessageSender <|.. MockSender : test
    SessionManager <|.. SqliteSessionManager : prod
    FileDownloader <|.. TeloxideDownloader : prod
    FileDownloader <|.. MockFileDownloader : test
```

---

## 7. 命令分发流程

```mermaid
flowchart LR
    A["用户消息"] --> B["CommandDispatcher"]
    B --> C["commands[0]: ResetCommand"]
    C --> D{"/reset?"}
    D -->|Yes| E["SessionManager::reset()"]
    D -->|No| F["commands[1]: StatusCommand"]
    F --> G{"/status?"}
    G -->|Yes| H["返回状态信息"]
    G -->|No| I["commands[2]: ModelCommand"]
    I --> J{"/model?"}
    J -->|Yes| K["ModelSelectionService"]
    K --> K1["search / select / clear"]
    J -->|No| L["无匹配<br/>进入 mention gate"]

    style B fill:#E8EAF6,stroke:#283593
    style E fill:#C8E6C9,stroke:#2E7D32
    style H fill:#C8E6C9,stroke:#2E7D32
    style K1 fill:#C8E6C9,stroke:#2E7D32
    style L fill:#FFECB3,stroke:#FF6F00
```

---

## 8. 模型选择系统

```mermaid
flowchart TD
    A["/model 命令输入"] --> B["try_handle_model_command_input()"]
    B --> C{"解析子命令"}

    C -->|"/model"| D["搜索模式<br/>ModelCatalog::search()"]
    C -->|"/model 3"| E["选择模式<br/>save_selected_model()"]
    C -->|"/model reset"| F["清除模式<br/>clear_selected_model()"]
    C -->|其他| G["回显当前模型"]

    D --> H["StaticModelCatalog"]
    H --> H1["模糊匹配模型列表"]
    H1 --> H2["返回 ModelSearchResult<br/>分页展示"]

    E --> I["SqliteModelSelectionStore"]
    I --> I1["INSERT/UPDATE<br/>model_selection 表"]

    F --> I
    F --> I2["DELETE<br/>model_selection 表"]

    subgraph stores["存储层"]
        I
        H
        J["InMemorySearchSessionStore<br/>跟踪翻页状态"]
    end

    D --> J
    E --> J

    style stores fill:#F3E5F5,stroke:#6A1B9A
```

---

## 9. 配置加载流程

```mermaid
flowchart TD
    A["load_config()"] --> B["loader.rs"]
    B --> C["查找配置文件"]
    C --> C1{"$LOOM_HOME/<br/>telegram-bot.toml?"}
    C1 -->|Yes| D["读取文件"]
    C1 -->|No| C2{"~/.loom/<br/>telegram-bot.toml?"}
    C2 -->|Yes| D
    C2 -->|No| C3{"./telegram-bot.toml?"}
    C3 -->|Yes| D
    C3 -->|No| C4["返回 ConfigError::Io"]

    D --> E["interpolate_env_vars()"]
    E --> E1{"扫描 ${VAR} 模式"}
    E1 --> F["std::env::var(VAR)"]
    F --> F1{"找到?"}
    F1 -->|Yes| G["替换为值"]
    F1 -->|No| H["ConfigError::EnvVarNotFound"]
    G --> I["toml::from_str()"]
    I --> I1{"解析成功?"}
    I1 -->|Yes| J["验证配置"]
    I1 -->|No| K["ConfigError::Toml"]
    J --> J1{"有 bot 配置?"}
    J1 -->|No| J2["ConfigError::NoBots"]
    J1 -->|Yes| J3{"每个 bot 有 token?"}
    J3 -->|No| J4["ConfigError::MissingToken"]
    J3 -->|Yes| L["返回 TelegramBotConfig"]

    style E fill:#E0F7FA,stroke:#00695C
    style J fill:#FFF3E0,stroke:#E65100
```

---

## 10. 错误重试策略

```mermaid
flowchart TD
    A["Telegram API 调用"] --> B{"成功?"}
    B -->|Yes| C["返回结果"]
    B -->|No| D["classify_error()"]

    D --> E{"RetryKind?"}
    E -->|"RetryAfter(secs)"| F["RateLimited"]
    E -->|"Network(_)"| G["Transient"]
    E -->|"其他"| H["Fatal<br/>立即返回错误"]

    F --> I["sleep(RetryAfter)"]
    G --> J["计算退避延迟"]
    J --> J1["delay = BASE × 2^attempt"]
    J1 --> J2["delay = min(delay, MAX_DELAY)"]
    J2 --> J3["delay × (1 ± 25% jitter)"]
    J3 --> K["sleep(delay)"]

    I --> L{"attempt < MAX_RETRIES?"}
    K --> L
    L -->|Yes| M["重试 API 调用"]
    M --> B
    L -->|No| N["返回错误"]

    style F fill:#FFCDD2,stroke:#C62828
    style G fill:#FFF9C4,stroke:#F57F17
    style H fill:#E0E0E0,stroke:#424242
```
