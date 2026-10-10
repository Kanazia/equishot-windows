import path from "node:path";
import process from "node:process";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],

  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },

  // Sept pages : la fenêtre EquiShot (galerie, réglages), la pastille, la sélection, les vignettes, les épingles, la barre d'enregistrement vidéo, la bulle de caméra.
  build: {
    rollupOptions: {
      input: {
        index: path.resolve(__dirname, "index.html"),
        pastille: path.resolve(__dirname, "pastille.html"),
        selection: path.resolve(__dirname, "selection.html"),
        vignettes: path.resolve(__dirname, "vignettes.html"),
        epingle: path.resolve(__dirname, "epingle.html"),
        enregistrement: path.resolve(__dirname, "enregistrement.html"),
        camera: path.resolve(__dirname, "camera.html"),
      },
    },
  },

  // Options Vite adaptées à Tauri, port fixe 1430.
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1431 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
