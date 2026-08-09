import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Phase 11: dev server on :3000 to match docker-compose.yml / Makefile's
// `make dashboard`. The backend (server/) runs on :8000 by default and
// is reached via VITE_API_BASE_URL (see .env.example) — CORS for this
// origin is allowed server-side via PLDDS_DASHBOARD_ORIGINS.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 3000,
  },
  preview: {
    port: 3000,
  },
});
