// download_bridge.js — injected at document start.
//
// Mirrors the WKWebView blob-download interceptor in WebViewHost.swift. The
// page calls URL.createObjectURL(blob) + <a download>.click() to push STL
// bytes; WebView2 normally drops these silently. We catch the click in
// capture phase, read the blob as base64, and ship it back to Rust via the
// Tauri event bus.
(function () {
  if (window.__SLP_DL_INSTALLED__) return;
  window.__SLP_DL_INSTALLED__ = true;

  const origCreate = URL.createObjectURL.bind(URL);
  const blobMap = new Map();
  URL.createObjectURL = function (blob) {
    const url = origCreate(blob);
    try { blobMap.set(url, blob); } catch (e) {}
    return url;
  };
  const origRevoke = URL.revokeObjectURL.bind(URL);
  URL.revokeObjectURL = function (url) {
    setTimeout(function () {
      try { origRevoke(url); } catch (e) {}
      blobMap.delete(url);
    }, 0);
  };

  document.addEventListener('click', function (e) {
    const a = e.target && e.target.closest ? e.target.closest('a[download]') : null;
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (href.indexOf('blob:') !== 0) return;
    const blob = blobMap.get(href);
    if (!blob) return;
    e.preventDefault();
    e.stopPropagation();
    const fname = a.getAttribute('download') || 'download.bin';
    const reader = new FileReader();
    reader.onload = function () {
      const dataUrl = String(reader.result || '');
      const comma = dataUrl.indexOf(',');
      const b64 = comma >= 0 ? dataUrl.substring(comma + 1) : '';
      try {
        if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.emit) {
          window.__TAURI__.event.emit('slp:stl-download', { filename: fname, base64: b64 });
        }
      } catch (err) {
        console.error('stl-download bridge error:', err);
      }
    };
    reader.readAsDataURL(blob);
  }, true);

  // License activation: the HTML gate already calls a unified API at
  // window.SLP_HOST.activate(key) when running in a host app. Provide that
  // here so the existing license-entry modal works unchanged.
  window.SLP_HOST = window.SLP_HOST || {};
  window.SLP_HOST.activate = function (key) {
    return new Promise(function (resolve) {
      if (!(window.__TAURI__ && window.__TAURI__.event)) {
        resolve({ ok: false, reason: 'no-host' });
        return;
      }
      const off = window.__TAURI__.event.listen('slp:license-result', function (e) {
        off.then(function (unlisten) { unlisten(); });
        resolve(e.payload || { ok: false, reason: 'no-reply' });
      });
      window.__TAURI__.event.emit('slp:activate-license', { key: key });
    });
  };

  // "Enter License Key…" menu command opens the existing in-page modal.
  if (window.__TAURI__ && window.__TAURI__.event) {
    window.__TAURI__.event.listen('slp:request-license-entry', function () {
      const btn = document.getElementById('btnEnterLicense')
                || document.querySelector('[data-action="enter-license"]');
      if (btn) btn.click();
    });
  }
})();
