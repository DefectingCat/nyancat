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

const { term } = createTerminal(app);

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

let resizeTimer: ReturnType<typeof setTimeout> | null = null;
const RESIZE_DEBOUNCE_MS = 300;

/**
 * 从 xterm.js 现有 canvas 反推真实单元格尺寸。
 * fitAddon.fit() 的缓存会在频繁 resize 后漂移（如 18.50 vs 实际 18.0），
 * 用 canvas 实际尺寸 / 行列数最准确。
 */
function getXtermCellSize(): { width: number; height: number } {
    const screen = app!.querySelector(".xterm-screen") as HTMLElement | null;
    if (screen && term.cols > 0 && term.rows > 0) {
        return {
            width: screen.offsetWidth / term.cols,
            height: screen.offsetHeight / term.rows,
        };
    }
    // 首次无法反推时回退到 DOM 测量
    const span = document.createElement("span");
    span.textContent = "W";
    span.style.fontFamily = (term.options.fontFamily as string) || "monospace";
    span.style.fontSize = `${(term.options.fontSize as number) || 16}px`;
    span.style.visibility = "hidden";
    span.style.position = "absolute";
    span.style.whiteSpace = "pre";
    app!.appendChild(span);
    const rect = span.getBoundingClientRect();
    app!.removeChild(span);
    return { width: rect.width, height: rect.height };
}

function doResize(): void {
    const containerW = app!.clientWidth;
    const containerH = app!.clientHeight;
    const cellSize = getXtermCellSize();
    const cols = Math.max(1, Math.floor(containerW / cellSize.width));
    const rows = Math.max(1, Math.floor(containerH / cellSize.height));

    if (cols === term.cols && rows === term.rows) {
        return;
    }

    term.resize(cols, rows);

    // xterm.js 不会自动更新 viewport 高度，窗口缩小时会溢出容器
    const viewport = app!.querySelector(".xterm-viewport") as HTMLElement | null;
    if (viewport) {
        viewport.style.height = `${containerH}px`;
    }

    requestAnimationFrame(() => {
        // 清屏 + 清 scrollback + 复位光标
        term.write("\x1B[0m\x1B[2J\x1B[3J\x1B[1;1H");
        wsClient.reportSize(cols, rows);
    });
}

function handleResize(): void {
    if (resizeTimer) {
        clearTimeout(resizeTimer);
    }
    resizeTimer = setTimeout(() => {
        resizeTimer = null;
        doResize();
    }, RESIZE_DEBOUNCE_MS);
}

window.addEventListener("resize", handleResize);

// ---------- 页面卸载清理 ----------

window.addEventListener("beforeunload", () => {
    window.removeEventListener("resize", handleResize);
    wsClient.disconnect();
});
