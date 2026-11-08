// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// The two editors of a project of a slideshow (src/slideshow.rs, docs/spec-pictures.md, 9; project owner, 2026-11-08):
//  - the preview (templates/slideshow_pick.html): the pictures of the choice above as small pictures, month by month; a tap puts one
//    into the project or takes it out again;
//  - the editor (templates/slideshow.html): the properties, and the pictures in their order, moved by their number (the field, the
//    buttons up and down), each with its transition and its seconds.
// Every change is saved at once; the server checks that the person may see each picture.
(() => {
  'use strict';
  const $ = (id) => document.getElementById(id);

  function csrf() {
    try { return JSON.parse(document.body.getAttribute('hx-headers') || '{}')['X-CSRF-Token'] || ''; } catch { return ''; }
  }

  async function post(url, body, json) {
    const headers = { 'X-CSRF-Token': csrf() };
    headers['Content-Type'] = json ? 'application/json' : 'application/x-www-form-urlencoded';
    const r = await fetch(url, { method: 'POST', credentials: 'same-origin', headers, body: json ? JSON.stringify(body) : body });
    if (!r.ok) throw new Error(String(r.status));
    return r.status === 204 ? null : r.json();
  }

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined) e.textContent = text;
    return e;
  }

  // ---- The preview ---------------------------------------------------------------------------------------------------------------

  const pick = $('pick-form');
  if (pick) {
    const id = pick.dataset.show;
    const t = pick.dataset;
    const base = '/pictures/shows/' + encodeURIComponent(id);
    let chosen = new Set();
    try { chosen = new Set(JSON.parse(t.chosen || '[]')); } catch { chosen = new Set(); }
    let shown = [];
    const grid = $('pick-grid');
    const errorEl = $('pick-error');
    const KEY = 'david.slideshow.pick';

    const counted = () => { $('pick-count').textContent = t.tCount.replace('{n}', String(chosen.size)); };
    const fail = () => { errorEl.textContent = t.tFailed; errorEl.hidden = false; };

    function query() {
      const q = new URLSearchParams();
      for (const i of pick.querySelectorAll('input[name=source]:checked')) q.append('source', i.value);
      const from = pick.elements.from.value;
      const to = pick.elements.to.value;
      if (from) q.set('from', String(new Date(Number(from), 0, 1).getTime()));
      if (to) q.set('to', String(new Date(Number(to) + 1, 0, 1).getTime() - 1));
      if (pick.elements.videos.value === 'on') q.set('videos', 'on');
      return q;
    }

    function remember() {
      const c = { sources: [...pick.querySelectorAll('input[name=source]:checked')].map((i) => i.value), from: pick.elements.from.value, to: pick.elements.to.value, videos: pick.elements.videos.value };
      try { localStorage.setItem(KEY, JSON.stringify(c)); } catch { /* not kept in a private window */ }
    }

    function restore() {
      let c = null;
      try { c = JSON.parse(localStorage.getItem(KEY) || 'null'); } catch { c = null; }
      if (!c) return;
      if (Array.isArray(c.sources) && c.sources.length) pick.querySelectorAll('input[name=source]').forEach((i) => { i.checked = c.sources.includes(i.value); });
      for (const f of ['from', 'to', 'videos']) {
        const s = pick.elements[f];
        if (s && typeof c[f] === 'string' && [...s.options].some((o) => o.value === c[f])) {
          s.value = c[f];
          s.dispatchEvent(new Event('change', { bubbles: true }));
        }
      }
    }

    function mark(btn, key) {
      const on = chosen.has(key);
      btn.setAttribute('aria-pressed', on ? 'true' : 'false');
      btn.title = (on ? t.tIn : t.tOut);
    }

    let loading = 0;
    async function load() {
      const mine = ++loading;
      errorEl.hidden = true;
      const q = query();
      let list = [];
      if (q.getAll('source').length) {
        try {
          const r = await fetch('/pictures/list?' + q, { credentials: 'same-origin' });
          if (!r.ok) throw new Error(String(r.status));
          list = (await r.json()).pictures || [];
        } catch { if (mine === loading) fail(); return; }
      }
      if (mine !== loading) return;
      shown = list;
      render();
    }

    function render() {
      grid.replaceChildren();
      $('pick-shown').textContent = t.tShown.replace('{n}', String(shown.length));
      const month = new Intl.DateTimeFormat(document.documentElement.lang || 'en', { month: 'long', year: 'numeric' });
      const day = new Intl.DateTimeFormat(document.documentElement.lang || 'en', { dateStyle: 'medium' });
      let last = '';
      const frag = document.createDocumentFragment();
      for (const p of shown) {
        const m = month.format(new Date(p.taken_at));
        if (m !== last) { frag.append(el('li', 'pick-month', m)); last = m; }
        const li = el('li', 'pick-cell');
        const btn = el('button', 'pick-tile');
        btn.type = 'button';
        btn.dataset.key = p.key;
        const img = el('img');
        img.src = '/pictures/' + encodeURIComponent(p.key) + '/thumb';
        img.alt = day.format(new Date(p.taken_at)) + ' · ' + p.name;
        img.loading = 'lazy';
        img.decoding = 'async';
        btn.append(img, el('span', 'pick-check', '✓'));
        if (p.video) btn.append(el('span', 'pick-video', t.tVideo));
        mark(btn, p.key);
        li.append(btn);
        frag.append(li);
      }
      grid.append(frag);
    }

    async function change(keys, add) {
      if (!keys.length) return;
      try {
        const r = await post(base + (add ? '/add' : '/remove'), { keys }, true);
        for (const k of keys) { if (add) chosen.add(k); else chosen.delete(k); }
        // The server takes only what the person may see and stops at the most a project holds: the count says what is in it.
        if (add && r && r.count < chosen.size) {
          window.location.href = base + '/pick?msg=err.show_full';
          return;
        }
      } catch { fail(); return; }
      counted();
      grid.querySelectorAll('.pick-tile').forEach((b) => { if (keys.includes(b.dataset.key)) mark(b, b.dataset.key); });
    }

    grid.addEventListener('click', (e) => {
      const b = e.target.closest('.pick-tile');
      if (!b) return;
      change([b.dataset.key], !chosen.has(b.dataset.key));
    });
    $('pick-all').addEventListener('click', () => change(shown.map((p) => p.key).filter((k) => !chosen.has(k)), true));
    $('pick-none').addEventListener('click', () => change(shown.map((p) => p.key).filter((k) => chosen.has(k)), false));
    pick.addEventListener('change', () => { remember(); load(); });
    restore();
    counted();
    load();
  }

  // ---- The editor ------------------------------------------------------------------------------------------------------------------

  const props = $('edit-props');
  if (props) {
    const id = props.dataset.show;
    const base = '/pictures/shows/' + encodeURIComponent(id);
    const status = $('edit-status');
    const list = $('edit-list');
    const say = (text) => { status.textContent = text; };

    // The properties: saved when a field changes (the title when it is left or after a pause in typing).
    let typing = 0;
    async function saveProps() {
      clearTimeout(typing);
      if (!props.elements.title.value.trim()) return;
      try {
        await post(base, new URLSearchParams(new FormData(props)).toString(), false);
        $('edit-title').textContent = props.elements.title.value.trim();
        say(props.dataset.tSaved);
      } catch { say(props.dataset.tFailed); }
    }
    props.addEventListener('change', saveProps);
    props.elements.title.addEventListener('input', () => { clearTimeout(typing); typing = setTimeout(saveProps, 800); });
    props.addEventListener('submit', (e) => { e.preventDefault(); saveProps(); });

    const rows = () => [...list.querySelectorAll('.edit-item')];

    function renumber() {
      const all = rows();
      all.forEach((li, i) => {
        const n = li.querySelector('.edit-n');
        n.value = String(i + 1);
        n.max = String(all.length);
      });
      $('edit-count').textContent = String(all.length);
    }

    async function saveOrder() {
      renumber();
      try {
        await post(base + '/order', { keys: rows().map((li) => li.dataset.key) }, true);
        say(props.dataset.tSaved);
      } catch { say(props.dataset.tFailed); }
    }

    // Puts a row at a place (0 is the first) and keeps the focus on what moved it.
    function moveTo(li, place, focus) {
      const all = rows().filter((r) => r !== li);
      const at = Math.max(0, Math.min(all.length, place));
      if (at >= all.length) list.append(li); else list.insertBefore(li, all[at]);
      if (focus) focus.focus();
      li.scrollIntoView({ block: 'nearest' });
      saveOrder();
    }

    list.addEventListener('click', async (e) => {
      const li = e.target.closest('.edit-item');
      if (!li) return;
      const at = rows().indexOf(li);
      if (e.target.closest('.edit-up') && at > 0) moveTo(li, at - 1, li.querySelector('.edit-up'));
      else if (e.target.closest('.edit-down') && at < rows().length - 1) moveTo(li, at + 1, li.querySelector('.edit-down'));
      else if (e.target.closest('.edit-remove')) {
        try {
          await post(base + '/remove', { keys: [li.dataset.key] }, true);
          li.remove();
          renumber();
          say(props.dataset.tSaved);
        } catch { say(props.dataset.tFailed); }
      }
    });

    list.addEventListener('change', async (e) => {
      const li = e.target.closest('.edit-item');
      if (!li) return;
      if (e.target.classList.contains('edit-n')) {
        const n = Math.round(Number(e.target.value));
        if (!Number.isFinite(n) || n < 1) { renumber(); return; }
        moveTo(li, n - 1, e.target);
        return;
      }
      if (e.target.matches('[data-choice-select]')) {
        const value = (name) => {
          const s = li.querySelector('select[name^="' + name + '-"]');
          return s && s.value !== 'project' ? s.value : '';
        };
        try {
          await post(base + '/picture', { key: li.dataset.key, transition: value('transition'), seconds: value('seconds') }, true);
          say(props.dataset.tSaved);
        } catch { say(props.dataset.tFailed); }
      }
    });
    // Enter in a number field moves at once instead of sending the page.
    list.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && e.target.classList.contains('edit-n')) { e.preventDefault(); e.target.blur(); }
    });
  }
})();
