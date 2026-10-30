// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// The fields of an album that are added by a "+": <div data-fields-editor data-catalog='[{"name":"DiscTotal","kind":"integer"}, ...]'> with a
// <textarea name="more"> that the server reads (one `Field=Value` per line). The script hides the textarea and keeps it in step with rows of an
// input each: text, a number or a date, as the kind of the field says (the catalog comes from the server). Without JavaScript the textarea stays.
(() => {
  const init = (box) => {
    if (box.dataset.ready) return;
    box.dataset.ready = '1';
    let catalog = [];
    try { catalog = JSON.parse(box.dataset.catalog || '[]'); } catch (_) { /* no catalog: the textarea stays */ }
    const area = box.querySelector('textarea[name=more]');
    const rows = box.querySelector('.fields-rows');
    const choose = box.querySelector('.fields-choose');
    const plus = box.querySelector('.fields-plus');
    if (!area || !rows || !choose || !plus || catalog.length === 0) return;
    const kindOf = (name) => (catalog.find((c) => c.name.toLowerCase() === name.toLowerCase()) || { kind: 'string' }).kind;
    const used = () => Array.from(rows.querySelectorAll('.field-row')).map((r) => r.dataset.name.toLowerCase());

    const sync = () => {
      area.value = Array.from(rows.querySelectorAll('.field-row'))
        .map((r) => [r.dataset.name, r.querySelector('input').value.trim()])
        .filter(([, v]) => v !== '')
        .map(([n, v]) => n + '=' + v.replace(/\r?\n/g, ' '))
        .join('\n');
    };
    const refill = () => {
      choose.replaceChildren();
      const first = document.createElement('option');
      first.value = '';
      first.textContent = box.dataset.choose || '';
      choose.append(first);
      const taken = used();
      for (const c of catalog) {
        if (taken.includes(c.name.toLowerCase())) continue;
        const o = document.createElement('option');
        o.value = c.name;
        o.textContent = c.name + (c.kind === 'string' ? '' : ' (' + c.kind + ')');
        choose.append(o);
      }
    };
    const addRow = (name, value) => {
      if (used().includes(name.toLowerCase())) return;
      const kind = kindOf(name);
      const row = document.createElement('div');
      row.className = 'field-row';
      row.dataset.name = name;
      const label = document.createElement('label');
      label.className = 'field-name';
      label.textContent = name;
      const input = document.createElement('input');
      input.name = 'field-value-' + name;
      // The input has the kind of the field: a number for a whole number, a date picker for a date, else text.
      if (kind === 'integer') { input.type = 'number'; input.step = '1'; input.min = '0'; input.inputMode = 'numeric'; }
      else if (kind === 'date') { input.type = 'date'; }
      else { input.type = 'text'; input.maxLength = 500; }
      input.value = value || '';
      input.addEventListener('input', sync);
      input.addEventListener('keydown', (e) => { if (e.key === 'Enter') e.preventDefault(); });
      label.append(input);
      const x = document.createElement('button');
      x.type = 'button';
      x.className = 'btn outline field-x';
      x.textContent = '×';
      x.title = x.ariaLabel = (box.dataset.remove || '') + ': ' + name;
      x.addEventListener('click', () => { row.remove(); sync(); refill(); });
      row.append(label, x);
      rows.append(row);
      refill();
      sync();
      input.focus();
    };
    plus.addEventListener('click', () => { if (choose.value) addRow(choose.value, ''); });
    choose.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); if (choose.value) addRow(choose.value, ''); } });
    // What the textarea holds already (the form drawn again) becomes rows.
    for (const line of area.value.split('\n')) {
      const at = line.indexOf('=');
      if (at > 0) addRow(line.slice(0, at).trim(), line.slice(at + 1).trim());
    }
    area.hidden = true;
    refill();
  };
  const all = (root) => (root.querySelectorAll ? root.querySelectorAll('[data-fields-editor]') : []).forEach(init);
  document.addEventListener('DOMContentLoaded', () => all(document));
  if (document.readyState !== 'loading') all(document);
  // The tags are loaded into the window when its tab is opened.
  document.body?.addEventListener('htmx:afterSettle', (e) => all(e.target || document));
})();
