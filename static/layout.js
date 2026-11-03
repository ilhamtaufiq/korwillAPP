// Layout master: sidebar, topbar, dan pembungkus konten. Dimuat oleh setiap halaman yang sudah login.
// Konten halaman dipindahkan ke dalam #page, sehingga id dan script halaman tetap bekerja.

(function () {
  const COLLAPSED_KEY = 'sidebar-collapsed';

  // Menu sesuai desain Stitch. Yang belum punya halaman ditampilkan nonaktif.
  const NAV = [
    { href: '/', label: 'Ringkasan Wilayah', icon: 'grid_view' },
    { href: '/daftar-proyek.html', label: 'Daftar Pekerjaan', icon: 'construction' },
    { href: '/tambah-proyek.html', label: 'Tambah Pekerjaan', icon: 'add_circle', adminOnly: true, id: 'menu-tambah' },
    { href: '/users.html', label: 'Daftar User', icon: 'group', adminOnly: true, id: 'menu-users' },
    { href: '/log-dokumentasi.html', label: 'Log Dokumentasi', icon: 'photo_camera', adminOnly: true, id: 'menu-log' },
    { href: '/lapming.html', label: 'Laporan Lapming', icon: 'monitoring', id: 'menu-lapming' },
    { href: '/ganti-sandi.html', label: 'Ganti Kata Sandi', icon: 'key' },
  ];

  function injectStyle() {
    const style = document.createElement('style');
    style.textContent = `
      .material-symbols-outlined { font-family: 'Material Symbols Outlined'; font-weight: normal; font-style: normal; font-size: 20px; line-height: 1; display: inline-block; font-feature-settings: 'liga'; -webkit-font-smoothing: antialiased; }
      #sidebar { position: fixed; left: 0; top: 0; height: 100%; width: 16rem; z-index: 50; display: none; flex-direction: column; justify-content: space-between; background: #fff; border-right: 1px solid #E2E8F0; transition: width .2s; overflow-y: auto; }
      @media (min-width: 1024px) { #sidebar { display: flex; } }
      body.sb-open #sidebar { display: flex; }
      #topbar { position: fixed; top: 0; right: 0; left: 0; height: 4rem; z-index: 40; display: flex; align-items: center; justify-content: space-between; padding: 0 1rem; background: #fff; border-bottom: 1px solid #E2E8F0; transition: left .2s; }
      @media (min-width: 1024px) { #topbar { left: 16rem; } }
      #page { padding-top: 4rem; transition: padding .2s; }
      @media (min-width: 1024px) { #page { padding-left: 16rem; } }
      body.sb-collapsed #sidebar { width: 4rem; }
      @media (min-width: 1024px) {
        body.sb-collapsed #topbar { left: 4rem; }
        body.sb-collapsed #page { padding-left: 4rem; }
      }
      body.sb-collapsed .nav-label, body.sb-collapsed .brand-text, body.sb-collapsed .section-label { display: none; }
      body.sb-collapsed .nav-item { justify-content: center; padding-left: 0; padding-right: 0; }
    `;
    document.head.appendChild(style);

    const font = document.createElement('link');
    font.rel = 'stylesheet';
    font.href = 'https://fonts.googleapis.com/css2?family=Material+Symbols+Outlined:wght,FILL@100..700,0..1&display=swap';
    document.head.appendChild(font);
  }

  function navItem(item, active) {
    const base = 'nav-item flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-semibold';
    if (item.href) {
      const cls = active
        ? 'border-l-2 border-primary bg-primary-soft text-primary'
        : 'text-ink2 hover:bg-muted';
      return `<a href="${item.href}" ${item.id ? `id="${item.id}" hidden` : ''} class="${base} ${cls}" title="${item.label}">
        <span class="material-symbols-outlined">${item.icon}</span><span class="nav-label">${item.label}</span></a>`;
    }
    return `<span class="${base} cursor-not-allowed text-ink3 opacity-60" title="Belum tersedia">
      <span class="material-symbols-outlined">${item.icon}</span><span class="nav-label">${item.label}</span></span>`;
  }

  function buildShell(user) {
    const path = location.pathname;
    const isActive = (href) => (href === '/' ? path === '/' || path === '/index.html' : path === href);
    const role = user ? (user.role === 'admin' ? 'Admin' : user.role === 'korwil' ? 'Koordinator Wilayah' : 'Tenaga Fasilitator') : '';
    const name = user ? user.username : '';

    const wrap = document.createElement('div');
    wrap.innerHTML = `
      <aside id="sidebar">
        <div class="flex flex-col">
          <div class="flex h-16 items-center gap-3 border-b border-line px-4">
            <img src="/logo.svg" alt="" class="h-8 w-8 shrink-0 rounded-lg">
            <div class="brand-text flex flex-col">
              <span class="text-sm font-bold leading-none">KorwilAPP</span>
              <span class="text-xs text-ink3">Sanimas Wilayah</span>
            </div>
          </div>
          <div class="p-4">
            <p class="section-label px-2 pb-2 text-xs font-semibold uppercase tracking-wider text-ink3">Menu Utama</p>
            <nav class="flex flex-col gap-1">
              ${NAV.filter((i) => i.href !== '/ganti-sandi.html').map((i) => navItem(i, isActive(i.href))).join('')}
            </nav>
          </div>
          <div class="px-4 py-2">
            <p class="section-label px-2 pb-2 text-xs font-semibold uppercase tracking-wider text-ink3">Akun</p>
            <nav class="flex flex-col gap-1">
              ${NAV.filter((i) => i.href === '/ganti-sandi.html').map((i) => navItem(i, isActive(i.href))).join('')}
            </nav>
          </div>
        </div>
      </aside>
      <header id="topbar">
        <div class="flex items-center gap-3">
          <button id="sb-toggle" type="button" aria-label="Perkecil atau besarkan menu" class="flex h-11 w-11 items-center justify-center rounded-lg text-ink2 hover:bg-muted">
            <span class="material-symbols-outlined">menu</span>
          </button>
          <div class="hidden items-center gap-2 text-xs text-ink3 md:flex">
            <span class="material-symbols-outlined text-[16px]">map</span>
            <span>Wilayah Operasional</span><span>/</span>
            <span class="font-semibold text-ink">Korwil I - Jawa Barat</span>
          </div>
        </div>
        <div class="flex items-center gap-3">
          <div class="flex flex-col text-right">
            <span id="user-name" class="text-sm font-semibold">${name}</span>
            <span id="user-role" class="text-xs text-ink3">${role}</span>
          </div>
          <button onclick="logout()" class="h-9 rounded-lg border border-line px-3 text-sm font-semibold text-ink2 hover:bg-muted">Keluar</button>
        </div>
      </header>
      <main id="page"></main>`;
    return wrap;
  }

  function setCollapsed(on) {
    document.body.classList.toggle('sb-collapsed', on);
    try { localStorage.setItem(COLLAPSED_KEY, on ? '1' : ''); } catch (_) {}
  }

  async function mount() {
    // Pindahkan konten halaman ke dalam satu kontainer. <main> lama dibuka (isinya dipindah).
    const content = document.createElement('div');
    content.className = 'mx-auto max-w-7xl px-4 pb-10 lg:px-6';
    for (const el of [...document.body.children]) {
      if (el.tagName === 'SCRIPT') continue;
      if (el.tagName === 'MAIN') {
        while (el.firstChild) content.appendChild(el.firstChild);
        el.remove();
      } else {
        content.appendChild(el);
      }
    }

    // Pasang layout sebelum menunggu /api/me, supaya elemen halaman tetap ada di dokumen.
    const shell = buildShell(null);
    document.body.prepend(...shell.childNodes);
    document.getElementById('page').appendChild(content);

    // Tombol menu: di desktop memperkecil sidebar, di HP membuka/menutup sidebar.
    const desktop = window.matchMedia('(min-width: 1024px)');
    document.getElementById('sb-toggle').addEventListener('click', () => {
      if (desktop.matches) {
        setCollapsed(!document.body.classList.contains('sb-collapsed'));
      } else {
        document.body.classList.toggle('sb-open');
      }
    });

    try {
      if (localStorage.getItem(COLLAPSED_KEY) === '1' && desktop.matches) {
        document.body.classList.add('sb-collapsed');
      }
    } catch (_) {}

    // Isi nama, peran, dan menu admin setelah data user dimuat.
    try {
      const res = await fetch('/api/me', { credentials: 'same-origin' });
      if (res.ok) {
        const user = await res.json();
        document.getElementById('user-name').textContent = user.username;
        document.getElementById('user-role').textContent =
          user.role === 'admin' ? 'Admin' : user.role === 'korwil' ? 'Koordinator Wilayah' : 'Tenaga Fasilitator';
        if (user.role === 'admin' || user.role === 'korwil') {
          const menu = document.getElementById('menu-lapming');
          if (menu) menu.hidden = false;
        }
        if (user.role === 'admin') {
          ['menu-tambah', 'menu-users', 'menu-log'].forEach((id) => {
            const menu = document.getElementById(id);
            if (menu) menu.hidden = false;
          });
        }
      }
    } catch (_) {}
  }

  injectStyle();
  document.addEventListener('DOMContentLoaded', mount);
})();
