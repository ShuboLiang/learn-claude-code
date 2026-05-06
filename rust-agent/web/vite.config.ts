import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'node:path'

const BACKEND = 'http://localhost:3000'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/sessions': { target: BACKEND, changeOrigin: true },
      '/v1': { target: BACKEND, changeOrigin: true },
      '/config': { target: BACKEND, changeOrigin: true },
      '/bots': { target: BACKEND, changeOrigin: true },
      '/browse': { target: BACKEND, changeOrigin: true },
      '/watch': { target: BACKEND, changeOrigin: true },
      '/file': { target: BACKEND, changeOrigin: true },
    },
  },
})
