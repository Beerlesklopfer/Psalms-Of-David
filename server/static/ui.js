// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// Small helpers for the administration pages. Everything also works without them:
// popup menus are <details> elements and the planner is a table of check boxes.
(() => {
  'use strict';
  document.documentElement.classList.add('js');

  // ---- Popup menus -------------------------------------------------------------------
  const open = () => document.querySelectorAll('details.popup[open]');

  // A click anywhere else closes them.
  document.addEventListener('click', (e) => {
    for (const p of open()) if (!p.contains(e.target)) p.open = false;
  });
  // Escape closes them and gives the focus back to the button that opened them.
  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    for (const p of open()) {
      p.open = false;
      const trigger = p.querySelector('summary');
      if (trigger) trigger.focus();
    }
  });
  // Only one is open at a time. "toggle" does not bubble, so listen while it travels down.
  document.addEventListener('toggle', (e) => {
    const t = e.target;
    if (!(t instanceof HTMLDetailsElement) || !t.classList.contains('popup') || !t.open) return;
    for (const p of open()) if (p !== t) p.open = false;
  }, true);

  // The Save of a window sits in the foot, outside the form it saves: while the form is sent the button wears the class
  // htmx puts on the form (the clock cursor and the dim look), and loses it when the answer is there, whatever the answer was.
  const saveOf = (e) => e.target && e.target.closest ? e.target.closest('dialog')?.querySelector('[data-save]') : null;
  document.body.addEventListener('htmx:beforeRequest', (e) => saveOf(e)?.classList.add('htmx-request'));
  document.body.addEventListener('htmx:afterRequest', (e) => saveOf(e)?.classList.remove('htmx-request'));

  // ---- Tabs of a window: <div class="tabs"> with buttons data-tab="x", and panels data-panel="x" in the window ----
  const showTab = (dialog, name) => {
    if (!dialog) return;
    dialog.querySelectorAll('.tabs [data-tab]').forEach((t) => t.setAttribute('aria-selected', String(t.dataset.tab === name)));
    dialog.querySelectorAll('[data-panel]').forEach((p) => { p.hidden = p.dataset.panel !== name; });
    // The one Save of the window belongs to the form of the open tab; a tab without a form (filing) has none.
    const save = dialog.querySelector('[data-save]');
    const tab = dialog.querySelector('.tabs [data-tab="' + name + '"]');
    if (save && tab) {
      const form = tab.dataset.form || '';
      save.hidden = form === '';
      if (form) save.setAttribute('form', form);
    }
  };

  // ---- Windows (<dialog>) --------------------------------------------------------------
  // A button with data-dialog="<id>" opens that dialog as a window. A link with
  // data-planner loads the planner of a user into its window first, and empties the
  // window again when it is closed, so that two planners never share element ids.
  document.addEventListener('click', (e) => {
    const target = e.target;
    if (!(target instanceof Element)) return;

    // A click on the dark area around a window closes it.
    if (target instanceof HTMLDialogElement && target.classList.contains('window')) {
      target.close();
      return;
    }
    // Cancel: close the window around the button, changing nothing.
    const closer = target.closest('[data-close]');
    if (closer) {
      const dialog = closer.closest('dialog');
      if (dialog) dialog.close();
      return;
    }
    // A row of folders (import settings): the button takes the row out of the form.
    const removeRow = target.closest('[data-remove-row]');
    if (removeRow) {
      removeRow.closest('.root-row')?.remove();
      return;
    }
    // The folder dialog: a button with data-pick-folder puts the path into the input named by data-target and closes the dialog.
    const pick = target.closest('[data-pick-folder]');
    if (pick) {
      const input = document.getElementById(pick.dataset.target);
      if (input) {
        input.value = pick.dataset.pickFolder;
        input.dispatchEvent(new Event('input', { bubbles: true }));
      }
      pick.closest('dialog')?.close();
      return;
    }
    // The tabs of a window (the window of a book): a tab shows its panel and hides the others.
    const tab = target.closest('.tabs [data-tab]');
    if (tab) {
      showTab(tab.closest('dialog'), tab.dataset.tab);
      return;
    }
    const opener = target.closest('[data-dialog]');
    if (opener) {
      const dialog = document.getElementById(opener.dataset.dialog);
      if (dialog && dialog.showModal) {
        dialog.querySelectorAll('.form-error').forEach((box) => { box.innerHTML = ''; });
        dialog.showModal();
        if (opener.dataset.tab) showTab(dialog, opener.dataset.tab);
      }
      return;
    }
    const planner = target.closest('a[data-planner]');
    if (planner && window.htmx) {
      e.preventDefault();
      const id = planner.dataset.planner;
      const dialog = document.getElementById('dlg-planner-' + id);
      if (!dialog || !dialog.showModal) {
        window.location.href = planner.href;
        return;
      }
      window.htmx
        .ajax('GET', planner.dataset.url, { target: '#dlg-planner-body-' + id, swap: 'innerHTML' })
        .then(() => dialog.showModal());
    }
  });
  // A form marked data-close-window closes its window once the server has accepted it.
  // A refusal leaves the window open, so the reason can be read and the input corrected.
  // This has to happen before the swap: the answer often replaces the very form (or the
  // row around it) that sent the request, and an event of a removed element never arrives.
  document.body.addEventListener('htmx:beforeSwap', (e) => {
    const config = e.detail && e.detail.requestConfig;
    const form = config && config.elt;
    if (!form || !form.closest) return;
    const dialog = form.closest('dialog');
    if (!dialog) return;
    const status = e.detail.xhr ? e.detail.xhr.status : 0;
    // A refusal is shown inside the window: the message area of the page lies behind it.
    if (status >= 400) {
      const box = form.querySelector('.form-error') || dialog.querySelector('.form-error');
      if (box) {
        e.detail.shouldSwap = false;
        box.innerHTML = e.detail.serverResponse;
      }
      // The input the server names gets a ring (see .is-invalid); one that was marked before is cleared first.
      dialog.querySelectorAll('.is-invalid').forEach((i) => i.classList.remove('is-invalid'));
      const named = e.detail.xhr ? e.detail.xhr.getResponseHeader('X-Invalid-Field') : null;
      const bad = named ? form.elements.namedItem(named) : null;
      if (bad && bad.classList) {
        bad.classList.add('is-invalid');
        bad.setAttribute('aria-invalid', 'true');
        bad.focus();
      }
      return;
    }
    if (status < 200 || status >= 300 || !form.matches('form[data-close-window]')) return;
    if (dialog.open) dialog.close();
    // A form that creates something starts empty the next time (data-reset).
    if (form.hasAttribute('data-reset')) form.reset();
  });
  // Typing in a marked input takes the ring off.
  document.addEventListener('input', (e) => {
    const t = e.target;
    if (t instanceof HTMLElement && t.classList.contains('is-invalid')) {
      t.classList.remove('is-invalid');
      t.removeAttribute('aria-invalid');
    }
  });
  document.addEventListener('close', (e) => {
    const d = e.target;
    if (d instanceof HTMLDialogElement && d.classList.contains('planner-window')) {
      const body = d.querySelector('.window-body');
      if (body) body.innerHTML = '';
    }
  }, true);



  // ---- Progress of a large upload ----------------------------------------------------
  // An archive can be several GB. htmx reports how much has gone out; a block marked
  // data-progress-for="<form id>" shows it and is hidden again when the request ends.
  const progressOf = (form) => form && form.id ? document.querySelector('[data-progress-for="' + form.id + '"]') : null;
  document.body.addEventListener('htmx:xhr:loadstart', (e) => {
    const box = progressOf(e.target);
    if (!box) return;
    box.hidden = false;
    box.querySelector('progress').value = 0;
    box.querySelector('.progress-percent').textContent = '0%';
  });
  document.body.addEventListener('htmx:xhr:progress', (e) => {
    const box = progressOf(e.target);
    if (!box || !e.detail.lengthComputable) return;
    const percent = Math.floor((e.detail.loaded / e.detail.total) * 100);
    box.querySelector('progress').value = percent;
    box.querySelector('.progress-percent').textContent = percent + '%';
  });
  document.body.addEventListener('htmx:xhr:loadend', (e) => {
    const box = progressOf(e.target);
    if (box) box.hidden = true;
  });

  // ---- Pictures: from a file, or pasted from the clipboard ---------------------------
  // The picture ends up in the file input of the form either way, so the form is sent as usual.
  const takePicture = (input, file) => {
    const list = new DataTransfer();
    list.items.add(file);
    input.files = list.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
  };
  document.addEventListener('change', (e) => {
    const input = e.target;
    if (!(input instanceof HTMLInputElement) || !input.hasAttribute('data-image-input') || !input.files[0]) return;
    const preview = document.getElementById(input.dataset.preview);
    if (!preview) return;
    const reader = new FileReader();
    reader.onload = () => { preview.src = reader.result; };
    reader.readAsDataURL(input.files[0]);
  });
  document.addEventListener('click', async (e) => {
    const button = e.target instanceof Element ? e.target.closest('[data-paste-image]') : null;
    if (!button) return;
    const input = document.getElementById(button.dataset.pasteImage);
    const hint = document.getElementById(button.dataset.hint);
    const say = (text) => { if (hint) hint.textContent = text; };
    try {
      for (const item of await navigator.clipboard.read()) {
        const type = item.types.find((t) => t.startsWith('image/'));
        if (!type) continue;
        const blob = await item.getType(type);
        takePicture(input, new File([blob], 'clipboard.' + type.split('/')[1], { type }));
        return;
      }
      say(button.dataset.none);
    } catch (err) {
      // Reading the clipboard needs a secure page and the user's permission; Ctrl+V always works.
      say(button.dataset.denied);
    }
  });
  document.addEventListener('paste', (e) => {
    const dialog = e.target instanceof Element ? e.target.closest('dialog[open]') : document.querySelector('dialog[open]');
    const open = dialog || document.querySelector('dialog[open]');
    const input = open && open.querySelector('input[data-image-input]');
    if (!input || !e.clipboardData) return;
    const file = Array.from(e.clipboardData.files).find((f) => f.type.startsWith('image/'));
    if (!file) return;
    e.preventDefault();
    takePicture(input, file);
  });

  // ---- Weekly planner ----------------------------------------------------------------
  // A grid of quarter hours, days across. Drag over cells to allow them (or to block
  // them, if the first cell was allowed); click a day or an hour to switch it as a whole,
  // the corner for everything. The check boxes stay the truth that is sent to the server.
  const box = (td) => td.querySelector('input[type=checkbox]');
  const cells = (table) => Array.from(table.querySelectorAll('td.slot'));
  const day = (td) => Number(td.dataset.day);
  const slot = (td) => Number(td.dataset.slot);
  const cellAt = (el) => (el && el.closest ? el.closest('td.slot') : null);

  const set = (td, on) => {
    box(td).checked = on;
    td.classList.toggle('on', on);
  };

  // The number of allowed hours, in the same form as the server writes it (12.25).
  const total = (table) => {
    const out = document.querySelector('.planner-total');
    if (!out) return;
    const n = cells(table).filter((td) => box(td).checked).length;
    out.textContent = `${Math.floor(n / 4)}.${String((n % 4) * 25).padStart(2, '0')}`;
  };

  let drag = null;

  document.addEventListener('pointerdown', (e) => {
    const td = cellAt(e.target);
    if (!td || e.button !== 0) return;
    const table = td.closest('table.planner');
    if (!table) return;
    drag = {
      table,
      start: td,
      mode: !box(td).checked,
      before: new Map(cells(table).map((c) => [c, box(c).checked])),
    };
    // Keep receiving moves when the pointer leaves the table.
    try {
      table.setPointerCapture(e.pointerId);
    } catch (_) {
      // A pointer that is already gone (or a synthetic one): moves then come from the document.
    }
    paint(td);
    e.preventDefault();
  });

  document.addEventListener('pointermove', (e) => {
    if (!drag) return;
    const td = cellAt(document.elementFromPoint(e.clientX, e.clientY));
    if (td && drag.table.contains(td)) paint(td);
  });

  const end = () => {
    if (!drag) return;
    total(drag.table);
    drag = null;
  };
  document.addEventListener('pointerup', end);
  document.addEventListener('pointercancel', end);

  // The rectangle between the first cell and this one gets the new state; every cell
  // outside it keeps what it had when the drag began, so shrinking the rectangle undoes.
  function paint(to) {
    const d0 = Math.min(day(drag.start), day(to)), d1 = Math.max(day(drag.start), day(to));
    const s0 = Math.min(slot(drag.start), slot(to)), s1 = Math.max(slot(drag.start), slot(to));
    for (const c of cells(drag.table)) {
      const inside = day(c) >= d0 && day(c) <= d1 && slot(c) >= s0 && slot(c) <= s1;
      set(c, inside ? drag.mode : drag.before.get(c));
    }
  }

  // A day, an hour or the corner as a whole: all on, unless they all are on already.
  document.addEventListener('click', (e) => {
    const head = e.target.closest ? e.target.closest('table.planner th[data-day], table.planner th[data-hour], table.planner th.corner') : null;
    if (!head) return;
    const table = head.closest('table.planner');
    let group = cells(table);
    if (head.dataset.day !== undefined) group = group.filter((c) => day(c) === Number(head.dataset.day));
    if (head.dataset.hour !== undefined) group = group.filter((c) => Math.floor(slot(c) / 4) === Number(head.dataset.hour));
    const allOn = group.every((c) => box(c).checked);
    for (const c of group) set(c, !allOn);
    total(table);
  });

  // The keyboard: a check box that changes by Space keeps its cell in step.
  document.addEventListener('change', (e) => {
    const td = cellAt(e.target);
    if (!td) return;
    td.classList.toggle('on', box(td).checked);
    total(td.closest('table.planner'));
  });

  // A select marked data-autosubmit sends its form when it changes (the size of a page).
  document.addEventListener('change', (e) => {
    const sel = e.target;
    if (sel instanceof HTMLSelectElement && sel.hasAttribute('data-autosubmit') && sel.form) sel.form.requestSubmit();
  });

  // ---- The syntax of a search (the same as `search.rs` of the server) -----------------------
  // Words are found anywhere, ignoring case; `*` is any run of characters and `?` one character; between slashes the text is a regular
  // expression. Returns a function text -> bool; an expression that does not compile finds nothing (the server says why after the submit).
  const searchMatcher = (raw) => {
    const q = (raw || '').trim();
    if (q === '') return () => true;
    const compile = (source) => { try { return new RegExp(source, 'i'); } catch (_) { return null; } };
    let re = null;
    if (q.length > 2 && q.startsWith('/') && q.endsWith('/')) {
      re = compile(q.slice(1, -1));
      return re ? (text) => re.test(text) : () => false;
    }
    if (q.includes('*') || q.includes('?')) {
      const source = Array.from(q).map((c) => (c === '*' ? '.*' : c === '?' ? '.' : c.replace(/[.*+?^${}()|[\]\\\/]/g, '\\$&'))).join('');
      re = compile(source);
      return re ? (text) => re.test(text) : () => false;
    }
    const needle = q.toLowerCase();
    return (text) => text.toLowerCase().includes(needle);
  };

  // ---- Filter of a table while typing: <input data-filter-rows="table rows selector"> -------
  // The rows carry their text in data-search (lower case). Without JavaScript the form submits and the server filters.
  document.querySelectorAll('input[data-filter-rows]').forEach((input) => {
    const rows = () => document.querySelectorAll(input.dataset.filterRows);
    const apply = () => {
      const match = searchMatcher(input.value);
      for (const row of rows()) row.hidden = !match(row.dataset.search || '');
    };
    input.addEventListener('input', apply);
    input.form?.addEventListener('submit', (e) => { if (rows().length) e.preventDefault(); });
    apply();
  });

  // ---- Copy button: puts the text of data-copy on the clipboard ---------------------
  // Delegated, because htmx swaps the panel that holds the button.
  document.addEventListener('click', async (e) => {
    const button = e.target.closest('[data-copy]');
    if (!button) return;
    const text = button.dataset.copy;
    try {
      await navigator.clipboard.writeText(text);
    } catch (_) {
      // No secure page or no permission: select the text so Ctrl+C works.
      const box = button.closest('section, div')?.querySelector('pre');
      if (box) getSelection().selectAllChildren(box);
      return;
    }
    const label = button.innerHTML;
    button.textContent = button.dataset.copied || 'OK';
    setTimeout(() => { button.innerHTML = label; }, 1500);
  });

  // ---- Tree of the media lists: a level folds the rows below it ----------------------------
  // The rows carry the ids of their levels (data-parents); without JavaScript everything is open.
  // The search is the server's (it looks at all pages and answers with the books it found, see the search form of the
  // media pages), so a search shows everything it found: the folds are not applied while there is a search.
  // The list is swapped by htmx on every search, therefore the click is delegated and the rows are drawn again after a swap.
  const folded = new Set();
  const refreshTree = () => {
    const tree = document.getElementById('book-list');
    if (!tree) return;
    const searching = (document.getElementById('book-search')?.value || '').trim() !== '';
    const parentsOf = (row) => (row.dataset.parents ? row.dataset.parents.split(' ') : []);
    for (const row of tree.querySelectorAll('tbody tr')) {
      const hidden = !searching && parentsOf(row).some((p) => folded.has(p));
      row.hidden = hidden;
      if (row.classList.contains('tree-node')) {
        row.querySelector('.tree-toggle')?.setAttribute('aria-expanded', folded.has(row.dataset.node) && !searching ? 'false' : 'true');
      }
    }
  };
  document.addEventListener('click', (e) => {
    const button = e.target.closest('#book-list .tree-toggle');
    if (!button) return;
    const id = button.closest('tr').dataset.node;
    if (folded.has(id)) folded.delete(id); else folded.add(id);
    refreshTree();
  });
  document.body.addEventListener('htmx:afterSwap', (e) => {
    if (e.target && (e.target.id === 'media-results' || e.target.querySelector?.('#book-list'))) refreshTree();
  });
  refreshTree();

  // ---- Drop files, folders and archives on the import page -----------------------------
  // An archive is sent as an upload of its own (upload.create); audio files, pictures and folders go together in
  // one upload (upload.files) with the place of each file inside the dropped folder.
  const ARCHIVE = /\.(zip|tar|gz|tgz|bz2|tbz2|xz|txz|rar|7z)$/i;
  const readAll = (reader) => new Promise((resolve, reject) => {
    const all = [];
    const next = () => reader.readEntries((batch) => { if (batch.length) { all.push(...batch); next(); } else resolve(all); }, reject);
    next();
  });
  const fileOf = (entry) => new Promise((resolve, reject) => entry.file(resolve, reject));
  // Every file below the dropped entries, with its path inside the dropped folder.
  const walk = async (entry, out) => {
    if (entry.isFile) {
      out.push({ path: entry.fullPath.replace(/^\//, ''), file: await fileOf(entry) });
    } else if (entry.isDirectory) {
      for (const child of await readAll(entry.createReader())) await walk(child, out);
    }
  };
  const send = (url, form, csrf, zone) => new Promise((resolve, reject) => {
    const box = zone.querySelector('[data-drop-progress]');
    const xhr = new XMLHttpRequest();
    xhr.open('POST', url);
    xhr.setRequestHeader('X-CSRF-Token', csrf);
    xhr.setRequestHeader('HX-Request', 'true');
    xhr.upload.onprogress = (e) => {
      if (!box || !e.lengthComputable) return;
      const percent = Math.floor((e.loaded / e.total) * 100);
      box.querySelector('progress').value = percent;
      box.querySelector('.progress-percent').textContent = percent + '%';
    };
    xhr.onload = () => (xhr.status >= 200 && xhr.status < 300 ? resolve(xhr.responseText) : reject(xhr.responseText || String(xhr.status)));
    xhr.onerror = () => reject('network');
    xhr.send(form);
  });
  const zones = document.querySelectorAll('[data-dropzone]');
  zones.forEach((zone) => {
    zone.addEventListener('dragover', (e) => { e.preventDefault(); zone.classList.add('over'); });
    zone.addEventListener('dragleave', () => zone.classList.remove('over'));
    zone.addEventListener('drop', async (e) => {
      e.preventDefault();
      zone.classList.remove('over');
      const error = zone.querySelector('[data-drop-error]');
      const box = zone.querySelector('[data-drop-progress]');
      error.textContent = '';
      // The entries must be taken before the first await: the list is emptied afterwards.
      const entries = Array.from(e.dataTransfer.items || []).map((i) => (i.webkitGetAsEntry ? i.webkitGetAsEntry() : null));
      const plain = Array.from(e.dataTransfer.files || []);
      const found = [];
      try {
        if (entries.some(Boolean)) { for (const entry of entries) if (entry) await walk(entry, found); }
        else plain.forEach((f) => found.push({ path: f.name, file: f }));
      } catch (err) { error.textContent = String(err); return; }
      const kind = zone.querySelector('[data-drop-kind]').value;
      const csrf = zone.dataset.csrf;
      // Only a file straight in the dropped area can be an archive; inside a folder it is just a file.
      const archives = found.filter((f) => !f.path.includes('/') && ARCHIVE.test(f.path));
      const loose = found.filter((f) => !archives.includes(f));
      box.hidden = false;
      let html = null;
      try {
        for (const a of archives) {
          const form = new FormData();
          form.append('kind', kind);
          form.append('archive', a.file, a.file.name);
          html = await send('/admin/rpc/upload.create', form, csrf, zone);
        }
        if (loose.length) {
          const form = new FormData();
          form.append('kind', kind);
          for (const f of loose) { form.append('path', f.path); form.append('file', f.file, f.file.name); }
          html = await send('/admin/rpc/upload.files', form, csrf, zone);
        }
      } catch (err) {
        const doc = new DOMParser().parseFromString(String(err), 'text/html');
        error.textContent = doc.body.textContent.trim() || String(err);
      }
      box.hidden = true;
      const queue = document.querySelector(zone.dataset.queue);
      if (html && queue && window.htmx) window.htmx.ajax('GET', '/admin/rpc/upload.list', { target: zone.dataset.queue, swap: 'outerHTML' });
    });
  });

  // ---- CD import: the list of tracks and the buttons that insert a placeholder -------------------------------------
  // Without this script every box of the list stays ticked, the buttons stay hidden and the lines of placeholders stay
  // visible as plain text. All of it works by data attributes, so it needs no inline script.
  const cdTracks = (el) => el.closest('[data-cd-tracks]');
  const cdSummary = (root) => {
    if (!root) return;
    const boxes = Array.from(root.querySelectorAll('input[name=only]'));
    const on = boxes.filter((b) => b.checked);
    const seconds = on.reduce((sum, b) => sum + (parseInt(b.dataset.seconds, 10) || 0), 0);
    const line = root.querySelector('[data-cd-summary]');
    if (line) {
      const time = Math.floor(seconds / 60) + ':' + String(seconds % 60).padStart(2, '0');
      line.textContent = (line.dataset.cdFormat || '').replace('{n}', on.length).replace('{m}', boxes.length).replace('{time}', time);
    }
    const form = root.closest('form');
    const go = form && form.querySelector('[data-cd-import]');
    if (go) go.disabled = on.length === 0 || form.hasAttribute('data-cd-busy');
  };
  const cdPrepare = () => {
    document.querySelectorAll('[data-cd-js]').forEach((e) => { e.hidden = false; });
    document.querySelectorAll('[data-cd-nojs]').forEach((e) => { e.hidden = true; });
    document.querySelectorAll('[data-cd-hint]').forEach((h) => { if (!h.closest('[data-cd-field]').contains(document.activeElement)) h.hidden = true; });
  };
  document.addEventListener('DOMContentLoaded', cdPrepare);
  document.addEventListener('htmx:afterSwap', cdPrepare);
  document.addEventListener('click', (e) => {
    const pick = e.target.closest && e.target.closest('[data-cd-select]');
    if (pick) {
      const root = cdTracks(pick);
      if (!root) return;
      root.querySelectorAll('input[name=only]').forEach((b) => {
        b.checked = pick.dataset.cdSelect === 'all' ? true : pick.dataset.cdSelect === 'none' ? false : !b.checked;
      });
      cdSummary(root);
      return;
    }
    const insert = e.target.closest && e.target.closest('[data-insert]');
    if (insert) {
      const field = insert.closest('[data-cd-field]');
      const input = field && field.querySelector('input[type=text], input:not([type])');
      if (!input) return;
      const text = insert.dataset.insert;
      const from = input.selectionStart ?? input.value.length;
      const to = input.selectionEnd ?? from;
      input.value = input.value.slice(0, from) + text + input.value.slice(to);
      input.setSelectionRange(from + text.length, from + text.length);
      input.focus();
      // The preview and the checks listen to "input".
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
  });
  document.addEventListener('change', (e) => {
    if (e.target.matches && e.target.matches('input[name=only]')) cdSummary(cdTracks(e.target));
  });
  document.addEventListener('focusin', (e) => {
    const field = e.target.closest && e.target.closest('[data-cd-field]');
    document.querySelectorAll('[data-cd-hint]').forEach((h) => { h.hidden = h.closest('[data-cd-field]') !== field; });
  });
  document.addEventListener('focusout', (e) => {
    const field = e.target.closest && e.target.closest('[data-cd-field]');
    if (field && !(e.relatedTarget && field.contains(e.relatedTarget))) {
      const h = field.querySelector('[data-cd-hint]');
      if (h) h.hidden = true;
    }
  });

  // ---- A plain form that asks first: <form data-confirm="Really?"> ------------------------
  document.addEventListener('submit', (e) => {
    const text = e.target.closest && e.target.closest('form[data-confirm]')?.dataset.confirm;
    if (text && !window.confirm(text)) e.preventDefault();
  });
  // The label filter: the ticked boxes become the comma separated value of the hidden field (before the form sends).
  document.addEventListener('change', (e) => {
    const box = e.target;
    if (!box.matches || !box.matches('.label-pick input[data-label]')) return;
    const form = box.closest('form');
    form.elements.label.value = [...form.querySelectorAll('.label-pick input[data-label]:checked')].map((b) => b.dataset.label).join(',');
  });
  // The labels of the titles: every change of an entry rewrites the hidden field `track_labels` (one line `track id=labels` per title).
  document.addEventListener('input', (e) => {
    const box = e.target.closest && e.target.closest('[data-track-labels]');
    if (!box || !e.target.matches('input[data-track]')) return;
    box.querySelector('input[name="track_labels"]').value = [...box.querySelectorAll('input[data-track]')].map((i) => i.dataset.track + '=' + i.value).join('\n');
  });
})();
