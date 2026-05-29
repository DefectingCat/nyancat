import "@xterm/xterm/css/xterm.css";
import "./style.css";
import { WsClient, createTerminal } from "./ws-client.ts";
import { ConnectionState } from "./types.ts";

// ---------- DOM 元素 ----------

const app = document.getElementById("app");
if (!app) {
    throw new Error("Could not find #app element");
}

const statusEl = document.getElementById("connection-status");

// ---------- 初始化终端 ----------

const { term, fitAddon } = createTerminal(app);

// ---------- 连接状态 UI ----------

function updateStatus(state: ConnectionState, detail?: string): void {
    if (!statusEl) return;

    const stateMap: Record<ConnectionState, { text: string; cls: string }> = {
        [ConnectionState.Connecting]: { text: "Connecting...", cls: "status-connecting" },
        [ConnectionState.Handshaking]: { text: "Handshaking...", cls: "status-connecting" },
        [ConnectionState.Connected]: { text: "Connected", cls: "status-connected" },
        [ConnectionState.Disconnected]: { text: "Disconnected", cls: "status-disconnected" },
        [ConnectionState.Reconnecting]: { text: "Reconnecting...", cls: "status-reconnecting" },
    };

    const info = stateMap[state];
    statusEl.className = info.cls;
    statusEl.textContent = detail ? `${info.text} (${detail})` : info.text;

    // 非连接状态时在终端显示提示
    if (state === ConnectionState.Disconnected) {
        term.writeln("\r\n\x1b[31m[Disconnected]\x1b[0m");
    } else if (state === ConnectionState.Reconnecting && detail) {
        term.writeln(`\r\n\x1b[33m[${detail}]\x1b[0m`);
    }
}

// ---------- WebSocket 客户端 ----------

const wsUrl = `${window.location.protocol === "https:" ? "wss" : "ws"}://${
    window.location.host
}/ws`;

const wsClient = new WsClient(wsUrl, {
    reconnectInterval: 1000,
    maxReconnectInterval: 30000,
    reconnectMultiplier: 2,
    jitter: 0.3,
});

wsClient.onStateChange((state, detail) => {
    updateStatus(state, detail);
});

wsClient.onFrame((frame) => {
    term.write(frame);
});

wsClient.onError((message) => {
    term.writeln(`\r\n\x1b[31m[Error] ${message}\x1b[0m`);
});

wsClient.onResizeRequest(() => ({
    cols: term.cols,
    rows: term.rows,
}));

wsClient.connect();

// ---------- Resize 处理 ----------

function handleResize(): void {
    fitAddon.fit();
    wsClient.reportSize(term.cols, term.rows);
}

window.addEventListener("resize", handleResize);

// ---------- 页面卸载清理 ----------

window.addEventListener("beforeunload", () => {
    window.removeEventListener("resize", handleResize);
    wsClient.disconnect();
});
