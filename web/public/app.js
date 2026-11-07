// Helper bersama untuk semua halaman. Dimuat oleh setiap halaman di folder static.

// Halaman yang butuh login disembunyikan dulu. Ditampilkan setelah requireAuth berhasil,
// supaya tidak sempat terlihat sebelum diarahkan ke halaman masuk.
(function () {
  if (location.pathname === '/login.html') return;
  const hide = document.createElement('style');
  hide.id = 'auth-hide';
  hide.textContent = 'body { visibility: hidden; }';
  document.head.appendChild(hide);
})();

async function api(method, path, body, text) {
  const opts = { method, headers: {}, credentials: 'same-origin' };
  if (body !== undefined) {
    opts.headers['Content-Type'] = 'application/json';
    opts.body = JSON.stringify(body);
  } else if (text !== undefined) {
    opts.headers['Content-Type'] = 'text/plain; charset=utf-8';
    opts.body = text;
  }
  const res = await fetch(path, opts);
  const data = res.status === 204 ? null : await res.json().catch(() => null);
  if (!res.ok) {
    throw new Error((data && data.error) || 'Permintaan gagal, coba lagi.');
  }
  return data;
}

// Dipanggil di awal halaman yang butuh login. Kalau belum login, kembali ke halaman masuk.
async function requireAuth() {
  let user;
  try {
    user = await api('GET', '/api/me');
  } catch (_) {
    location.href = '/login.html';
    throw new Error('Belum login');
  }
  document.getElementById('auth-hide')?.remove();
  return user;
}

async function logout() {
  await api('POST', '/api/logout').catch(() => {});
  location.href = '/login.html';
}

function esc(value) {
  return String(value).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));
}

function showError(el, message) {
  el.textContent = message;
  el.classList.remove('hidden');
}

function hideError(el) {
  el.textContent = '';
  el.classList.add('hidden');
}

// Placeholder saat data masih dimuat. Dipakai sebelum await, lalu ditimpa hasil render.
function skeletonBar(extra = '') {
  return `<div class="h-3 animate-pulse rounded bg-muted ${extra}"></div>`;
}

function skeletonRows(cols, n = 5) {
  const cells = Array.from({ length: cols }, () => `<td class="px-3 py-3">${skeletonBar('w-4/5')}</td>`).join('');
  return `<tr class="border-t border-line">${cells}</tr>`.repeat(n);
}

function skeletonItems(n = 3) {
  return Array.from(
    { length: n },
    () => `<li class="flex items-center gap-3 px-4 py-3">
      <div class="h-10 w-10 shrink-0 animate-pulse rounded bg-muted"></div>
      <div class="flex-1 space-y-2">${skeletonBar('w-2/3')}${skeletonBar('w-1/2')}</div>
    </li>`,
  ).join('');
}

function skeletonCards(n = 6) {
  return Array.from(
    { length: n },
    () => `<div class="rounded-lg border border-line bg-white p-4">
      <div class="space-y-3">${skeletonBar('w-3/4')}${skeletonBar('w-1/2')}${skeletonBar('w-2/3')}</div>
    </div>`,
  ).join('');
}
