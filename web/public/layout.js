// Pengatur layout yang sudah ada di markup (Shell.astro): tombol perkecil/besarkan sidebar,
// nama dan peran user, serta menu yang hanya tampil untuk peran tertentu.
(function () {
  const COLLAPSED_KEY = 'sidebar-collapsed';
  const desktop = window.matchMedia('(min-width: 1024px)');

  function setCollapsed(on) {
    document.body.classList.toggle('sb-collapsed', on);
    try { localStorage.setItem(COLLAPSED_KEY, on ? '1' : ''); } catch (_) {}
  }

  async function mount() {
    try {
      if (localStorage.getItem(COLLAPSED_KEY) === '1' && desktop.matches) {
        document.body.classList.add('sb-collapsed');
      }
    } catch (_) {}

    // Di desktop tombol memperkecil sidebar; di HP membuka atau menutupnya.
    document.getElementById('sb-toggle')?.addEventListener('click', () => {
      if (desktop.matches) {
        setCollapsed(!document.body.classList.contains('sb-collapsed'));
      } else {
        document.body.classList.toggle('sb-open');
      }
    });

    try {
      const res = await fetch('/api/me', { credentials: 'same-origin' });
      if (!res.ok) return;
      const user = await res.json();
      document.getElementById('user-name').textContent = user.username;
      document.getElementById('user-role').textContent =
        user.role === 'admin' ? 'Admin' : user.role === 'korwil' ? 'Koordinator Wilayah' : 'Tenaga Fasilitator';
      const admin = user.role === 'admin';
      ['menu-tambah', 'menu-users', 'menu-log'].forEach((id) => {
        const m = document.getElementById(id);
        if (m) m.hidden = !admin;
      });
      const lapming = document.getElementById('menu-lapming');
      if (lapming) lapming.hidden = !(admin || user.role === 'korwil');
      const anomali = document.getElementById('menu-anomali');
      if (anomali) anomali.hidden = !(admin || user.role === 'korwil');
    } catch (_) {}
  }

  document.addEventListener('DOMContentLoaded', mount);
})();