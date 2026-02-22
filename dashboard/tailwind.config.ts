import type { Config } from 'tailwindcss';

export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        mesh: {
          bg: '#000000',
          sidebar: '#0a0a0a',
          surface: '#111111',
          'surface-hover': '#1a1a1a',
          border: '#333333',
          'border-hover': '#444444',
          accent: '#0070f3',
          'accent-hover': '#0060d0',
          success: '#0070f3',
          error: '#ee0000',
          warning: '#f5a623',
          muted: '#888888',
          text: '#ededed',
          'text-secondary': '#888888',
        },
      },
      fontFamily: {
        sans: ['Geist', 'system-ui', '-apple-system', 'sans-serif'],
        mono: ['Geist Mono', 'Menlo', 'Monaco', 'monospace'],
      },
      maxWidth: {
        content: '1200px',
      },
    },
  },
  plugins: [],
} satisfies Config;
