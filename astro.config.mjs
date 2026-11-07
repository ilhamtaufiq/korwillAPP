import { defineConfig } from 'astro/config';

// Sumber halaman ada di web/src dan aset publik di web/public. Hasil build ditulis ke static/,
// yang dilayani Axum seperti sebelumnya.
export default defineConfig({
  srcDir: './web/src',
  publicDir: './web/public',
  outDir: './static',
  build: { format: 'file' },
});