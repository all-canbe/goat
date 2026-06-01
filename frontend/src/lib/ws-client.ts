import type { WSMessage, ConnectionStatus } from '@/types'

type MessageHandler = (msg: WSMessage) => void
type StatusHandler = (status: ConnectionStatus) => void

export class WSClient {
  private ws: WebSocket | null = null
  private url: string
  private handlers: Map<string, Set<MessageHandler>> = new Map()
  private statusHandlers: Set<StatusHandler> = new Set()
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private _status: ConnectionStatus = 'disconnected'
  private pendingQueue: Array<{ type: string; payload: Record<string, unknown> }> = []

  constructor(url: string = '') {
    this.url = url || this.detectUrl()
  }

  get status(): ConnectionStatus {
    return this._status
  }

  private detectUrl(): string {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    return `${protocol}//${window.location.host}/ws`
  }

  connect(): void {
    if (this.ws?.readyState === WebSocket.OPEN) return
    this.setStatus('connecting')

    this.ws = new WebSocket(this.url)
    this.ws.onopen = () => {
      this.setStatus('connected')
      this.flushQueue()
    }
    this.ws.onclose = () => {
      this.setStatus('disconnected')
      this.scheduleReconnect()
    }
    this.ws.onerror = () => {
      this.setStatus('disconnected')
    }
    this.ws.onmessage = (event) => {
      try {
        const msg: WSMessage = JSON.parse(event.data)
        const handlers = this.handlers.get(msg.type)
        if (handlers) {
          handlers.forEach((h) => h(msg))
        }
      } catch {
        // ignore malformed messages
      }
    }
  }

  private flushQueue(): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return
    const queue = this.pendingQueue
    this.pendingQueue = []
    for (const msg of queue) {
      this.ws.send(JSON.stringify({ type: msg.type, payload: msg.payload }))
    }
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null
      this.connect()
    }, 3000)
  }

  disconnect(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    this.ws?.close()
    this.ws = null
    this.setStatus('disconnected')
    this.pendingQueue = []
  }

  send(type: string, payload: Record<string, unknown> = {}): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ type, payload }))
    } else if (this.ws?.readyState === WebSocket.CONNECTING) {
      this.pendingQueue.push({ type, payload })
    } else {
      this.pendingQueue.push({ type, payload })
      this.connect()
    }
  }

  on(type: string, handler: MessageHandler): () => void {
    if (!this.handlers.has(type)) {
      this.handlers.set(type, new Set())
    }
    this.handlers.get(type)!.add(handler)
    return () => {
      this.handlers.get(type)?.delete(handler)
    }
  }

  onStatusChange(handler: StatusHandler): () => void {
    this.statusHandlers.add(handler)
    return () => {
      this.statusHandlers.delete(handler)
    }
  }

  private setStatus(status: ConnectionStatus): void {
    this._status = status
    this.statusHandlers.forEach((h) => h(status))
  }
}