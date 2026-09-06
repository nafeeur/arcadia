import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    // Parallel dev sessions: PORT is assigned by the launcher (switches ports if 5173 is taken), keeps the default when unset
    port: process.env.PORT ? Number(process.env.PORT) : 5173,
    proxy: {
      // Backend port can be overridden with UTOPIA_DEV_API (defaults to 1516, matching UTOPIA_BIND_ADDR in .env)
      "/api": process.env.UTOPIA_DEV_API ?? "http://127.0.0.1:1516",
    },
  },
});
