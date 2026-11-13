// WebSocket transport. Binary frames only, protocol-encoded both ways.
import { decodeServerMsg, encodeClientMsg, type ClientMsg, type ServerMsg } from "@/protocol/generated";

export function defaultServerUrl(): string {
  const configured = process.env.NEXT_PUBLIC_VOIDWRAP_WS;
  if (configured) return configured;
  const host = typeof window === "undefined" ? "localhost" : window.location.hostname;
  return `ws://${host}:7878/ws`;
}

export interface ConnectionHandlers {
  onOpen(): void;
  onMessage(msg: ServerMsg): void;
  onClose(reason: string): void;
}

export class Connection {
  private ws: WebSocket;
  private closed = false;

  constructor(url: string, private readonly handlers: ConnectionHandlers) {
    this.ws = new WebSocket(url);
    this.ws.binaryType = "arraybuffer";
    this.ws.onopen = () => handlers.onOpen();
    this.ws.onmessage = (ev: MessageEvent) => {
      if (!(ev.data instanceof ArrayBuffer)) return;
      let msg: ServerMsg;
      try {
        msg = decodeServerMsg(new Uint8Array(ev.data));
      } catch (e) {
        console.warn("dropping undecodable server message", e);
        return;
      }
      handlers.onMessage(msg);
    };
    this.ws.onclose = (ev) => this.finish(ev.reason || `connection closed (${ev.code})`);
    this.ws.onerror = () => this.finish("could not reach the game server");
  }

  private finish(reason: string) {
    if (this.closed) return;
    this.closed = true;
    this.handlers.onClose(reason);
  }

  send(msg: ClientMsg): void {
    if (this.ws.readyState === WebSocket.OPEN) this.ws.send(encodeClientMsg(msg));
  }

  close(): void {
    this.closed = true;
    this.ws.close();
  }
}
