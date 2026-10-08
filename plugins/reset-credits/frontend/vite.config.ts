import { fileURLToPath, URL } from 'node:url'
import CodexProxyUI from '@codex-proxy/ui/vite'
import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'

export default defineConfig(({ mode }) => {
  const sourceUi = mode === 'source'
  const uiRoot = new URL('../../../../ui/', import.meta.url)

  return {
    base: './',
    plugins: [
      vue(),
      tailwindcss(),
      CodexProxyUI({ source: sourceUi ? uiRoot : undefined }),
      {
        name: 'plugin-classic-script',
        // 宿主隔离页只接受经典脚本；HTML 和资源引用仍由 Vite 生成。
        transformIndexHtml: { order: 'post', handler: html => html.replace('type="module" crossorigin', 'defer') },
      },
    ],
    resolve: {
      alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
    },
    build: {
      outDir: sourceUi ? 'node_modules/.vite/source-dist' : 'dist',
      modulePreload: false,
      cssCodeSplit: false,
      rolldownOptions: {
        output: { format: 'iife', codeSplitting: false, entryFileNames: 'app.js', assetFileNames: 'app.[ext]' },
      },
    },
  }
})
