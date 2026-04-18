import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Session } from "../App";

// ttyd wire protocol (subprotocol "tty"):
//   first byte of each message is a 1-char command.
//   server → client:
//     '0' = output bytes (UTF-8)
//     '1' = set window title
//     '2' = set preferences (JSON)
//     '3' = server ping (binary message)
//   client → server:
//     '0' = user input bytes
//     '1' = resize event, JSON payload { "columns": N, "rows": N }
//     '2' = pause (unused)
//     '3' = resume (unused)
//   the very first client message must be JSON { "AuthToken": "<Basic base64>" }
//   (not prefixed by a command byte).

const CMD_INPUT = "0";
const CMD_RESIZE = "1";

export default function SessionView({
  session,
  onStopped,
}: {
  session: Session;
  onStopped: () => void;
}) {
  const termRef = useRef<HTMLDivElement>(null);
  const [elapsed, setElapsed] = useState(0);
  const [connectionState, setConnectionState] = useState<
    "connecting" | "connected" | "closed" | "error"
  >("connecting");
  const [stopping, setStopping] = useState(false);

  useEffect(() => {
    if (!termRef.current) return;

    const term = new Terminal({
      convertEol: false,
      cursorBlink: true,
      fontFamily: 'Cascadia Code, Consolas, "Courier New", monospace',
      fontSize: 14,
      theme: { background: "#1a1b26", foreground: "#c0caf5" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(termRef.current);
    fit.fit();

    const decoder = new TextDecoder();

    const wsUrl = `wss://${session.hostname}/ws`;
    const ws = new WebSocket(wsUrl, ["tty"]);
    ws.binaryType = "arraybuffer";

    const sendAuth = () => {
      ws.send(
        JSON.stringify({
          AuthToken: btoa(`toddler:${session.password}`),
        }),
      );
    };

    const sendResize = () => {
      try {
        ws.send(
          CMD_RESIZE +
            JSON.stringify({ columns: term.cols, rows: term.rows }),
        );
      } catch {}
    };

    ws.onopen = () => {
      setConnectionState("connected");
      sendAuth();
      sendResize();
    };

    ws.onclose = () => setConnectionState("closed");
    ws.onerror = () => setConnectionState("error");

    ws.onmessage = (ev) => {
      const buf =
        ev.data instanceof ArrayBuffer ? ev.data : null;
      if (!buf) return;
      const view = new Uint8Array(buf);
      if (view.length === 0) return;
      const cmd = String.fromCharCode(view[0]);
      const payload = view.subarray(1);
      if (cmd === "0") {
        term.write(decoder.decode(payload));
      }
      // ignore '1' (title), '2' (prefs), '3' (ping)
    };

    const disposeData = term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(CMD_INPUT + data);
      }
    });

    const disposeResize = term.onResize(() => {
      if (ws.readyState === WebSocket.OPEN) sendResize();
    });

    const onWindowResize = () => {
      try {
        fit.fit();
      } catch {}
    };
    window.addEventListener("resize", onWindowResize);

    (window as any).__toddler_ws = ws;

    return () => {
      window.removeEventListener("resize", onWindowResize);
      disposeData.dispose();
      disposeResize.dispose();
      try {
        ws.close();
      } catch {}
      term.dispose();
    };
  }, [session.hostname, session.password]);

  useEffect(() => {
    const id = setInterval(() => {
      setElapsed(Math.floor(Date.now() / 1000 - session.started_at));
    }, 1000);
    return () => clearInterval(id);
  }, [session.started_at]);

  async function stop() {
    setStopping(true);
    try {
      const ws = (window as any).__toddler_ws as WebSocket | undefined;
      try {
        ws?.close();
      } catch {}
      await invoke("stop_session");
    } finally {
      onStopped();
    }
  }

  const mins = Math.floor(elapsed / 60);
  const secs = elapsed % 60;

  return (
    <div className="session-root">
      <div className="session-bar">
        <div>
          <strong>{session.repo}</strong>{" "}
          <span className="muted">
            · {session.region} · {mins}m {secs.toString().padStart(2, "0")}s
          </span>
          <span className={`dot ${connectionState}`} />
        </div>
        <button className="danger" onClick={stop} disabled={stopping}>
          {stopping ? "Stopping…" : "Stop and discard"}
        </button>
      </div>
      <div className="terminal-host" ref={termRef} />
    </div>
  );
}
