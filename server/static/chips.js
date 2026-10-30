// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// Chips for a list of words (the labels of a book): <input type="text" data-chips list="words" value="Gebet, Schule"> holds the words
// separated by commas. The script hides it behind a row of chips (each with a cross to take it away), an entry with the suggestions of the
// datalist and the button "Add". Without JavaScript the plain field stays and the words are typed with commas.
// The texts come from the attributes data-add, data-remove and data-placeholder, so the page translates them.
(() => {
  const clean = (text) => text.split(/[,;]/).map((w) => w.trim().replace(/\s+/g, ' ')).filter(Boolean);
  const init = (input) => {
    if (input.dataset.chipsReady) return;
    input.dataset.chipsReady = '1';
    const words = [];
    const addWords = (text) => {
      let changed = false;
      for (const w of clean(text)) {
        if (!words.some((x) => x.toLowerCase() === w.toLowerCase())) { words.push(w); changed = true; }
      }
      return changed;
    };
    addWords(input.value);

    const box = document.createElement('div');
    box.className = 'chips';
    const list = document.createElement('span');
    list.className = 'chips-list';
    const entry = document.createElement('input');
    entry.type = 'text';
    entry.className = 'chips-entry';
    entry.maxLength = 40;
    entry.autocomplete = 'off';
    entry.spellcheck = false;
    entry.placeholder = input.dataset.placeholder || '';
    if (input.getAttribute('list')) entry.setAttribute('list', input.getAttribute('list'));
    const add = document.createElement('button');
    add.type = 'button';
    add.className = 'btn outline chips-add';
    add.textContent = input.dataset.add || '+';

    const sync = () => {
      input.value = words.join(', ');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    };
    const render = () => {
      list.replaceChildren();
      for (const w of words) {
        const chip = document.createElement('button');
        chip.type = 'button';
        chip.className = 'chip';
        chip.title = (input.dataset.remove || '') + ': ' + w;
        chip.setAttribute('aria-label', (input.dataset.remove || '') + ': ' + w);
        const label = document.createElement('span');
        label.textContent = w;
        const x = document.createElement('span');
        x.className = 'chip-x';
        x.setAttribute('aria-hidden', 'true');
        x.textContent = '\u00d7';
        chip.append(label, x);
        chip.addEventListener('click', () => {
          words.splice(words.indexOf(w), 1);
          render();
          sync();
        });
        list.append(chip);
      }
    };
    const commit = () => {
      if (entry.value.trim() && addWords(entry.value)) { render(); sync(); }
      entry.value = '';
    };
    entry.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' || e.key === ',' || e.key === ';') {
        e.preventDefault();
        commit();
      } else if (e.key === 'Backspace' && entry.value === '' && words.length) {
        words.pop();
        render();
        sync();
      }
    });
    // A word picked from the suggestions arrives as an "input" event without a key.
    entry.addEventListener('change', commit);
    add.addEventListener('click', () => { commit(); entry.focus(); });
    // What was typed and not yet added goes with the form (before htmx sends it).
    input.form?.addEventListener('submit', commit, true);

    // The suggestions: the labels that are in use already and fit what is typed (asked of the server, so a label that was set a moment ago is
    // there too). Arrow keys choose, Enter or a click takes, Escape closes.
    const menu = document.createElement('ul');
    menu.className = 'chips-suggest';
    menu.setAttribute('role', 'listbox');
    menu.hidden = true;
    let shown = [];
    let active = -1;
    let timer = 0;
    const close = () => { menu.hidden = true; shown = []; active = -1; };
    const mark = () => menu.querySelectorAll('li').forEach((li, i) => li.setAttribute('aria-selected', String(i === active)));
    const take = (w) => { addWords(w); render(); sync(); entry.value = ''; close(); entry.focus(); };
    const open = (names) => {
      shown = names.filter((n) => !words.some((w) => w.toLowerCase() === n.toLowerCase()));
      menu.replaceChildren();
      for (const n of shown) {
        const li = document.createElement('li');
        li.setAttribute('role', 'option');
        li.textContent = n;
        // mousedown, not click: the entry would lose the focus first and close the list.
        li.addEventListener('mousedown', (e) => { e.preventDefault(); take(n); });
        menu.append(li);
      }
      active = -1;
      menu.hidden = shown.length === 0;
    };
    const ask = () => {
      const url = input.dataset.suggest;
      if (!url) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(async () => {
        try {
          const r = await fetch(url + '?q=' + encodeURIComponent(entry.value.trim()), { credentials: 'same-origin', headers: { Accept: 'application/json' } });
          if (r.ok) open(await r.json());
        } catch (_) { /* no suggestions, the word can still be typed */ }
      }, 120);
    };
    entry.addEventListener('input', ask);
    entry.addEventListener('focus', ask);
    entry.addEventListener('blur', () => window.setTimeout(close, 120));
    entry.addEventListener('keydown', (e) => {
      if (menu.hidden || shown.length === 0) return;
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        active = (active + (e.key === 'ArrowDown' ? 1 : -1) + shown.length) % shown.length;
        mark();
      } else if (e.key === 'Enter' && active >= 0) {
        // The chosen suggestion, not the text typed so far (the other handler would add that).
        e.preventDefault();
        e.stopImmediatePropagation();
        take(shown[active]);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        close();
      }
    }, true);

    box.append(list, entry, add, menu);
    input.type = 'hidden';
    input.after(box);
    render();
  };
  const all = (root) => (root.querySelectorAll ? root.querySelectorAll('input[data-chips]') : []).forEach(init);
  document.addEventListener('DOMContentLoaded', () => all(document));
  if (document.readyState !== 'loading') all(document);
  document.body?.addEventListener('htmx:afterSettle', (e) => all(e.target || document));
})();
