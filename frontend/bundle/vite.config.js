import { defineConfig } from "vite";

export default defineConfig({
  build: {
    emptyOutDir: true,
    lib: {
      entry: "./index.ts",
      formats: ["es"],
      fileName: () => "xterm-bridge.js",
    },
    rollupOptions: {
      output: {
        assetFileNames: (assetInfo) => {
          if (assetInfo.name === "style.css") {
            return "xterm-bridge.css";
          }
          return "assets/[name][extname]";
        },
      },
    },
  },
});
