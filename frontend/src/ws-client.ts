import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import {
    FrameCode,
    type ClientFrame,
    type ServerFrame,
    ConnectionState,
    type WsClientOptions,
} from "./types.ts";

const DEFAULT_OPTIONS: Required<WsClientOptions> = {
    maxRetries: Infinity,
    reconnectInterval: 1000,
    maxReconnectInterval: 30000,
    reconnectMultiplier: 2,
    jitter: 0.3,
};

/** 连接状态变化回调 */
type StateChangeCallback = (state: ConnectionState, detail?: string) => void;
/** 帧数据接收回调 */
type FrameCallback = (frame: string) => void;
/** 错误回调 */
type ErrorCallback = (message: string) => void;
/** 尺寸请求回调（返回当前终端尺寸） */
type ResizeCallback = () => { cols: number; rows: number };

/**
 * WebSocket 客户端封装
 *
 * 职责：
 * - 自动重连（指数退避 + 抖动）
 * - 连接状态管理
 * - 协议帧序列化/反序列化
 * - resize 消息自动上报
 */
export class WsClient {
    private ws: WebSocket | null = null;
    private state: ConnectionState = ConnectionState.Disconnected;
    private retryCount = 0;
    private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    private readonly options: Required<WsClientOptions>;
    private readonly url: string;

    // 回调
    private onStateChangeCb: StateChangeCallback | null = null;
    private onFrameCb: FrameCallback | null = null;
    private onErrorCb: ErrorCallback | null = null;
    private onResizeCb: ResizeCallback | null = null;

    constructor(url: string, options: WsClientOptions = {}) {
        this.url = url;
        this.options = { ...DEFAULT_OPTIONS, ...options };
    }

    // ---------- 公共 API ----------

    connect(): void {
        if (this.ws?.readyState === WebSocket.CONNECTING) return;
        this.clearReconnectTimer();
        this.setState(ConnectionState.Connecting);

        try {
            this.ws = new WebSocket(this.url);
        } catch (e) {
            this.handleError(`Failed to create WebSocket: ${String(e)}`);
            this.scheduleReconnect();
            return;
        }

        this.ws.onopen = () => {
            this.retryCount = 0;
            this.setState(ConnectionState.Handshaking);
        };

        this.ws.onmessage = (ev) => {
            this.handleMessage(ev.data);
        };

        this.ws.onclose = () => {
            this.ws = null;
            if (this.state !== ConnectionState.Reconnecting) {
                this.setState(ConnectionState.Disconnected);
                this.scheduleReconnect();
            }
        };

        this.ws.onerror = (ev) => {
            this.handleError(`WebSocket error: ${JSON.stringify(ev)}`);
        };
    }

    disconnect(): void {
        this.clearReconnectTimer();
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
        this.setState(ConnectionState.Disconnected);
    }

    /** 上报当前终端尺寸 */
    reportSize(cols: number, rows: number): void {
        if (this.ws?.readyState !== WebSocket.OPEN) return;
        const msg: ClientFrame = {
            code: FrameCode.Ok,
            width: cols,
            height: rows,
        };
        this.ws.send(JSON.stringify(msg));
    }

    // ---------- 回调注册 ----------

    onStateChange(cb: StateChangeCallback): void {
        this.onStateChangeCb = cb;
    }

    onFrame(cb: FrameCallback): void {
        this.onFrameCb = cb;
    }

    onError(cb: ErrorCallback): void {
        this.onErrorCb = cb;
    }

    onResizeRequest(cb: ResizeCallback): void {
        this.onResizeCb = cb;
    }

    getState(): ConnectionState {
        return this.state;
    }

    // ---------- 内部方法 ----------

    private setState(state: ConnectionState, detail?: string): void {
        this.state = state;
        this.onStateChangeCb?.(state, detail);
    }

    private handleMessage(data: string): void {
        let msg: ServerFrame;
        try {
            msg = JSON.parse(data) as ServerFrame;
        } catch (e) {
            this.handleError(`Invalid JSON from server: ${String(e)}`);
            return;
        }

        switch (msg.code) {
            case FrameCode.Init:
                // 服务端请求尺寸，上报当前尺寸
                if (this.onResizeCb) {
                    const { cols, rows } = this.onResizeCb();
                    this.reportSize(cols, rows);
                }
                this.setState(ConnectionState.Connected, "handshake complete");
                break;

            case FrameCode.Ok:
                if (msg.frame !== undefined && msg.frame !== null) {
                    this.onFrameCb?.(msg.frame);
                }
                break;

            case FrameCode.Error:
                this.handleError(msg.error ?? "Server error");
                break;

            default:
                this.handleError(`Unknown frame code: ${msg.code}`);
        }
    }

    private handleError(message: string): void {
        console.error("[WsClient]", message);
        this.onErrorCb?.(message);
    }

    private scheduleReconnect(): void {
        if (
            this.options.maxRetries !== Infinity &&
            this.retryCount >= this.options.maxRetries
        ) {
            this.setState(
                ConnectionState.Disconnected,
                `max retries (${this.options.maxRetries}) reached`
            );
            return;
        }

        this.retryCount++;
        const delay = this.calculateDelay();
        this.setState(
            ConnectionState.Reconnecting,
            `retry ${this.retryCount} in ${Math.round(delay / 1000)}s`
        );

        this.reconnectTimer = setTimeout(() => {
            this.reconnectTimer = null;
            this.connect();
        }, delay);
    }

    private calculateDelay(): number {
        const base = Math.min(
            this.options.reconnectInterval *
                Math.pow(this.options.reconnectMultiplier, this.retryCount - 1),
            this.options.maxReconnectInterval
        );
        const jitter = base * this.options.jitter * Math.random();
        return Math.round(base + jitter);
    }

    private clearReconnectTimer(): void {
        if (this.reconnectTimer) {
            clearTimeout(this.reconnectTimer);
            this.reconnectTimer = null;
        }
    }
}

/**
 * 创建并配置 xterm Terminal + FitAddon
 */
export function createTerminal(container: HTMLElement): {
    term: Terminal;
    fitAddon: FitAddon;
} {
    const term = new Terminal({
        cursorBlink: true,
        fontSize: 16,
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(container);
    fitAddon.fit();
    return { term, fitAddon };
}
