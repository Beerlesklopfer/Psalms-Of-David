// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
// The cut and the zoom of a cover in the tags window (project owner, 2026-10-28): drag to move,
// mouse wheel, pinch on a touch screen or a trackpad to zoom. The cut goes into the file field of the
// form as a JPEG, so the server gets the picture as it is meant and the form is sent as usual.
(function () {
  'use strict';

  // ---- The arithmetic (no page needed, tested with node) -----------------------------------------
  // `iw`, `ih`: the picture; `fw`, `fh`: the frame; `zoom` >= 1 on top of the smallest scale that
  // still fills the frame; `ox`, `oy`: where the top left of the picture lies in the frame.
  const fill = (iw, ih, fw, fh) => Math.max(fw / iw, fh / ih);
  const MAX_ZOOM = 6;
  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
  // The picture must always cover the frame: no empty edge.
  const place = (iw, ih, fw, fh, zoom, ox, oy) => {
    const z = clamp(zoom, 1, MAX_ZOOM);
    const s = fill(iw, ih, fw, fh) * z;
    return { zoom: z, scale: s, ox: clamp(ox, fw - iw * s, 0), oy: clamp(oy, fh - ih * s, 0) };
  };
  // Zoom so that the point (px, py) of the frame stays where it is.
  const zoomAt = (st, iw, ih, fw, fh, newZoom, px, py) => {
    const z = clamp(newZoom, 1, MAX_ZOOM);
    const s0 = st.scale;
    const s1 = fill(iw, ih, fw, fh) * z;
    const ix = (px - st.ox) / s0;
    const iy = (py - st.oy) / s0;
    return place(iw, ih, fw, fh, z, px - ix * s1, py - iy * s1);
  };
  // The part of the picture that is in the frame, in picture pixels.
  const region = (st, fw, fh) => ({ sx: -st.ox / st.scale, sy: -st.oy / st.scale, sw: fw / st.scale, sh: fh / st.scale });
  const centered = (iw, ih, fw, fh) => place(iw, ih, fw, fh, 1, (fw - iw * fill(iw, ih, fw, fh)) / 2, (fh - ih * fill(iw, ih, fw, fh)) / 2);

  const api = { fill, place, zoomAt, region, centered, MAX_ZOOM };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  if (typeof document === 'undefined') return;

  // ---- The frame: the picture of the tags window itself ----------------------------------------
  // Mouse over it and it is ready: drag to move the picture, the mouse wheel (or two fingers, or a
  // pinch on a trackpad) to zoom. The frame is square, like the covers in the lists. Only a changed cut
  // is sent: until then the form has no picture and the album keeps its cover.
  const OUT_MAX = 1000; // the longest side of the cut, in pixels

  function Cropper(frame) {
    this.frame = frame;
    this.input = document.getElementById(frame.dataset.input);
    this.img = frame.querySelector('.crop-image');
    this.st = null;
    this.loading = null;
    this.pointers = new Map();
    this.timer = 0;
    this.bind();
  }

  Cropper.prototype.size = function () {
    return { fw: this.frame.clientWidth || 192, fh: this.frame.clientHeight || 192 };
  };

  // The picture as a data URL (the page lets images in from itself and from data:, not from blob:).
  const dataUrl = (blob) => new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = reject;
    reader.readAsDataURL(blob);
  });

  Cropper.prototype.show = function (url) {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        this.iw = img.naturalWidth;
        this.ih = img.naturalHeight;
        this.bitmap = img;
        this.img.src = url;
        this.img.style.objectFit = 'fill';
        this.st = centered(this.iw, this.ih, ...Object.values(this.size()));
        this.draw();
        resolve();
      };
      img.onerror = reject;
      img.src = url;
    });
  };

  // The picture that the album has now: loaded when the mouse comes over the frame, once.
  Cropper.prototype.ensure = function () {
    if (this.st || this.loading) return this.loading;
    this.frame.classList.add('loading'); // the mouse pointer says that it is not ready yet
    this.loading = fetch(this.frame.dataset.src, { credentials: 'same-origin' })
      .then((res) => { if (!res.ok) throw new Error(String(res.status)); return res.blob(); })
      .then(dataUrl)
      .then((url) => this.show(url))
      .catch(() => { this.loading = null; })
      .finally(() => this.frame.classList.remove('loading'));
    return this.loading;
  };

  // A picture that was chosen or pasted replaces it, centred.
  Cropper.prototype.load = function (file) {
    return dataUrl(file).then((url) => this.show(url)).then(() => this.commit());
  };

  Cropper.prototype.draw = function () {
    const st = this.st;
    this.img.style.width = this.iw * st.scale + 'px';
    this.img.style.height = this.ih * st.scale + 'px';
    this.img.style.transform = `translate(${st.ox}px, ${st.oy}px)`;
  };

  // The cut becomes the file of the form (a little later: a drag makes many small changes).
  Cropper.prototype.commit = function () {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      const { fw, fh } = this.size();
      const r = region(this.st, fw, fh);
      const k = Math.min(1, OUT_MAX / Math.max(r.sw, r.sh));
      const canvas = document.createElement('canvas');
      canvas.width = Math.max(1, Math.round(r.sw * k));
      canvas.height = Math.max(1, Math.round(r.sh * k));
      const ctx = canvas.getContext('2d');
      ctx.fillStyle = '#fff'; // a picture with a transparent background becomes white, not black
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(this.bitmap, r.sx, r.sy, r.sw, r.sh, 0, 0, canvas.width, canvas.height);
      canvas.toBlob((blob) => {
        if (!blob) return;
        const list = new DataTransfer();
        list.items.add(new File([blob], 'cover.jpg', { type: 'image/jpeg' }));
        this.input.files = list.files; // no change event: that would load the picture again
      }, 'image/jpeg', 0.92);
    }, 150);
  };

  Cropper.prototype.moveBy = function (dx, dy) {
    const { fw, fh } = this.size();
    this.st = place(this.iw, this.ih, fw, fh, this.st.zoom, this.st.ox + dx, this.st.oy + dy);
    this.draw();
    this.commit();
  };

  Cropper.prototype.zoomTo = function (zoom, px, py) {
    const { fw, fh } = this.size();
    this.st = zoomAt(this.st, this.iw, this.ih, fw, fh, zoom, px, py);
    this.draw();
    this.commit();
  };

  // Back to the album's own picture: nothing is sent.
  Cropper.prototype.reset = function () {
    clearTimeout(this.timer);
    this.input.value = '';
    this.st = null;
    this.loading = null;
    this.img.removeAttribute('style');
    this.img.src = this.frame.dataset.src + '?' + Date.now();
  };

  Cropper.prototype.bind = function () {
    const frame = this.frame;
    const local = (e) => {
      const b = frame.getBoundingClientRect();
      return { x: e.clientX - b.left, y: e.clientY - b.top };
    };
    // Ready as soon as the mouse is over it (or a finger touches it, or it has the focus).
    ['pointerenter', 'focusin', 'touchstart'].forEach((name) => frame.addEventListener(name, () => this.ensure(), { passive: true }));
    // The mouse wheel zooms; a pinch on a trackpad arrives as a wheel with ctrlKey.
    frame.addEventListener('wheel', (e) => {
      e.preventDefault();
      if (!this.st) { this.ensure(); return; }
      const p = local(e);
      const step = e.ctrlKey ? -e.deltaY * 0.02 : -e.deltaY * 0.0015;
      this.zoomTo(this.st.zoom * Math.exp(step), p.x, p.y);
    }, { passive: false });
    // Drag moves the picture; two pointers (two fingers) zoom and move.
    frame.addEventListener('pointerdown', (e) => {
      if (!this.st) { this.ensure(); return; }
      frame.setPointerCapture(e.pointerId);
      this.pointers.set(e.pointerId, local(e));
      frame.classList.add('grabbing');
    });
    frame.addEventListener('pointermove', (e) => {
      if (!this.st || !this.pointers.has(e.pointerId)) return;
      const before = Array.from(this.pointers.values());
      this.pointers.set(e.pointerId, local(e));
      const after = Array.from(this.pointers.values());
      if (before.length === 1) {
        this.moveBy(after[0].x - before[0].x, after[0].y - before[0].y);
      } else if (before.length >= 2) {
        const d0 = Math.hypot(before[0].x - before[1].x, before[0].y - before[1].y) || 1;
        const d1 = Math.hypot(after[0].x - after[1].x, after[0].y - after[1].y) || 1;
        const mid = { x: (after[0].x + after[1].x) / 2, y: (after[0].y + after[1].y) / 2 };
        const before0 = { x: (before[0].x + before[1].x) / 2, y: (before[0].y + before[1].y) / 2 };
        this.zoomTo(this.st.zoom * (d1 / d0), mid.x, mid.y);
        this.moveBy(mid.x - before0.x, mid.y - before0.y);
      }
    });
    const up = (e) => {
      this.pointers.delete(e.pointerId);
      if (!this.pointers.size) frame.classList.remove('grabbing');
    };
    frame.addEventListener('pointerup', up);
    frame.addEventListener('pointercancel', up);
    // Keys for those without a mouse: arrows move, plus and minus zoom.
    frame.addEventListener('keydown', (e) => {
      if (!this.st) return;
      const step = e.shiftKey ? 30 : 8;
      const { fw, fh } = this.size();
      const keys = { ArrowLeft: [step, 0], ArrowRight: [-step, 0], ArrowUp: [0, step], ArrowDown: [0, -step] };
      if (keys[e.key]) { e.preventDefault(); this.moveBy(...keys[e.key]); }
      else if (e.key === '+' || e.key === '=') { e.preventDefault(); this.zoomTo(this.st.zoom * 1.1, fw / 2, fh / 2); }
      else if (e.key === '-') { e.preventDefault(); this.zoomTo(this.st.zoom / 1.1, fw / 2, fh / 2); }
    });
  };

  const byInput = new Map();
  const cropperOf = (input) => {
    if (!input || !input.id) return null;
    if (byInput.has(input.id) && document.contains(byInput.get(input.id)?.frame)) return byInput.get(input.id);
    const frame = document.querySelector(`[data-cropper][data-input="${input.id}"]`);
    const c = frame ? new Cropper(frame) : null;
    byInput.set(input.id, c);
    return c;
  };

  // A picture that is chosen or pasted goes into the frame. Capturing, so that it runs before the plain preview.
  document.addEventListener('change', (e) => {
    const input = e.target;
    if (!(input instanceof HTMLInputElement) || !input.hasAttribute('data-image-input') || !input.files[0]) return;
    const c = cropperOf(input);
    if (!c) return;
    e.stopImmediatePropagation();
    c.load(input.files[0]);
  }, true);

  document.addEventListener('click', (e) => {
    const button = e.target instanceof Element ? e.target.closest('[data-crop-reset]') : null;
    if (!button) return;
    const c = cropperOf(document.getElementById(button.dataset.cropReset));
    if (c) c.reset();
  });
})();
