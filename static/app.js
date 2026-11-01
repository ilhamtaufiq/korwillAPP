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

// Atribut hidden harus selalu menyembunyikan elemen. Tanpa ini, kelas flex dari Tailwind menimpanya.
(function () {
  const style = document.createElement('style');
  style.textContent = '[hidden] { display: none !important; }';
  document.head.appendChild(style);
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
  if (user.must_change_password) {
    showPasswordNotice();
  }
  return user;
}

// Notifikasi di bagian paling atas halaman untuk user yang masih memakai kata sandi awal.
function showPasswordNotice() {
  if (document.getElementById('password-notice')) return;
  const bar = document.createElement('a');
  bar.id = 'password-notice';
  bar.href = '/ganti-sandi.html';
  bar.className =
    'block border-b border-danger-line bg-danger-soft px-4 py-2 text-center text-sm font-semibold text-danger';
  bar.textContent = 'Kata sandi Anda masih kata sandi awal. Ketuk di sini untuk menggantinya sekarang.';
  (document.getElementById('page') || document.body).prepend(bar);
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
