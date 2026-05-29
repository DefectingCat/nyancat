/**
 * WebSocket 通信协议类型定义
 * 与后端 src/http.rs 中的 FrameCode / MessageFrame 对应
 *
 * 注意：tsconfig.json 启用了 erasableSyntaxOnly，因此使用 const 对象 + type 替代 enum
 */

/** 帧类型码 —— 与后端 FrameCode 枚举对齐 */
export const FrameCode = {
    /** 初始化，服务端请求客户端终端尺寸 */
    Init: 0,
    /** 正常帧：客户端上报尺寸 / 服务端发送动画帧 */
    Ok: 1,
    /** 错误帧 */
    Error: 2,
} as const;
export type FrameCode = (typeof FrameCode)[keyof typeof FrameCode];

/** 服务端 → 客户端的消息帧 */
export interface ServerFrame {
    code: FrameCode;
    width?: number;
    height?: number;
    frame?: string;
    error?: string;
}

/** 客户端 → 服务端的消息帧 */
export interface ClientFrame {
    code: typeof FrameCode.Ok;
    width: number;
    height: number;
}

/** 连接状态 */
export const ConnectionState = {
    /** 正在连接 */
    Connecting: "connecting",
    /** 已连接，等待初始化 */
    Handshaking: "handshaking",
    /** 正常传输中 */
    Connected: "connected",
    /** 连接断开 */
    Disconnected: "disconnected",
    /** 正在重连 */
    Reconnecting: "reconnecting",
} as const;
export type ConnectionState =
    (typeof ConnectionState)[keyof typeof ConnectionState];

/** WebSocket 客户端配置选项 */
export interface WsClientOptions {
    /** 重连最大尝试次数（默认无限） */
    maxRetries?: number;
    /** 初始重连间隔（ms，默认 1000） */
    reconnectInterval?: number;
    /** 最大重连间隔（ms，默认 30000） */
    maxReconnectInterval?: number;
    /** 重连间隔乘数（默认 2） */
    reconnectMultiplier?: number;
    /** 随机抖动范围（0-1，默认 0.3） */
    jitter?: number;
}
