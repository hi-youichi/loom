@home
Feature: 首页
  用户打开 anureo 首页并开始使用：
  首屏正常加载、新建会话发送消息、刷新后状态恢复、
  无会话时有友好空状态、模型不可用时有友好报错而非原始错误。

  Background:
    Given 我已进入应用

  Scenario: SMK-001 首屏加载显示侧栏而非白屏
    Then 侧栏可见
    And 侧栏包含会话操作按钮
    And 无 JavaScript 报错
    And 控制台无错误输出
    And 无网络请求失败
    And 网络无异常响应

  @chat-contract
  Scenario: SMK-004 新建会话并发送消息
    When 我点击新建会话按钮
    And 我发送消息 "Hello, what can you do?"
    Then 用户消息 "Hello, what can you do?" 出现在聊天区
    And 我等待助手回复完成
    And 助手回复出现在聊天区
    And 助手回复内容不为空
    When 我刷新页面
    Then 用户消息 "Hello, what can you do?" 出现在聊天区
    And 聊天区包含助手消息
    And 助手回复内容不为空
    And 无 JavaScript 报错

  Scenario: SMK-005 刷新页面后会话列表正确恢复
    When 我刷新页面
    Then 侧栏可见
    And 侧栏包含会话操作按钮
    And 无 JavaScript 报错

  Scenario: ERR-001 零会话时显示友好空状态提示
    Then 侧栏可见
    And 页面显示友好空状态提示或侧栏内容

  Scenario: ERR-003 模型不可用时显示友好错误提示
    When 我点击新建会话按钮
    And 我在输入框输入消息 "Hello, what can you do?"
    And 我发送消息
    Then 页面显示友好错误提示或无原始错误泄露
