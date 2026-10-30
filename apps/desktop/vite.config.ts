import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  test: { environment: 'happy-dom', include: ['src/**/*.test.ts'], restoreMocks: true },
})
