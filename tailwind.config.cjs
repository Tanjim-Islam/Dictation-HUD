/** @type {import('tailwindcss').Config} */
const token = (name) => `rgb(var(--${name}) / <alpha-value>)`;

module.exports = {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        frame: token('frame'),
        panel: token('panel'),
        surface: token('surface'),
        'surface-strong': token('surface-strong'),
        border: token('border'),
        ink: token('ink'),
        'ink-muted': token('ink-muted'),
        'ink-faint': token('ink-faint'),
        track: token('track'),
        accent: token('accent'),
        'accent-soft': token('accent-soft'),
        danger: token('danger'),
        'danger-soft': token('danger-soft'),
        success: token('success'),
        'success-soft': token('success-soft'),
        warning: token('warning'),
        'warning-soft': token('warning-soft'),
      },
      fontFamily: {
        sans: ['"Figtree Variable"', 'Figtree', '"Segoe UI Variable Text"', '"Segoe UI"', 'system-ui', 'sans-serif'],
        display: ['"EB Garamond"', 'Georgia', '"Times New Roman"', 'serif'],
        mono: ['"Cascadia Code"', 'Consolas', 'ui-monospace', 'monospace'],
      },
      fontSize: {
        '2xs': ['11px', '14px'],
        xs: ['12px', '16px'],
        sm: ['13px', '18px'],
        base: ['15px', '22px'],
        lg: ['17px', '24px'],
        xl: ['20px', '28px'],
        '2xl': ['24px', '30px'],
        '3xl': ['30px', '36px'],
      },
      borderRadius: {
        sm: '6px',
        DEFAULT: '8px',
        md: '10px',
        lg: '12px',
        xl: '16px',
        '2xl': '20px',
      },
      boxShadow: {
        menu: '0 8px 24px -6px rgb(26 26 26 / 0.14), 0 2px 6px -2px rgb(26 26 26 / 0.08)',
        dialog: '0 24px 64px -12px rgb(26 26 26 / 0.28), 0 4px 16px -4px rgb(26 26 26 / 0.10)',
        pill: '0 6px 18px -4px rgb(0 0 0 / 0.45)',
      },
      keyframes: {
        'fade-in': { from: { opacity: '0' }, to: { opacity: '1' } },
        'fade-out': { from: { opacity: '1' }, to: { opacity: '0' } },
        'pop-in': { from: { opacity: '0', transform: 'translateY(-4px) scale(0.98)' }, to: { opacity: '1', transform: 'none' } },
        'pop-out': { from: { opacity: '1', transform: 'none' }, to: { opacity: '0', transform: 'translateY(-4px) scale(0.98)' } },
        'dialog-in': { from: { opacity: '0', transform: 'translate(-50%, -48%) scale(0.97)' }, to: { opacity: '1', transform: 'translate(-50%, -50%) scale(1)' } },
        'dialog-out': { from: { opacity: '1', transform: 'translate(-50%, -50%) scale(1)' }, to: { opacity: '0', transform: 'translate(-50%, -48%) scale(0.97)' } },
      },
      animation: {
        'fade-in': 'fade-in 160ms cubic-bezier(0.22, 1, 0.36, 1)',
        'fade-out': 'fade-out 120ms ease-in',
        'pop-in': 'pop-in 160ms cubic-bezier(0.22, 1, 0.36, 1)',
        'pop-out': 'pop-out 110ms ease-in',
        'dialog-in': 'dialog-in 220ms cubic-bezier(0.22, 1, 0.36, 1)',
        'dialog-out': 'dialog-out 140ms ease-in',
      },
    },
  },
  plugins: [require('tailwindcss-animate')],
};
