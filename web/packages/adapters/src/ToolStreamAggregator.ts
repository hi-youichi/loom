import type {
  LoomToolCallChunkEvent,
  LoomToolCallEvent,
  LoomToolEndEvent,
  LoomToolEvent,
  LoomToolOutputEvent,
  LoomToolStartEvent,
  LoomToolStatus,
} from '@loom/protocol'

export type ToolStreamState = {
  callId: string
  name: string
  status: LoomToolStatus
  argumentsText: string
  outputText: string
  resultText: string
  isError: boolean
}

function formatValue(value: unknown): string {
  if (typeof value === 'string') {
    return value
  }

  if (value === undefined || value === null) {
    return ''
  }

  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

function getFallbackKey(event: LoomToolEvent): string {
  const name = typeof event.name === 'string' && event.name.length > 0 ? event.name : 'unknown'
  return `fallback:${name}`
}

function createEmptyToolState(callId: string, name = 'unknown'): ToolStreamState {
  return {
    callId,
    name,
    status: 'queued',
    argumentsText: '',
    outputText: '',
    resultText: '',
    isError: false,
  }
}

export class ToolStreamAggregator {
  private readonly tools = new Map<string, ToolStreamState>()

  reset() {
    this.tools.clear()
  }

  apply(event: LoomToolEvent): ToolStreamState {
    const key = event.call_id ?? getFallbackKey(event)
    const current = this.tools.get(key) ?? createEmptyToolState(key)
    const next = this.reduce(current, event)
    this.tools.set(key, next)
    return next
  }

  snapshot(): ToolStreamState[] {
    return Array.from(this.tools.values())
  }

  private reduce(current: ToolStreamState, event: LoomToolEvent): ToolStreamState {
    switch (event.type) {
      case 'tool_call_chunk':
        return this.applyToolCallChunk(current, event)
      case 'tool_call':
        return this.applyToolCall(current, event)
      case 'tool_start':
        return this.applyToolStart(current, event)
      case 'tool_output':
        return this.applyToolOutput(current, event)
      case 'tool_end':
        return this.applyToolEnd(current, event)
      default:
        return current
    }
  }

  private applyToolCallChunk(
    current: ToolStreamState,
    event: LoomToolCallChunkEvent,
  ): ToolStreamState {
    return {
      ...current,
      name: event.name || current.name,
      status: 'queued',
      argumentsText: `${current.argumentsText}${event.arguments_delta}`,
    }
  }

  private applyToolCall(current: ToolStreamState, event: LoomToolCallEvent): ToolStreamState {
    return {
      ...current,
      name: event.name || current.name,
      status: 'queued',
      argumentsText: formatValue(event.arguments),
    }
  }

  private applyToolStart(current: ToolStreamState, event: LoomToolStartEvent): ToolStreamState {
    return {
      ...current,
      name: event.name || current.name,
      status: 'running',
    }
  }

  private applyToolOutput(current: ToolStreamState, event: LoomToolOutputEvent): ToolStreamState {
    return {
      ...current,
      name: event.name || current.name,
      status: current.status === 'queued' ? 'running' : current.status,
      outputText: `${current.outputText}${event.content}`,
    }
  }

  private applyToolEnd(current: ToolStreamState, event: LoomToolEndEvent): ToolStreamState {
    return {
      ...current,
      name: event.name || current.name,
      status: event.is_error ? 'error' : 'done',
      resultText: formatValue(event.result),
      isError: event.is_error,
    }
  }
}
