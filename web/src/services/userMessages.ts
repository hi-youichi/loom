import type { LoomServerMessage } from '../types/protocol/loom'
import { getConnection } from './connection'

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
  threadId: string,
  options?: { before?: number; limit?: number }
): Promise<UserMessageItem[]> {
  const resp = await getConnection().request({
    type: 'user_messages',
    id: crypto.randomUUID(),
    thread_id: threadId,
    before: options?.before,
    limit: options?.limit,
  })

  const msg = resp as UserMessagesResponse
  return msg.messages ?? []
}
