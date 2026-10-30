// Language switch DE/EN: the choice is kept in this browser; without JavaScript both languages are shown.
(() => {
  const root = document.documentElement;
  const valid = (v) => v === 'de' || v === 'en';
  let lang = null;
  try { lang = new URLSearchParams(location.search).get('lang'); } catch (e) { /* old browser */ }
  if (!valid(lang)) { try { lang = localStorage.getItem('lang'); } catch (e) { lang = null; } }
  if (!valid(lang)) lang = (navigator.language || 'en').toLowerCase().startsWith('de') ? 'de' : 'en';
  const apply = (l) => {
    root.dataset.l = l;
    root.lang = l;
    document.querySelectorAll('.langsw button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.set === l)));
  };
  apply(lang);
  document.addEventListener('DOMContentLoaded', () => {
    apply(lang);
    document.querySelectorAll('.langsw button').forEach((b) => b.addEventListener('click', () => {
      lang = b.dataset.set;
      try { localStorage.setItem('lang', lang); } catch (e) { /* private mode */ }
      apply(lang);
    }));
  });
})();
