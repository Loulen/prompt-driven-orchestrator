import { useCallback, useEffect, useRef, useState } from "react";
import type { WsMessage } from "../types";

export type ConnectionStatus = "connected" | "reconnecting" | "disconnected";

const RECONNECT_INTERVAL = 3000;

export function useDaemonSocket() {
  const [status, setStatus] = useState<ConnectionStatus>("disconnected");
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const listenersRef = useRef<Set<(msg: WsMessage) => void>>(new Set());

  const subscribe = useCallback((listener: (msg: WsMessage) => void) => {
    listenersRef.current.add(listener);
    return () => {
      listenersRef.current.delete(listener);
    };
  }, []);

  useEffect(() => {
    function connect() {
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const url = `${protocol}//${window.location.host}/ws`;

      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.addEventListener("open", () => {
        setStatus("connected");
      });

      ws.addEventListener("message", (e) => {
        try {
          const msg: WsMessage = JSON.parse(e.data);
          if (msg.type === "event" || msg.type === "pipeline_changed") {
            for (const listener of listenersRef.current) {
              listener(msg);
            }
          }
        } catch {
          // ignore malformed messages
        }
      });

      ws.addEventListener("close", () => {
        setStatus("reconnecting");
        wsRef.current = null;
        reconnectTimer.current = setTimeout(connect, RECONNECT_INTERVAL);
      });

      ws.addEventListener("error", () => {
        ws.close();
      });
    }

    connect();
    return () => {
      clearTimeout(reconnectTimer.current);
      wsRef.current?.close();
    };
  }, []);

  return { status, subscribe };
}
