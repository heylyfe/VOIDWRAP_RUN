// WebSocket transport. Binary frames only, protocol-encoded both ways.
import { decodeServerMsg, encodeClientMsg, type ClientMsg, type ServerMsg } from "@/protocol/generated";

/**
 * Where the game server is:
 * 1. `?server=wss://host/ws` in the page URL (handy for testing a remote server),
 * 2. NEXT_PUBLIC_VOIDWRAP_WS at build time,
 * 3. the web dev server (port 3000) -> the game server on port 7878 of the same machine,
 * 4. otherwise the page was served BY the game server: connect back to the same address.
 */
/** HTTP origin of the game server for API calls: the dev server (port 3000) talks to port 7878,
 *  otherwise the page itself was served by the game server (empty string = same origin). */
export function httpBaseUrl(): string {
  if (typeof window === "undefined") return "";
  const { protocol, hostname, port } = window.location;
  return port === "3000" ? `${protocol}//${hostname}:7878` : "";
}

export function defaultServerUrl(): string {
  if (typeof window === "undefined") return "ws://localhost:7878/ws";
  const fromQuery = new URLSearchParams(window.location.search).get("server");
  if (fromQuery && /^wss?:\/\//.test(fromQuery)) return fromQuery;
  const configured = process.env.NEXT_PUBLIC_VOIDWRAP_WS;
  if (configured) return configured;
  const { protocol, hostname, host, port } = window.location;
  if (port === "3000") return `ws://${hostname}:7878/ws`;
  return `${protocol === "https:" ? "wss" : "ws"}://${host}/ws`;
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
