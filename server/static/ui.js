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
    const opener = target.closest('[data-dialog]');
    if (opener) {
      const dialog = document.getElementById(opener.dataset.dialog);
      if (dialog && dialog.showModal) {
        dialog.querySelectorAll('.form-error').forEach((box) => { box.innerHTML = ''; });
        dialog.showModal();
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
      const box = dialog.querySelector('.form-error');
      if (box) {
        e.detail.shouldSwap = false;
        box.innerHTML = e.detail.serverResponse;
      }
      return;
    }
    if (status < 200 || status >= 300 || !form.matches('form[data-close-window]')) return;
    if (dialog.open) dialog.close();
    // A form that creates something starts empty the next time (data-reset).
    if (form.hasAttribute('data-reset')) form.reset();
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
})();
