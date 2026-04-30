import { getConnection } from '@loom/ws-client'

export type UserMessageItem = {
  role: string
  content: string
}

export type UserMessagesResponse = {
  type: 'user_messages'
  id: string
  thread_id: string
  messages: UserMessageItem[]
  has_more: boolean | null
}

export async function getUserMessages(
  sessionId: string,
  options?: { before?: number; limit?: number }
): Promise<UserMessageItem[]> {
  const resp = await getConnection().request({
    type: 'user_messages',
    id: crypto.randomUUID(),
    thread_id: sessionId,
    before: options?.before,
    limit: options?.limit,
  })

  const msg = resp as UserMessagesResponse
  return msg.messages ?? []
}
