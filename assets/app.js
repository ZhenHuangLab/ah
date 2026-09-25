'use strict';

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => Array.from(el.querySelectorAll(s));

const LIVE_MS = 2 * 60 * 1000;
const root = document.documentElement;
const scroller = $('#scroll');
const conv = $('#conv');
const ticks = $('#ticks');
const card = $('#card');

const S = {
  home: '',
  sessions: [],
  agent: localStorage.getItem('ah.agent') || '',
  filter: '',
  // The open session: {id, gen, rev, meta, items: [], els: []}.
  cur: null,
  es: null,
  opening: 0,
  follow: true,
  // Until this time, growing content keeps the view pinned to the bottom.
  stickUntil: 0,
  // The scroll position pin() last set.
  pinned: -1,
  turns: [],
  on: -1,
  // A turn picked with the keys or the rail, kept marked while the view stays where the jump left it.
  held: null,
};

// ---------- helpers ----------

function esc(s) {
  return String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
}

function ago(ms) {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return 'now';
  if (s < 3600) return Math.floor(s / 60) + 'm';
  if (s < 86400) return Math.floor(s / 3600) + 'h';
  if (s < 86400 * 30) return Math.floor(s / 86400) + 'd';
  return new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function when(ms) {
  return ms ? new Date(ms).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : '';
}

function tilde(p) {
  return S.home && p.startsWith(S.home) ? '~' + p.slice(S.home.length) : p;
}

function project(p) {
  const t = p.replace(/\/+$/, '');
  return t.slice(t.lastIndexOf('/') + 1) || t;
}

async function copyText(s) {
  if (navigator.clipboard && window.isSecureContext) return navigator.clipboard.writeText(s);
  // Plain http over the tailnet is not a secure context, so fall back to execCommand.
  const ta = document.createElement('textarea');
  ta.value = s;
  ta.style.cssText = 'position:fixed;top:0;left:0;opacity:0';
  document.body.appendChild(ta);
  ta.select();
  document.execCommand('copy');
  ta.remove();
}

// ---------- session list ----------

function group(ms) {
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const day = 86400000;
  if (ms >= today) return 'Today';
  if (ms >= today - day) return 'Yesterday';
  if (ms >= today - 6 * day) return 'Previous 7 days';
  if (ms >= today - 29 * day) return 'Previous 30 days';
  return new Date(ms).toLocaleDateString(undefined, { year: 'numeric', month: 'long' });
}

function renderList() {
  const words = S.filter.toLowerCase().split(/\s+/).filter(Boolean);
  let html = '';
  let grp = '';
  for (const m of S.sessions) {
    if (S.agent && m.agent !== S.agent) continue;
    if (words.length) {
      const hay = `${m.title} ${m.cwd} ${m.id} ${m.agent}`.toLowerCase();
      if (!words.every(w => hay.includes(w))) continue;
    }
    const g = group(m.modified);
    if (g !== grp) {
      html += `<div class="grp">${esc(g)}</div>`;
      grp = g;
    }
    const live = Date.now() - m.modified < LIVE_MS ? '<span class="dot" title="Active"></span>' : '';
    const on = S.cur && S.cur.id === m.id ? ' on' : '';
    html += `<a class="row${on}" href="#/${encodeURIComponent(m.id)}" data-id="${esc(m.id)}">` +
      `<div class="t">${esc(m.title)}</div>` +
      `<div class="m">${live}<span class="badge ${m.agent}">${m.agent}</span>` +
      `<span class="p" title="${esc(m.cwd)}">${esc(project(m.cwd))}</span><span>· ${ago(m.modified)}</span></div></a>`;
  }
  $('#list').innerHTML = html || '<div class="grp">No sessions</div>';
}

let listTimer = 0;
function listSoon() {
  if (!listTimer) listTimer = setTimeout(() => { listTimer = 0; renderList(); }, 400);
}

async function loadSessions() {
  const r = await fetch('api/sessions');
  const d = await r.json();
  S.home = d.home;
  S.sessions = d.sessions;
  renderList();
}

function upsert(m) {
  const k = S.sessions.findIndex(s => s.id === m.id);
  if (k >= 0) S.sessions.splice(k, 1);
  let j = S.sessions.findIndex(s => s.modified <= m.modified);
  if (j < 0) j = S.sessions.length;
  S.sessions.splice(j, 0, m);
  if (S.cur && S.cur.id === m.id) {
    Object.assign(S.cur.meta, m);
    renderHead();
  }
  listSoon();
}

function listEvents() {
  const es = new EventSource('api/events');
  let broken = false;
  es.addEventListener('meta', e => upsert(JSON.parse(e.data)));
  es.addEventListener('gone', e => {
    S.sessions = S.sessions.filter(s => s.id !== e.data);
    listSoon();
  });
  es.addEventListener('reload', loadSessions);
  es.onerror = () => { broken = true; };
  es.onopen = () => {
    if (broken) loadSessions();
    broken = false;
  };
}

// ---------- conversation ----------

function renderHead() {
  const m = S.cur.meta;
  $('#title').textContent = m.title;
  document.title = `${m.title} · ah`;
  const live = Date.now() - m.modified < LIVE_MS ? '<span class="dot" title="Active"></span>' : '';
  $('#sub').innerHTML = `${live}<span class="badge ${m.agent}">${m.agent}</span>` +
    `<span title="${esc(m.cwd)}">${esc(tilde(m.cwd))}</span>` +
    (m.model ? `<span>${esc(m.model)}</span>` : '') +
    `<span>${esc(when(m.started))}</span>`;
}

async function open(id, target) {
  const token = ++S.opening;
  if (S.es) S.es.close();
  S.es = null;
  const r = await fetch('api/s/' + encodeURIComponent(id));
  if (token !== S.opening) return;
  if (!r.ok) {
    S.cur = null;
    S.turns = [];
    S.on = -1;
    conv.innerHTML = '';
    ticks.innerHTML = '';
    $('#title').textContent = 'ah';
    $('#sub').innerHTML = '';
    document.title = 'ah';
    $('#empty').textContent = r.status === 404 ? 'No such session.' : 'Could not load this session.';
    return;
  }
  const snap = await r.json();
  if (token !== S.opening) return;
  S.cur = { id, gen: snap.gen, rev: snap.rev, meta: snap.meta, items: [], els: [] };
  S.held = null;
  conv.innerHTML = '';
  for (const it of snap.items) put(it);
  renderHead();
  buildRail();
  for (const row of $$('#list .row')) row.classList.toggle('on', row.dataset.id === id);
  document.body.classList.remove('side-open');
  const el = target != null && S.cur.els[target];
  if (el) {
    el.scrollIntoView({ block: 'start' });
    S.follow = false;
  } else {
    stick();
  }
  spy();
  subscribe();
}

function subscribe() {
  const c = S.cur;
  const es = new EventSource(`api/s/${encodeURIComponent(c.id)}/events?gen=${c.gen}&rev=${c.rev}`);
  es.addEventListener('items', e => {
    if (S.cur !== c) return;
    const d = JSON.parse(e.data);
    const follow = S.follow;
    c.rev = d.rev;
    Object.assign(c.meta, d.meta);
    for (const it of d.items) put(it);
    renderHead();
    buildRail();
    if (follow) stick();
    else $('#bottom').hidden = false;
  });
  es.addEventListener('reset', () => {
    if (S.cur === c) open(c.id, S.follow ? null : topItem());
  });
  // Reconnect by hand so the request carries the latest revision.
  es.onerror = () => {
    es.close();
    if (S.es === es) {
      S.es = null;
      setTimeout(() => { if (S.cur === c && !S.es) subscribe(); }, 2000);
    }
  };
  S.es = es;
}

function put(it) {
  const c = S.cur;
  c.items[it.i] = it;
  let el = c.els[it.i];
  // Folds the reader opened stay open when the item is re-rendered, including ones inside
  // content that loads later. Until they have loaded again the item keeps its height, so the
  // view does not jump.
  const keep = el ? new Set($$('details[open]', el).map(d => d.dataset.k)) : null;
  if (keep && keep.size) el.style.minHeight = el.offsetHeight + 'px';
  if (!el) {
    el = document.createElement('article');
    el.id = 'i-' + it.i;
    el.dataset.i = it.i;
    let next = null;
    for (let k = it.i + 1; k < c.els.length && !next; k++) next = c.els[k] || null;
    conv.insertBefore(el, next);
    c.els[it.i] = el;
  }
  el.className = `item ${it.role}` + (it.kind ? ` k-${it.kind}` : '') + (it.role === 'assistant' && !it.md ? ' bare' : '');
  let acts = '';
  if (it.md) {
    const t = it.role === 'user' && it.time ? `<span>${esc(when(it.time))}</span>` : '';
    acts = `<div class="acts">${t}<button class="copy">Copy</button></div>`;
  }
  el.innerHTML = it.html + acts;
  el.keep = keep;
  if (keep) reopen(el, keep);
  settle(el);
  seen.observe(el);
}

// Math, highlighting and table wrappers are applied when an item comes near the viewport.
const seen = new IntersectionObserver(entries => {
  for (const e of entries) {
    if (!e.isIntersecting) continue;
    seen.unobserve(e.target);
    enhance(e.target);
  }
}, { root: scroller, rootMargin: '1200px 0px' });

function enhance(el) {
  if (window.katex) {
    for (const m of $$('.math', el)) {
      if (m.dataset.done) continue;
      m.dataset.done = '1';
      try {
        katex.render(m.textContent, m, { displayMode: m.classList.contains('math-display'), throwOnError: false });
      } catch (_) { /* leave the source visible */ }
    }
  }
  if (window.hljs) {
    for (const code of $$('pre code[class*="language-"]', el)) {
      if (code.dataset.highlighted || code.textContent.length > 200000) continue;
      const lang = (code.className.match(/language-([\w+#.-]+)/) || [])[1];
      if (lang && hljs.getLanguage(lang)) hljs.highlightElement(code);
    }
  }
  for (const t of $$('.md table', el)) {
    if (t.parentElement.classList.contains('tbl')) continue;
    const w = document.createElement('div');
    w.className = 'tbl';
    t.replaceWith(w);
    w.appendChild(t);
  }
  for (const a of $$('.md a[href]', el)) {
    a.target = '_blank';
    a.rel = 'noopener noreferrer';
  }
  for (const p of $$('.prompt', el)) clamp(p);
}

function clamp(p) {
  if (p.dataset.clamped) return;
  p.dataset.clamped = '1';
  p.classList.add('clamp');
  if (p.scrollHeight <= p.clientHeight + 8) {
    p.classList.remove('clamp');
    return;
  }
  const b = document.createElement('button');
  b.className = 'more';
  b.textContent = 'Show more';
  b.onclick = () => { b.textContent = p.classList.toggle('clamp') ? 'Show more' : 'Show less'; };
  p.after(b);
}

function reopen(el, keep) {
  for (const d of $$('details[data-k]', el)) {
    if (!keep.has(d.dataset.k)) continue;
    d.open = true;
    // Load now rather than on the toggle event, so the item counts the load before its
    // parent's finishes.
    if (d.dataset.src && !d.dataset.loaded) loadBody(d);
  }
}

/** Releases the height an item kept for its reopened folds once they have all loaded. */
function settle(item) {
  if (!item.pending) item.style.minHeight = '';
}

/** Fetches the content of a fold (tool rows and details, thinking, notice body) when it is opened. */
async function loadBody(d) {
  d.dataset.loaded = '1';
  const item = d.closest('.item');
  const body = $(':scope > .body', d);
  body.innerHTML = '<div class="sec-t">loading…</div>';
  item.pending = (item.pending || 0) + 1;
  try {
    const r = await fetch(d.dataset.src);
    if (!r.ok) throw new Error(r.status);
    body.innerHTML = await r.text();
    if (item.keep) reopen(body, item.keep);
    enhance(body);
  } catch (_) {
    body.innerHTML = '<div class="sec-t">could not load</div>';
    delete d.dataset.loaded;
  } finally {
    item.pending--;
    settle(item);
  }
}

// ---------- position, follow and the turn rail ----------

function atBottom() {
  return scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 60;
}

function stick() {
  S.follow = true;
  S.stickUntil = Date.now() + 1500;
  pin();
  $('#bottom').hidden = true;
}

function pin() {
  scroller.scrollTop = scroller.scrollHeight;
  S.pinned = scroller.scrollTop;
}

new ResizeObserver(() => {
  if (S.follow && Date.now() < S.stickUntil) pin();
}).observe(conv);

/** Index of the first visible item at the top of the view. */
function topItem() {
  if (!S.cur) return null;
  const y = scroller.scrollTop;
  for (const el of S.cur.els) {
    if (el && el.offsetParent && el.offsetTop + el.offsetHeight > y) return +el.dataset.i;
  }
  return null;
}

function buildRail() {
  const c = S.cur;
  const turns = [];
  for (const it of c.items) {
    if (!it) continue;
    if (it.role === 'user') turns.push({ i: it.i, size: it.md.length, answer: '', time: it.time });
    else if (turns.length) {
      const t = turns[turns.length - 1];
      t.size += it.md.length;
      if (it.role === 'assistant' && it.preview) t.answer = it.preview;
    }
  }
  S.turns = turns;
  ticks.innerHTML = turns.map((t, k) => {
    const w = Math.round(8 + Math.min(14, 3 * Math.log2(1 + t.size / 200)));
    return `<button class="tick" data-k="${k}" style="--w:${w}px" aria-label="Turn ${k + 1}"></button>`;
  }).join('');
  S.on = -1;
  spy();
}

/**
 * Highlights the turn under the reading line, a third of the way down the view. At the bottom
 * the line moves to the lower edge, since the last turns may never reach a third of the way up.
 */
function spy() {
  if (!S.cur || !S.turns.length) return;
  let k = 0;
  if (S.held && S.held.top === scroller.scrollTop && S.held.k < S.turns.length) {
    k = S.held.k;
  } else {
    S.held = null;
    const y = scroller.scrollTop + (atBottom() ? scroller.clientHeight : scroller.clientHeight / 3);
    let lo = 0;
    let hi = S.turns.length - 1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (S.cur.els[S.turns[mid].i].offsetTop <= y) {
        k = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
  }
  if (k === S.on) return;
  const bs = ticks.children;
  if (bs[S.on]) bs[S.on].classList.remove('on');
  if (bs[k]) {
    bs[k].classList.add('on');
    const tr = ticks.getBoundingClientRect();
    const br = bs[k].getBoundingClientRect();
    if (br.top < tr.top || br.bottom > tr.bottom) ticks.scrollTop += br.top - tr.top - tr.height / 2;
  }
  S.on = k;
}

function jump(k) {
  const t = S.turns[k];
  if (!t) return;
  S.cur.els[t.i].scrollIntoView({ block: 'start' });
  S.follow = false;
  // Near the end the view cannot scroll a turn up to the reading line, so mark it directly.
  S.held = { k, top: scroller.scrollTop };
  spy();
  history.replaceState(null, '', `#/${encodeURIComponent(S.cur.id)}/${t.i}`);
}

function showCard(k, tick) {
  const t = S.turns[k];
  if (!t) return;
  $('.card-t', card).textContent = S.cur.items[t.i].preview;
  $('.card-b', card).textContent = t.answer || '…';
  $('.card-w', card).textContent = when(t.time);
  card.hidden = false;
  const box = $('#main').getBoundingClientRect();
  const r = tick.getBoundingClientRect();
  const top = r.top + r.height / 2 - box.top - card.offsetHeight / 2;
  card.style.top = Math.max(8, Math.min(top, box.height - card.offsetHeight - 8)) + 'px';
}

function setChat(on) {
  const i = S.follow ? null : topItem();
  const el = i != null && S.cur.els[i];
  const before = el ? el.getBoundingClientRect().top : 0;
  root.classList.toggle('chat', on);
  localStorage.setItem('ah.chat', on ? '1' : '0');
  if (S.follow) stick();
  else if (el && el.offsetParent) scroller.scrollTop += el.getBoundingClientRect().top - before;
  S.on = -1;
  spy();
}

// ---------- wiring ----------

function route() {
  const m = location.hash.match(/^#\/([^/]+)(?:\/(\d+))?/);
  if (!m) {
    if (window.innerWidth <= 860) document.body.classList.add('side-open');
    return;
  }
  const id = decodeURIComponent(m[1]);
  const target = m[2] != null ? +m[2] : null;
  if (S.cur && S.cur.id === id) {
    const el = target != null && S.cur.els[target];
    if (el) el.scrollIntoView({ block: 'start' });
    return;
  }
  open(id, target);
}

function wire() {
  for (const b of $$('#agents button')) b.classList.toggle('on', b.dataset.agent === S.agent);
  $('#agents').addEventListener('click', e => {
    const b = e.target.closest('button');
    if (!b) return;
    S.agent = b.dataset.agent;
    localStorage.setItem('ah.agent', S.agent);
    for (const x of $$('#agents button')) x.classList.toggle('on', x === b);
    renderList();
  });
  $('#filter').addEventListener('input', e => {
    S.filter = e.target.value;
    renderList();
  });
  $('#menu').addEventListener('click', () => document.body.classList.toggle('side-open'));
  $('#main').addEventListener('click', e => {
    if (!e.target.closest('#menu')) document.body.classList.remove('side-open');
  });
  $('#mode').addEventListener('click', () => setChat(!root.classList.contains('chat')));
  $('#theme').addEventListener('click', () => {
    const t = root.dataset.theme === 'dark' ? 'light' : 'dark';
    root.dataset.theme = t;
    localStorage.setItem('ah.theme', t);
  });
  $('#bottom').addEventListener('click', stick);

  scroller.addEventListener('scroll', () => {
    // A scroll pin() made is not the reader's, even when content has grown below it since.
    if (atBottom()) S.follow = true;
    else if (scroller.scrollTop !== S.pinned) S.follow = false;
    if (S.follow) $('#bottom').hidden = true;
    requestAnimationFrame(spy);
  }, { passive: true });

  ticks.addEventListener('mouseover', e => {
    const b = e.target.closest('.tick');
    if (b) showCard(+b.dataset.k, b);
  });
  ticks.addEventListener('mouseleave', () => { card.hidden = true; });
  ticks.addEventListener('click', e => {
    const b = e.target.closest('.tick');
    if (!b) return;
    card.hidden = true;
    jump(+b.dataset.k);
  });

  conv.addEventListener('click', e => {
    const b = e.target.closest('.copy');
    if (!b) return;
    const it = S.cur.items[+b.closest('.item').dataset.i];
    copyText(it.md).then(() => {
      b.textContent = 'Copied';
      setTimeout(() => { b.textContent = 'Copy'; }, 1200);
    });
  });

  // `toggle` does not bubble; listen in the capture phase.
  document.addEventListener('toggle', e => {
    const d = e.target;
    if (d.dataset && d.dataset.src && d.open && !d.dataset.loaded) loadBody(d);
  }, true);

  document.addEventListener('keydown', e => {
    if (e.target.closest('input, textarea') || e.metaKey || e.ctrlKey || e.altKey) return;
    const cur = S.cur && S.turns[S.on];
    const offset = cur ? S.cur.els[cur.i].getBoundingClientRect().top - scroller.getBoundingClientRect().top : 0;
    switch (e.key) {
      case 't': setChat(!root.classList.contains('chat')); break;
      case 'j': case ']': jump(Math.min(S.on + 1, S.turns.length - 1)); break;
      case 'k': case '[': jump(offset < -20 ? S.on : Math.max(S.on - 1, 0)); break;
      case 'g': scroller.scrollTop = 0; break;
      case 'G': stick(); break;
      case '/':
        document.body.classList.add('side-open');
        $('#filter').focus();
        break;
      default: return;
    }
    e.preventDefault();
  });

  window.addEventListener('hashchange', route);
  setInterval(() => {
    renderList();
    if (S.cur) renderHead();
  }, 60000);
}

(async function init() {
  wire();
  await loadSessions();
  listEvents();
  route();
})();
