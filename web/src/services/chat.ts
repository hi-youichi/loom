type ChatReply = {
  content: string
}

const demoReplies = [
  '已收到。我可以继续帮你整理成下一步动作。',
  '这条消息我收到了，要不要我继续展开成更完整的方案？',
  '如果这是聊天输入区，下一步通常会接历史消息和接口返回。',
]

export async function sendMessage(content: string): Promise<ChatReply> {
  await new Promise((resolve) => window.setTimeout(resolve, 700))

  const normalized = content.trim().toLowerCase()
  if (normalized.includes('error')) {
    throw new Error('mock request failed')
  }

  const reply =
    normalized.includes('方案')
      ? '可以，当前发送组件已经具备继续扩展成完整聊天页的基础。'
      : demoReplies[Math.floor(Math.random() * demoReplies.length)]

  return {
    content: reply,
  }
}
