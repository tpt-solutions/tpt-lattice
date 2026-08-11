import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

// The wasm engine package is built from the `tpt-lattice-wasm` crate into
// `crates/tpt-lattice-wasm/pkg` (run `wasm-pack build` / the repo's build step
// first). We alias it so the worker can `import ... from "@engine/..."`.
export default defineConfig({
  plugins: [solid()],
  worker: {
    format: "es",
  },
  resolve: {
    alias: {
      "@engine": resolve(__dirname, "../crates/tpt-lattice-wasm/pkg"),
    },
  },
  server: {
    fs: {
      // Allow Vite to serve the wasm package that lives outside this folder.
      allow: [resolve(__dirname, "..")],
    },
  },
});
