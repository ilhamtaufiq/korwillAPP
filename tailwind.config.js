/** @type {import('tailwindcss').Config} */
// Sumber warna dan font untuk semua halaman. Ubah warna di sini, lalu jalankan `npm run build:css`.
module.exports = {
  content: ['./static/**/*.html', './static/**/*.js'],
  theme: {
    extend: {
      colors: {
        primary: '#059669',
        'primary-hover': '#047857',
        'primary-soft': '#ECFDF5',
        canvas: '#F8FAFC',
        muted: '#F1F5F9',
        line: '#E2E8F0',
        ink: '#0F172A',
        ink2: '#334155',
        ink3: '#64748B',
        danger: '#B91C1C',
        'danger-soft': '#FEF2F2',
        'danger-line': '#FCA5A5',
        warn: '#B45309',
        'warn-soft': '#FFFBEB',
        'warn-line': '#FCD34D',
      },
      fontFamily: {
        sans: ['Inter', 'sans-serif'],
      },
    },
  },
  plugins: [],
};
