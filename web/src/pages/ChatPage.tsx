import { useMemo, useState, useEffect } from 'react'

import { MessageComposer } from '../components/MessageComposer'
import { ThinkIndicator } from '../components/ThinkIndicator'
import { ToolBlockView } from '../components/ToolBlockView'
import { sendMessage } from '../services/chat'
import { useModels } from '../hooks/useModels'
import type { ToolBlock, ToolStatus } from '../types/chat'

const THREAD_STORAGE_KEY = 'loom-web-thread-id'

function getOrCreateThreadId() {
  const existing = window.localStorage.getItem(THREAD_STORAGE_KEY)
  if (existing) {
    return existing
  }

  const nextThreadId = crypto.randomUUID()
  window.localStorage.setItem(THREAD_STORAGE_KEY, nextThreadId)
  return nextThreadId
}

function formatTime(timestamp: string) {
  return new Intl.DateTimeFormat('en', {
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(timestamp))
}

type UserEvent = {
  type: 'user'
  id: string
  createdAt: string
  text: string
}

type AssistantTextEvent = {
  type: 'assistant_text'
  id: string
  createdAt: string
  text: string
}

type AssistantThinkingEvent = {
  type: 'assistant_thinking'
  id: string
  lines: string[]
  active: boolean
}

type ToolBlockEvent = {
  type: 'tool_block'
  tool: ToolBlock
}

type StreamEvent = UserEvent | AssistantTextEvent | AssistantThinkingEvent | ToolBlockEvent

function formatThinkLine(event: Record<string, unknown>) {
  switch (event.type) {
    case 'run_start':
      return `run_start${typeof event.agent === 'string' ? ` · ${event.agent}` : ''}`
    case 'node_enter':
    case 'node_exit':
    case 'usage':
    case 'values':
    case 'updates':
    case 'checkpoint':
    case 'message_chunk':
    case 'thought_chunk':
    case 'tool_call':
    case 'tool_start':
    case 'tool_output':
    case 'tool_end':
      return null
    default:
      return typeof event.type === 'string' ? event.type : null
  }
}

function createToolBlock(callId: string, name: string, args: string): ToolBlock {
  return {
    id: crypto.randomUUID(),
    type: 'tool',
    callId,
    name,
    status: 'queued' as ToolStatus,
    argumentsText: args,
    outputText: '',
    resultText: '',
    isError: false,
  }
}

export function ChatPage() {
  const threadId = useMemo(() => getOrCreateThreadId(), [])
  const [streamEvents, setStreamEvents] = useState<StreamEvent[]>([])
  const [sending, setSending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const { models } = useModels()
  const [selectedModel, setSelectedModel] = useState('')

  useEffect(() => {
    if (selectedModel || models.length === 0) return
    const fallback = 'claude-3-5-sonnet'
    const match = models.find(m => m.id.includes(fallback) || m.name.includes(fallback))
    setSelectedModel(match?.id || models[0].id)
  }, [models, selectedModel])

  const handleModelChange = (model: string) => {
    if (import.meta.env.DEV) {
      console.log('🔄 Model changed to:', model);
    }
    setSelectedModel(model)
  }

  const handleSend = async (text: string) => {
    const createdAt = new Date().toISOString()
    const userId = crypto.randomUUID()
    const thinkingId = crypto.randomUUID()
    const textId = crypto.randomUUID()

    // Validate model selection
    if (!selectedModel) {
      setError('Please select a model before sending a message')
      return
    }
    
    if (import.meta.env.DEV) {
      console.log('📤 Preparing to send message with model:', selectedModel);
      console.log('📤 Model details:', models.find(m => m.id === selectedModel));
    }

    // Add user message and assistant placeholder events
    setStreamEvents((current) => [
      ...current,
      { type: 'user', id: userId, createdAt, text },
      { type: 'assistant_thinking', id: thinkingId, lines: [], active: true },
      { type: 'assistant_text', id: textId, createdAt, text: '' },
    ])
    setSending(true)
    setError(null)

    try {
      const reply = await sendMessage(text, {
        threadId,
        model: selectedModel,
        onEvent: (event) => {
          const evt = event as Record<string, unknown>
          
          // Handle tool events separately
          if (evt.type === 'tool_call') {
            const callId = typeof evt.id === 'string' ? evt.id : crypto.randomUUID()
            const name = typeof evt.name === 'string' ? evt.name : 'unknown'
            const args = typeof evt.input === 'string' 
              ? evt.input 
              : JSON.stringify(evt.input, null, 2)
            
            const toolBlock = createToolBlock(callId, name, args)
            setStreamEvents((current) => [...current, { type: 'tool_block', tool: toolBlock }])
            return
          }
          
          if (evt.type === 'tool_start') {
            const callId = typeof evt.id === 'string' ? evt.id : ''
            setStreamEvents((current) =>
              current.map((e) => {
                if (e.type === 'tool_block' && e.tool.callId === callId) {
                  return { ...e, tool: { ...e.tool, status: 'running' as ToolStatus } }
                }
                return e
              }),
            )
            return
          }
          
          if (evt.type === 'tool_output') {
            const callId = typeof evt.id === 'string' ? evt.id : ''
            const content = typeof evt.content === 'string' ? evt.content : ''
            setStreamEvents((current) =>
              current.map((e) => {
                if (e.type === 'tool_block' && e.tool.callId === callId) {
                  return { 
                    ...e, 
                    tool: { 
                      ...e.tool, 
                      outputText: e.tool.outputText + content 
                    } 
                  }
                }
                return e
              }),
            )
            return
          }
          
          if (evt.type === 'tool_end') {
            const callId = typeof evt.id === 'string' ? evt.id : ''
            const result = typeof evt.result === 'string' 
              ? evt.result 
              : JSON.stringify(evt.result, null, 2)
            const isError = evt.error !== undefined && evt.error !== null
            
            setStreamEvents((current) =>
              current.map((e) => {
                if (e.type === 'tool_block' && e.tool.callId === callId) {
                  return { 
                    ...e, 
                    tool: { 
                      ...e.tool, 
                      status: (isError ? 'error' : 'done') as ToolStatus,
                      resultText: result,
                      isError
                    } 
                  }
                }
                return e
              }),
            )
            return
          }

          // thought_chunk: append reasoning content to thinking lines
          if (evt.type === 'thought_chunk' && typeof evt.content === 'string') {
            setStreamEvents((current) =>
              current.map((e) =>
                e.type === 'assistant_thinking' && e.id === thinkingId
                  ? { ...e, lines: (e.lines.join('\n') + evt.content).split('\n') }
                  : e,
              ),
            )
            return
          }

          // Handle other thinking events (run_start, node_enter, etc.)
          const line = formatThinkLine(evt)
          if (!line) {
            return
          }

          setStreamEvents((current) =>
            current.map((e) =>
              e.type === 'assistant_thinking' && e.id === thinkingId
                ? { ...e, lines: [...e.lines, line] }
                : e,
            ),
          )
        },
        onChunk: (chunk) => {
          setStreamEvents((current) =>
            current.map((e) =>
              e.type === 'assistant_text' && e.id === textId
                ? { ...e, text: e.text + chunk }
                : e,
            ),
          )
        },
      })

      // Mark thinking as inactive and finalize text
      setStreamEvents((current) =>
        current.map((e) => {
          if (e.type === 'assistant_thinking' && e.id === thinkingId) {
            return { ...e, active: false }
          }
          if (e.type === 'assistant_text' && e.id === textId) {
            return { ...e, text: reply.content || e.text }
          }
          return e
        }),
      )
    } catch (caughtError) {
      const nextError =
        caughtError instanceof Error
          ? caughtError.message
          : 'Request failed. Check whether `loom serve` is running.'

      // Remove assistant events on error
      setStreamEvents((current) =>
        current.filter(
          (e) =>
            !(e.type === 'assistant_thinking' && e.id === thinkingId) &&
            !(e.type === 'assistant_text' && e.id === textId),
        ),
      )
      setError(nextError)
      throw caughtError
    } finally {
      setSending(false)
    }
  }

  return (
    <main className="shell">
      <section className="chat-panel" aria-label="Chat demo">
        <div className="message-list" role="log" aria-live="polite">
          {streamEvents.map((event) => {
            if (event.type === 'user') {
              return (
                <article key={event.id} className="message message--user" aria-label="user message">
                  <div className="message__meta">
                    <time dateTime={event.createdAt}>{formatTime(event.createdAt)}</time>
                  </div>
                  <p className="message__content">{event.text}</p>
                </article>
              )
            }

            if (event.type === 'assistant_thinking') {
              return <ThinkIndicator key={event.id} lines={event.lines} active={event.active} />
            }

            if (event.type === 'assistant_text') {
              if (!event.text) return null
              return (
                <article
                  key={event.id}
                  className="message message--assistant"
                  aria-label="assistant message"
                >
                  <div className="message__meta">
                    <time dateTime={event.createdAt}>{formatTime(event.createdAt)}</time>
                  </div>
                  <p className="message__content">{event.text}</p>
                </article>
              )
            }

            if (event.type === 'tool_block') {
              return <ToolBlockView key={event.tool.id} tool={event.tool} />
            }

            return null
          })}
        </div>

        {error ? <p className="chat-panel__error">{error}</p> : null}

        <MessageComposer 
          disabled={sending} 
          onSend={handleSend}
          selectedModel={selectedModel}
          onModelChange={handleModelChange}
        />
      </section>
    </main>
  )
}
