import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Build ide u `dist`, a `dist` se ugrađuje u binarni fajl (rust-embed).
// U razvoju (`npm run dev`) zahtjevi idu na pokrenuti server (zadano :8200).
const target = process.env.RUSTIIO_API || 'http://127.0.0.1:8200'

export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
    sourcemap: false,
  },
  server: {
    port: 5173,
    proxy: {
      '/api': { target },
      '/art': { target },
      '/res': { target },
      '/sub': { target },
      '/tr': { target },
      '/ws': { target: target.replace('http', 'ws'), ws: true },
      '/healthz': { target },
    },
  },
})
