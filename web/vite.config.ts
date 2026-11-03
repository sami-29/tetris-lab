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
    // `data/measurements/width-curve.json` — the artifact `bench width-curve`
    // writes and `src/config/width.ts` imports for the two numbers — is a sibling
    // of `web/`, which the dev server refuses by default. The build reads it
    // through Rollup's JSON plugin and needs nothing; this is only so `npm run dev`
    // serves the same file the bundle does.
    fs: { allow: [".."] },
  },
});
