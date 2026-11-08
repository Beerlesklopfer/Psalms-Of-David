// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// The slideshow of the browser (docs/spec-pictures.md, step 2; templates/pictures.html). It is the model of the slideshow in the apps
// (project owner, 2026-11-08): made for the phone first. A tap shows the controls, a swipe goes forward or back, the keys do the same on a
// computer (arrows, space, Escape). The choices are kept in this browser (localStorage), nothing else is.
// The same script plays a project of a slideshow (src/show_project.rs): opened from the disk, it reads `window.DAVID_SHOW` (show.js)
// and its files lie next to it, so it needs neither the server nor a network.
(() => {
  'use strict';
  const form = document.getElementById('show-form');
  const show = document.getElementById('show');
  const project = window.DAVID_SHOW;
  const offline = !form && !!project;
  if (!show || (!form && !offline)) return;

  const $ = (id) => document.getElementById(id);
  const stage = $('show-stage');
  const layers = [$('show-a'), $('show-b')];
  const video = $('show-video');
  const music = $('show-music');
  const caption = $('show-caption');
  const dateEl = $('show-date');
  const placeEl = $('show-place');
  const toastEl = $('show-toast');
  const lang = show.dataset.lang || 'en';
  const placesOn = offline || show.dataset.places === 'true';
  const KEY = 'david.slideshow';
  const FADE_MS = 1000;

  // ---- The choices -------------------------------------------------------------------------------------------------------------------

  const fields = ['from', 'to', 'order', 'seconds', 'transition', 'videos', 'info', 'music'];

  function choices() {
    const c = { sources: [...form.querySelectorAll('input[name=source]:checked')].map((i) => i.value) };
    for (const f of fields) c[f] = form.elements[f] ? form.elements[f].value : '';
    return c;
  }

  function restore() {
    let saved = null;
    try { saved = JSON.parse(localStorage.getItem(KEY) || 'null'); } catch { saved = null; }
    if (!saved) return;
    if (Array.isArray(saved.sources) && saved.sources.length) {
      form.querySelectorAll('input[name=source]').forEach((i) => { i.checked = saved.sources.includes(i.value); });
    }
    for (const f of fields) {
      const el = form.elements[f];
      if (el && typeof saved[f] === 'string' && [...el.options].some((o) => o.value === saved[f])) {
        el.value = saved[f];
        el.dispatchEvent(new Event('change', { bubbles: true }));
      }
    }
  }

  function remember(c) {
    try { localStorage.setItem(KEY, JSON.stringify(c)); } catch { /* a private window: the choices are not kept */ }
  }

  function query(c) {
    const q = new URLSearchParams();
    for (const s of c.sources) q.append('source', s);
    if (c.from) q.set('from', String(new Date(Number(c.from), 0, 1).getTime()));
    if (c.to) q.set('to', String(new Date(Number(c.to) + 1, 0, 1).getTime() - 1));
    if (c.videos === 'on') q.set('videos', 'on');
    return q;
  }

  async function fetchList(c) {
    if (!c.sources.length) return [];
    const r = await fetch('/pictures/list?' + query(c), { credentials: 'same-origin' });
    if (!r.ok) throw new Error(String(r.status));
    return (await r.json()).pictures || [];
  }

  const countEl = $('show-count');
  const errorEl = $('show-error');
  let counting = 0;
  async function recount() {
    const mine = ++counting;
    try {
      const list = await fetchList(choices());
      if (mine === counting) countEl.textContent = show.dataset.tCount.replace('{n}', String(list.length));
    } catch {
      if (mine === counting) countEl.textContent = '';
    }
  }

  if (form) {
    restore();
    recount();
    form.addEventListener('change', recount);
    $('show-save').addEventListener('click', () => {
      // The project: the same choice as a file that plays without the server (a download; the page stays).
      const c = choices();
      remember(c);
      if (!c.sources.length) { errorEl.textContent = show.dataset.tEmpty; errorEl.hidden = false; return; }
      errorEl.hidden = true;
      const q = query(c);
      for (const f of ['order', 'seconds', 'transition', 'videos', 'info', 'music']) if (c[f]) q.set(f, c[f]);
      window.location.href = '/pictures/project?' + q;
    });

    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      errorEl.hidden = true;
      const c = choices();
      remember(c);
      let list;
      try {
        list = await fetchList(c);
      } catch {
        errorEl.textContent = show.dataset.tFailed;
        errorEl.hidden = false;
        return;
      }
      if (!list.length) {
        errorEl.textContent = show.dataset.tEmpty;
        errorEl.hidden = false;
        return;
      }
      if (c.order === 'random') shuffle(list);
      start(list, c);
    });
  }

  function shuffle(a) {
    for (let i = a.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [a[i], a[j]] = [a[j], a[i]];
    }
    return a;
  }

  if (offline) {
    const count = $('project-count');
    const broken = !Array.isArray(project.pictures) || project.format !== 'david-slideshow' || project.version > 1;
    if (broken) {
      $('project-error').hidden = false;
      $('project-start').disabled = true;
    } else {
      count.textContent = show.dataset.tCount.replace('{n}', String(project.pictures.length));
      $('project-start').addEventListener('click', () => {
        const list = project.pictures.slice();
        const c = Object.assign({}, project.settings, { seconds: String(project.settings.seconds), music: (project.music || []).length ? 'project' : '' });
        if (c.order === 'random') shuffle(list);
        start(list, c);
      });
    }
  }

  // ---- The show ----------------------------------------------------------------------------------------------------------------------

  let items = [];
  let opts = null;
  let at = 0;
  let front = 0;
  let timer = 0;
  let paused = false;
  let running = false;
  let wake = null;
  let step = 0;
  const infos = new Map();

  const url = (key, what) => '/pictures/' + encodeURIComponent(key) + '/' + what;
  // Where a picture and a video come from: the server, or the files of the project next to the page.
  const screenOf = (item) => (offline ? item.image : url(item.key, 'screen'));
  const videoOf = (item) => (offline ? item.video : url(item.key, 'file'));

  async function start(list, c) {
    items = list;
    opts = c;
    at = 0;
    paused = false;
    running = true;
    show.hidden = false;
    document.body.classList.add('show-open');
    caption.hidden = c.info === 'none';
    try { if (show.requestFullscreen) await show.requestFullscreen(); } catch { /* not allowed here: the show fills the window */ }
    try { if (navigator.wakeLock) wake = await navigator.wakeLock.request('screen'); } catch { wake = null; }
    if (c.music) startMusic(c.music);
    display(0, 1);
    flashControls();
  }

  function stop() {
    if (!running) return;
    running = false;
    clearTimeout(timer);
    video.pause();
    video.removeAttribute('src');
    video.load();
    music.pause();
    music.removeAttribute('src');
    layers.forEach((l) => { l.classList.remove('on', 'kb', 'cut'); l.removeAttribute('src'); });
    show.hidden = true;
    document.body.classList.remove('show-open');
    if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
    if (wake) { wake.release().catch(() => {}); wake = null; }
  }

  function transitionOf() {
    const t = opts.transition;
    if (t !== 'random') return t;
    return ['fade', 'zoom', 'cut'][Math.floor(Math.random() * 3)];
  }

  function schedule(ms) {
    clearTimeout(timer);
    if (!paused && running) timer = setTimeout(() => go(1), ms);
  }

  function go(dir) {
    if (!items.length) return;
    at = (at + dir + items.length) % items.length;
    display(at, dir);
  }

  async function display(i, dir) {
    const mine = ++step;
    clearTimeout(timer);
    const item = items[i];
    showCaption(item);
    if (item.video) return playVideo(item, mine);
    endVideo();
    const next = layers[1 - front];
    const kind = transitionOf();
    next.classList.remove('on', 'kb', 'cut');
    next.src = screenOf(item);
    try {
      await next.decode();
    } catch {
      // A picture the server cannot show: on to the next one, in the same direction.
      if (mine === step) setTimeout(() => go(dir || 1), 50);
      return;
    }
    if (mine !== step) return;
    const old = layers[front];
    next.classList.toggle('cut', kind === 'cut');
    old.classList.toggle('cut', kind === 'cut');
    if (kind === 'zoom') {
      next.style.setProperty('--kb-ms', (Number(opts.seconds) * 1000 + FADE_MS * 2) + 'ms');
      next.style.setProperty('--kb-x', (20 + Math.random() * 60).toFixed(0) + '%');
      next.style.setProperty('--kb-y', (20 + Math.random() * 60).toFixed(0) + '%');
      // Start the animation again even when the same layer zoomed before.
      void next.offsetWidth;
      next.classList.add('kb');
    }
    next.classList.add('on');
    old.classList.remove('on');
    front = 1 - front;
    preload(i + 1);
    schedule(Number(opts.seconds) * 1000);
  }

  function preload(i) {
    const item = items[(i + items.length) % items.length];
    if (item && !item.video) { const im = new Image(); im.src = screenOf(item); }
  }

  // ---- Videos: they play through, the music is quieter meanwhile -----------------------------------------------------------------------

  function playVideo(item, mine) {
    layers.forEach((l) => l.classList.remove('on'));
    video.hidden = false;
    video.poster = screenOf(item);
    video.src = videoOf(item);
    video.onended = () => { if (mine === step) go(1); };
    video.onerror = () => { if (mine === step) go(1); };
    duck(true);
    video.play().catch(() => {
      // The browser does not let it play with sound: muted then, the music stays.
      video.muted = true;
      duck(false);
      video.play().catch(() => { if (mine === step) go(1); });
    });
  }

  function endVideo() {
    if (video.hidden) return;
    video.pause();
    video.onended = null;
    video.onerror = null;
    video.removeAttribute('src');
    video.load();
    video.hidden = true;
    video.muted = false;
    duck(false);
  }

  let fade = 0;
  function duck(down) {
    clearInterval(fade);
    if (!music.src) return;
    const target = down ? 0.15 : 1;
    fade = setInterval(() => {
      const v = music.volume + (target > music.volume ? 0.05 : -0.05);
      if (Math.abs(target - music.volume) <= 0.05) { music.volume = target; clearInterval(fade); } else music.volume = Math.min(1, Math.max(0, v));
    }, 40);
  }

  // ---- The music ---------------------------------------------------------------------------------------------------------------------

  let tracks = [];
  let track = 0;
  let mp3 = false;

  async function startMusic(id) {
    if (offline) {
      tracks = (project.music || []).slice();
      if (opts.order === 'random') shuffle(tracks);
      track = 0;
      playTrack();
      return;
    }
    try {
      const r = await fetch('/pictures/music/' + encodeURIComponent(id), { credentials: 'same-origin' });
      tracks = r.ok ? (await r.json()).tracks || [] : [];
    } catch { tracks = []; }
    if (!tracks.length || !running) return;
    if (opts.order === 'random') shuffle(tracks);
    track = 0;
    playTrack();
  }

  function playTrack() {
    if (!running || !tracks.length) return;
    music.volume = video.hidden ? 1 : 0.15;
    music.src = offline ? tracks[track].file : '/pictures/track/' + tracks[track].id + (mp3 ? '?as=mp3' : '');
    music.play().catch(() => toast(show.dataset.tMusicFailed));
  }

  music.addEventListener('ended', () => { mp3 = false; track = (track + 1) % tracks.length; playTrack(); });
  music.addEventListener('error', () => {
    if (!music.getAttribute('src')) return;
    // The browser cannot play this format: once more as an MP3 the server makes, then the next track (a project holds MP3s already).
    if (!mp3 && !offline) { mp3 = true; playTrack(); return; }
    mp3 = false;
    track = (track + 1) % tracks.length;
    if (tracks.length > 1) playTrack();
  });

  // ---- The caption: the date, and the place when the server names places ---------------------------------------------------------------

  function showCaption(item) {
    if (opts.info === 'none') return;
    dateEl.textContent = new Intl.DateTimeFormat(lang, { dateStyle: 'long' }).format(new Date(item.taken_at));
    placeEl.textContent = '';
    if (opts.info !== 'both' || !placesOn || item.video) return;
    if (offline) { placeEl.textContent = item.place || ''; return; }
    const known = infos.get(item.key);
    if (known !== undefined) { placeEl.textContent = known || ''; return; }
    fetch(url(item.key, 'info'), { credentials: 'same-origin' })
      .then((r) => (r.ok ? r.json() : null))
      .then((v) => {
        const name = (v && v.place) || '';
        infos.set(item.key, name);
        if (items[at] && items[at].key === item.key) placeEl.textContent = name;
      })
      .catch(() => {});
  }

  // ---- Controls: tap, swipe, keys ------------------------------------------------------------------------------------------------------

  let hideUi = 0;
  function flashControls() {
    show.classList.add('ui');
    clearTimeout(hideUi);
    hideUi = setTimeout(() => show.classList.remove('ui'), 3000);
  }

  function toast(text) {
    toastEl.textContent = text;
    toastEl.hidden = false;
    setTimeout(() => { toastEl.hidden = true; }, 2500);
  }

  function togglePause() {
    paused = !paused;
    if (paused) {
      clearTimeout(timer);
      if (!video.hidden) video.pause();
      toast(show.dataset.tPaused);
    } else if (!video.hidden) {
      video.play().catch(() => {});
    } else {
      schedule(Number(opts.seconds) * 1000);
    }
    show.classList.toggle('paused', paused);
  }

  $('show-prev').addEventListener('click', (e) => { e.stopPropagation(); go(-1); flashControls(); });
  $('show-next').addEventListener('click', (e) => { e.stopPropagation(); go(1); flashControls(); });
  $('show-pause').addEventListener('click', (e) => { e.stopPropagation(); togglePause(); flashControls(); });
  $('show-close').addEventListener('click', (e) => { e.stopPropagation(); stop(); });

  let down = null;
  stage.addEventListener('pointerdown', (e) => { down = { x: e.clientX, y: e.clientY, t: Date.now() }; });
  stage.addEventListener('pointerup', (e) => {
    if (!down) return;
    const dx = e.clientX - down.x;
    const dy = e.clientY - down.y;
    down = null;
    if (Math.abs(dx) > 40 && Math.abs(dx) > Math.abs(dy)) { go(dx < 0 ? 1 : -1); flashControls(); return; }
    // A swipe down closes, as in the photo apps of the phones.
    if (dy > 80 && Math.abs(dy) > Math.abs(dx)) { stop(); return; }
    flashControls();
  });
  show.addEventListener('mousemove', () => { if (running) flashControls(); });

  document.addEventListener('keydown', (e) => {
    if (!running) return;
    if (e.key === 'ArrowRight') go(1);
    else if (e.key === 'ArrowLeft') go(-1);
    else if (e.key === ' ') { e.preventDefault(); togglePause(); }
    else if (e.key === 'Escape') stop();
    else return;
    flashControls();
  });

  // Leaving the full screen (Escape, the gesture of the phone) ends the show.
  document.addEventListener('fullscreenchange', () => { if (!document.fullscreenElement && running) stop(); });
  // The lock of the screen is lost when the page is hidden; asked for again when it comes back.
  document.addEventListener('visibilitychange', async () => {
    if (running && document.visibilityState === 'visible' && navigator.wakeLock) {
      try { wake = await navigator.wakeLock.request('screen'); } catch { wake = null; }
    }
  });
})();
