import { defineConfig } from 'vite'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath } from 'node:url'

export default defineConfig({
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  server: {
    port: 3001,
    proxy: { '/api': 'http://localhost:3000' },
  },
  // TanStack prerenders its SPA shell through Vite preview. An explicit IPv4
  // loopback keeps Bun/Linux's localhost resolution aligned with the listener.
  preview: { host: '127.0.0.1' },
  plugins: [
    tailwindcss(),
    tanstackStart({
      spa: { enabled: true, prerender: { crawlLinks: false, outputPath: '/_shell.html' } },
    }),
    viteReact(),
  ],
})
