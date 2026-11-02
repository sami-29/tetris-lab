import { defineConfig } from "vite";

// Zero runtime dependencies, one hand-loaded wasm artifact.
//
// `assetsInlineLimit: 0` is the load-bearing setting: Vite would otherwise inline a
// small asset as a data URL, and the loader's `compileStreaming` path (and the
// MIME requirement §10 puts on the host) needs a real `application/wasm` response.
// The artifact is emitted as a hashed file, so a deploy is cacheable forever.
export default defineConfig({
  build: {
    target: "es2022",
    outDir: "dist",
    assetsInlineLimit: 0,
    sourcemap: true,
  },
  worker: {
    format: "es",
  },
  server: {
    port: 5173,
  },
});
