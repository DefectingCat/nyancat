import { defineConfig } from "vite";

export default defineConfig(({ mode }) => ({
  // 生产构建时移除 console 和 debugger
  esbuild: {
    drop: mode === "production" ? ["console", "debugger"] : [],
  },

  build: {
    // 构建目标：现代浏览器，减少 polyfill
    target: "es2020",

    // 生产环境不生成 sourcemap
    sourcemap: false,

    // 关闭 gzip 压缩大小报告，加速构建
    reportCompressedSize: false,

    // 小于 4KB 的资源内联为 base64，减少 HTTP 请求
    assetsInlineLimit: 4096,

    // 代码分割：将大型依赖拆分为独立 chunk，提升缓存命中率
    rollupOptions: {
      output: {
        manualChunks(id) {
          // 将 xterm 相关依赖拆分为独立 chunk
          if (id.includes("@xterm")) {
            return "xterm";
          }
        },
      },
    },
  },
}));
