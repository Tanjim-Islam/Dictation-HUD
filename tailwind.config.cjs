/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    './index.html',
    './src/**/*.{ts,tsx}',
  ],
  theme: {
    extend: {
      colors: {
        bg: '#171717',
        fg: '#f2f1ea',
        muted: '#c9c7bf',
        card: '#1f1f1f',
        accent: '#e5e2d6',
        badge: {
          border: '#7a1f1f',
          bg: '#fbe9e9',
          text: '#7a1f1f'
        }
      },
      fontFamily: {
        geistmono: ['Geist Mono', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace']
      }
    },
  },
  plugins: [],
};

