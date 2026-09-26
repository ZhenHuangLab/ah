'use strict';

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => Array.from(el.querySelectorAll(s));

const LIVE_MS = 2 * 60 * 1000;
// The server pings every 15 s; a stream silent for this long has lost its connection.
const SILENT_MS = 45 * 1000;
// Text sizes of the conversation, as factors of the default.
const SCALES = [0.8, 0.9, 1, 1.1, 1.2, 1.35, 1.5];
// The narrowest the session list gets by dragging its edge.
const SIDE_MIN = 220;
const ICON_COPY = '<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="5.5" y="5.5" width="8" height="8" rx="1.8"/>' +
  '<path d="M10.5 5.5V4.3a1.8 1.8 0 0 0-1.8-1.8H4.3a1.8 1.8 0 0 0-1.8 1.8v4.4a1.8 1.8 0 0 0 1.8 1.8h1.2"/></svg>';
const ICON_DONE = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8.5l3.2 3L13 4.5"/></svg>';
const VIEWS = ['all', 'chat', 'answers'];
const VIEW_NAMES = { all: 'All', chat: 'Chat', answers: 'Answers' };
// Everything as rows of messages and tool calls; the chat as a speech bubble; answers as a bubble
// with a check mark.
const BUBBLE = 'M3 3h10a1.5 1.5 0 0 1 1.5 1.5v5.5a1.5 1.5 0 0 1-1.5 1.5H7.5L4.5 14v-2.5H3A1.5 1.5 0 0 1 1.5 10V4.5A1.5 1.5 0 0 1 3 3z';
const VIEW_ICONS = {
  all: 'M2.5 3.5h11M2.5 6.5h11M5 9.5h8.5M5 12.5h8.5M2.5 9.5h.5M2.5 12.5h.5',
  chat: BUBBLE,
  answers: BUBBLE + 'M5.5 7.3l1.7 1.7 3.3-3.3',
};
const VIEW_HELP = {
  all: 'Everything, with tool calls folded. Click for chat only (t)',
  chat: 'Chat only: no tool calls, thinking or notices. Click for answers only (a)',
  answers: 'Answers only: prompts and final answers. Click to show everything',
};
const root = document.documentElement;
const scroller = $('#scroll');
const conv = $('#conv');
const ticks = $('#ticks');
const card = $('#card');
const narrow = matchMedia('(max-width: 860px)');
// The id of the shared session this page shows to a guest; unset for the owner's viewer.
const SHARE = $('meta[name="ah-share"]') ? $('meta[name="ah-share"]').content : '';
if (SHARE) root.classList.add('shared');

const S = {
  home: '',
  // The public host name, when a tunnel serves ah on one; sessions can then be shared.
  public: null,
  sessions: [],
  agent: localStorage.getItem('ah.agent') || '',
  filter: '',
  // Sessions listed by date, or by folder under headers that open and close.
  group: localStorage.getItem('ah.group') === 'folder' ? 'folder' : 'date',
  folders: new Set(JSON.parse(localStorage.getItem('ah.folders') || '[]')),
  // all: everything; chat: no tool calls, thinking or notices; answers: prompts and final answers.
  view: VIEWS.includes(localStorage.getItem('ah.view')) ? localStorage.getItem('ah.view') : 'all',
  scale: SCALES.includes(+localStorage.getItem('ah.scale')) ? +localStorage.getItem('ah.scale') : 1,
  // The conversation across the whole window rather than in a centered column.
  wide: localStorage.getItem('ah.wide') === '1',
  // The open session: {id, gen, rev, meta, items: [], els: []}.
  cur: null,
  // Event streams of the session list and of the open session.
  listEs: null,
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

/** Puts `s` on the clipboard; rejects when the browser refuses. */
async function copyText(s) {
  if (navigator.clipboard && window.isSecureContext) {
    try {
      return await navigator.clipboard.writeText(s);
    } catch (_) { /* try the older way below */ }
  }
  // Plain http over the tailnet is not a secure context, so this is the usual way.
  const ta = document.createElement('textarea');
  ta.value = s;
  ta.style.cssText = 'position:fixed;top:0;left:0;opacity:0';
  document.body.appendChild(ta);
  ta.select();
  const ok = document.execCommand('copy');
  ta.remove();
  if (!ok) throw new Error('the browser refused to copy');
}

let toastTimer = 0;
function toast(msg) {
  const t = $('#toast');
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 1400);
}

const AGENT_NAMES = { claude: 'Claude Code', codex: 'Codex', pi: 'pi' };

/** The agent's mark, named in a tooltip. */
function agentIcon(agent) {
  const name = esc(AGENT_NAMES[agent] || agent);
  return `<span class="agent" title="${name}"><svg class="ico" role="img" aria-label="${name}"><use href="#i-${esc(agent)}"/></svg></span>`;
}

/** Markdown of an item's text; in the answers view, of the final answer it shows. */
function itemMarkdown(it) {
  return S.view === 'answers' && it.answer ? it.texts[it.texts.length - 1] : it.texts.join('\n\n');
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

function row(m) {
  const live = Date.now() - m.modified < LIVE_MS ? '<span class="dot" title="Active"></span>' : '';
  const on = S.cur && S.cur.id === m.id ? ' on' : '';
  // Under a folder header the folder name would repeat on every row.
  const where = S.group === 'folder' ? '' : `<span class="p" title="${esc(m.cwd)}">${esc(project(m.cwd))}</span>`;
  return `<a class="row${on}" href="#/${encodeURIComponent(m.id)}" data-id="${esc(m.id)}">` +
    `<div class="t">${esc(m.title)}</div>` +
    `<div class="m">${live}${agentIcon(m.agent)}${where}<span>· ${ago(m.modified)}</span></div></a>`;
}

function renderList() {
  const words = S.filter.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = S.sessions.filter(m => (!S.agent || m.agent === S.agent) &&
    words.every(w => `${m.title} ${m.cwd} ${m.id} ${m.agent}`.toLowerCase().includes(w)));
  let html = '';
  if (S.group === 'folder') {
    // Folders in order of their latest session. While filtering, all of them are open.
    const folders = new Map();
    const uses = {};
    for (const m of shown) {
      if (!folders.has(m.cwd)) {
        folders.set(m.cwd, []);
        uses[project(m.cwd)] = (uses[project(m.cwd)] || 0) + 1;
      }
      folders.get(m.cwd).push(m);
    }
    for (const [cwd, list] of folders) {
      const open = words.length > 0 || S.folders.has(cwd);
      // Two folders of the same name are told apart by their paths.
      const name = uses[project(cwd)] > 1 ? tilde(cwd) : project(cwd) || '(no folder)';
      html += `<button class="folder${open ? ' open' : ''}" data-cwd="${esc(cwd)}" title="${esc(cwd)}">` +
        `<span>${esc(name)}</span><span class="n">${list.length}</span></button>`;
      if (open) html += list.map(row).join('');
    }
  } else {
    let grp = '';
    for (const m of shown) {
      const g = group(m.modified);
      if (g !== grp) {
        html += `<div class="grp">${esc(g)}</div>`;
        grp = g;
      }
      html += row(m);
    }
  }
  $('#list').innerHTML = html || '<div class="grp">No sessions</div>';
  $('#group').textContent = S.group === 'folder' ? 'By folder' : 'By date';
}

function toggleGrouping() {
  S.group = S.group === 'folder' ? 'date' : 'folder';
  localStorage.setItem('ah.group', S.group);
  renderList();
}

function toggleFolder(cwd) {
  if (!S.folders.delete(cwd)) S.folders.add(cwd);
  localStorage.setItem('ah.folders', JSON.stringify([...S.folders]));
  renderList();
}

let listTimer = 0;
function listSoon() {
  if (!listTimer) listTimer = setTimeout(() => { listTimer = 0; renderList(); }, 400);
}

async function loadSessions() {
  const r = await fetch('api/sessions');
  const d = await r.json();
  S.home = d.home;
  S.public = d.public;
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

/**
 * An event stream that closes and calls `lost` on an error, or when the server has sent nothing,
 * not even a ping, for SILENT_MS. Browsers allow six connections to a host over plain HTTP, and
 * a connection that died with the network (a laptop that slept, a relay that dropped) would
 * otherwise keep its place until the system gives up on it.
 */
function stream(url, on, lost) {
  const es = new EventSource(url);
  let timer = 0;
  const close = () => {
    clearTimeout(timer);
    es.close();
  };
  const fail = () => {
    close();
    lost();
  };
  const alive = () => {
    clearTimeout(timer);
    timer = setTimeout(fail, SILENT_MS);
  };
  es.addEventListener('ping', alive);
  for (const [name, f] of Object.entries(on)) {
    es.addEventListener(name, e => {
      alive();
      f(e);
    });
  }
  es.onopen = alive;
  es.onerror = fail;
  alive();
  return { close };
}

/** Follows changes to the session list; after a gap, `reload` fetches the list again. */
function followList(reload) {
  if (reload) loadSessions();
  S.listEs = stream('api/events', {
    meta: e => upsert(JSON.parse(e.data)),
    gone: e => {
      S.sessions = S.sessions.filter(s => s.id !== e.data);
      listSoon();
    },
    reload: loadSessions,
  }, () => {
    S.listEs = null;
    setTimeout(() => { if (!S.listEs && !document.hidden) followList(true); }, 2000);
  });
}

/** Closes the streams while the page is hidden, so tabs in the background hold no connections. */
function followWhileShown() {
  if (document.hidden) {
    if (S.listEs) S.listEs.close();
    if (S.es) S.es.close();
    S.listEs = S.es = null;
    return;
  }
  if (SHARE) return;
  if (!S.listEs) followList(true);
  if (S.cur && !S.es) subscribe();
}

/** Shows or hides the session list: a drawer on narrow screens, a column that stays hidden otherwise. */
function toggleSide(show) {
  if (narrow.matches) {
    root.classList.toggle('side-open', show);
    return;
  }
  const hide = show === undefined ? !root.classList.contains('side-closed') : !show;
  root.classList.toggle('side-closed', hide);
  localStorage.setItem('ah.side', hide ? '0' : '1');
}

function findSession() {
  toggleSide(true);
  $('#filter').focus();
}

// ---------- conversation ----------

function renderHead() {
  const m = S.cur.meta;
  $('#title').textContent = m.title;
  document.title = `${m.title} · ah`;
  const live = Date.now() - m.modified < LIVE_MS ? '<span class="dot" title="Active"></span>' : '';
  $('#sub').innerHTML = `${live}${agentIcon(m.agent)}` +
    (m.cwd ? `<span title="${esc(m.cwd)}">${esc(tilde(m.cwd))}</span>` : '') +
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
    $('#share').hidden = true;
    document.title = 'ah';
    $('#hint').textContent = r.status === 404 ? 'No such session.' : 'Could not load this session.';
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
  $('#share').hidden = !S.public;
  for (const row of $$('#list .row')) row.classList.toggle('on', row.dataset.id === id);
  root.classList.remove('side-open');
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

/** Follows the open session from its current revision; a hidden page catches up when shown. */
function subscribe() {
  if (document.hidden) return;
  const c = S.cur;
  const es = stream(`api/s/${encodeURIComponent(c.id)}/events?gen=${c.gen}&rev=${c.rev}`, {
    items: e => {
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
    },
    reset: () => {
      if (S.cur === c) open(c.id, S.follow ? null : topItem());
    },
  }, () => {
    // Reconnect by hand so the request carries the latest revision.
    if (S.es !== es) return;
    S.es = null;
    setTimeout(() => { if (S.cur === c && !S.es && !document.hidden) subscribe(); }, 2000);
  });
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
  const text = it.texts.length > 0;
  el.className = `item ${it.role}` + (it.kind ? ` k-${it.kind}` : '') +
    (it.role === 'assistant' && !text ? ' bare' : '') + (it.answer ? ' answer' : '');
  let acts = '';
  if (text) {
    // The time is shown in the reader's own time zone.
    const t = it.time ? `<span>${esc(when(it.time))}</span>` : '';
    const copy = `<button class="copy" title="Copy as Markdown" aria-label="Copy as Markdown">${ICON_COPY}</button>`;
    acts = `<div class="acts">${it.role === 'user' ? t + copy : copy + t}</div>`;
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
      if (m.dataset.tex != null) continue;
      // The TeX stays on the element for copying.
      m.dataset.tex = m.textContent;
      try {
        katex.render(m.dataset.tex, m, { displayMode: m.classList.contains('math-display'), throwOnError: false });
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

/** Opens every group of tool calls and thinking, or closes all folds when some are open. */
function toggleAll() {
  if (!S.cur) return;
  const open = $$('details[open]', conv);
  if (open.length) {
    for (const d of open) d.open = false;
    return;
  }
  if (S.view !== 'all') setView('all');
  for (const d of $$('.item > details.tools, .item > details.thinking', conv)) d.open = true;
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

/** The shown item at the reading line, a third of the way down the view. */
function readingItem() {
  if (!S.cur) return null;
  const y = scroller.scrollTop + scroller.clientHeight / 3;
  let found = null;
  for (const el of S.cur.els) {
    if (!el || !el.offsetParent) continue;
    if (el.offsetTop > y) break;
    found = el;
  }
  return found && S.cur.items[+found.dataset.i];
}

function copyCurrent() {
  const it = readingItem();
  const md = it && itemMarkdown(it);
  if (!md) return toast('Nothing to copy here');
  copyText(md).then(() => toast('Copied as Markdown'), () => toast('Could not copy'));
}

function buildRail() {
  const c = S.cur;
  const turns = [];
  const size = it => it.texts.reduce((n, s) => n + s.length, 0);
  for (const it of c.items) {
    if (!it) continue;
    if (it.role === 'user') turns.push({ i: it.i, size: size(it), answer: '', time: it.time });
    else if (turns.length) {
      const t = turns[turns.length - 1];
      t.size += size(it);
      if (it.role === 'assistant' && it.preview) t.answer = it.preview;
    }
  }
  S.turns = turns;
  // A session of one or two turns fits on a screen or two; the rail would only add noise.
  $('#rail').hidden = turns.length <= 2;
  $('#main').classList.toggle('norail', turns.length <= 2);
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

function prevTurn() {
  const cur = S.cur && S.turns[S.on];
  // Inside a long turn, go back to its start first.
  const offset = cur ? S.cur.els[cur.i].getBoundingClientRect().top - scroller.getBoundingClientRect().top : 0;
  jump(offset < -20 ? S.on : Math.max(S.on - 1, 0));
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

/**
 * Runs `change`, which lays the conversation out anew, and keeps the reader's place: the top
 * item if it stays shown, else the prompt of its turn. An item the view begins inside keeps the
 * same part of it at the top as it grows or shrinks.
 */
function keepPlace(change) {
  const c = S.cur;
  const anchors = [];
  if (c && !S.follow) {
    const i = topItem();
    if (i != null) anchors.push(c.els[i]);
    if (S.turns[S.on]) anchors.push(c.els[S.turns[S.on].i]);
  }
  const at = el => {
    const r = el.getBoundingClientRect();
    return { y: r.top - scroller.getBoundingClientRect().top, h: r.height };
  };
  const before = anchors.map(at);
  change();
  if (S.follow) {
    stick();
  } else {
    const k = anchors.findIndex(el => el.offsetParent);
    if (k >= 0) {
      const was = before[k];
      const now = at(anchors[k]);
      scroller.scrollTop += now.y - (was.y < 0 && was.h ? was.y / was.h * now.h : was.y);
    }
  }
  S.on = -1;
  spy();
}

function setView(v) {
  keepPlace(() => {
    S.view = v;
    localStorage.setItem('ah.view', v);
    root.classList.toggle('chat', v !== 'all');
    root.classList.toggle('answers', v === 'answers');
    renderViewButton();
  });
}

/** Steps the conversation's text size up or down, or back to the default with 0. */
function setScale(step) {
  const k = SCALES.indexOf(S.scale);
  const scale = step ? SCALES[Math.max(0, Math.min(SCALES.length - 1, k + step))] : 1;
  if (scale === S.scale) return;
  keepPlace(() => {
    S.scale = scale;
    localStorage.setItem('ah.scale', scale);
    root.style.setProperty('--scale', scale);
  });
  renderSettings();
}

/** Lays the conversation across the whole window, or in a centered column. */
function setWide(wide) {
  keepPlace(() => {
    S.wide = wide;
    localStorage.setItem('ah.wide', wide ? '1' : '0');
    root.classList.toggle('wide', wide);
  });
  renderSettings();
}

/**
 * Moves the highlight of a group of buttons under its selected one. Hidden groups are left for
 * when they show, and the first placement does not slide.
 */
function slide(group) {
  if (!group.offsetParent) return;
  const on = $('button.on', group);
  const first = !group.style.getPropertyValue('--w');
  if (first) group.classList.add('still');
  group.style.setProperty('--x', (on ? on.offsetLeft : 0) + 'px');
  group.style.setProperty('--w', (on ? on.offsetWidth : 0) + 'px');
  if (first) {
    void group.offsetWidth;
    group.classList.remove('still');
  }
}

function slideAll() {
  for (const g of $$('.slide')) slide(g);
}

/** The text size and width controls at the top of the command list. */
function renderSettings() {
  $('#pal-scale').textContent = Math.round(S.scale * 100) + '%';
  $('#pal-set [data-scale="-1"]').disabled = S.scale === SCALES[0];
  $('#pal-set [data-scale="1"]').disabled = S.scale === SCALES[SCALES.length - 1];
  for (const b of $$('#pal-set [data-wide]')) b.classList.toggle('on', (b.dataset.wide === '1') === S.wide);
  for (const b of $$('#pal-set [data-theme]')) b.classList.toggle('on', b.dataset.theme === root.dataset.theme);
  for (const b of $$('#pal-set [data-look]')) b.classList.toggle('on', b.dataset.look === (root.dataset.style || 'minimal'));
  for (const b of $$('#pal-set [data-face]')) b.classList.toggle('on', b.dataset.face === (root.dataset.font || 'sans'));
  for (const g of $$('#pal-set .slide')) slide(g);
}

/** The header button names the current view and switches to the next one. */
function renderViewButton() {
  const b = $('#view');
  b.innerHTML = `<svg viewBox="0 0 16 16" aria-hidden="true"><path d="${VIEW_ICONS[S.view]}"/></svg>`;
  b.title = VIEW_HELP[S.view];
  b.setAttribute('aria-label', VIEW_HELP[S.view]);
  b.classList.toggle('on', S.view !== 'all');
}

function switchTheme() {
  const t = root.dataset.theme === 'dark' ? 'light' : 'dark';
  setTheme(t);
}

/** The minimal style, or the pixel style: square edges drawn in pixels and hard shadows. */
function setLook(look) {
  keepPlace(() => {
    if (look === 'pixel') root.dataset.style = 'pixel';
    else delete root.dataset.style;
    localStorage.setItem('ah.style', look);
  });
  renderSettings();
}

/** The font of the page and the conversation: sans-serif, monospace or pixel. */
function setFont(face) {
  keepPlace(() => {
    if (face === 'sans') delete root.dataset.font;
    else root.dataset.font = face;
    localStorage.setItem('ah.font', face);
  });
  renderSettings();
  slideAll();
}

function setTheme(t) {
  root.dataset.theme = t;
  localStorage.setItem('ah.theme', t);
  renderSettings();
}

// ---------- commands ----------

/**
 * Every command, listed in the command list; `keys` also run them from the keyboard. Commands
 * marked `own` are the owner's and are left out of shared pages; `when` limits others.
 */
const COMMANDS = [
  { label: 'Show everything', keys: [], own: true, on: () => S.view === 'all', run: () => setView('all') },
  { label: 'Chat only: hide tool calls, thinking and notices', keys: ['t'], own: true, on: () => S.view === 'chat', run: () => setView(S.view === 'chat' ? 'all' : 'chat') },
  { label: 'Answers only: prompts and final answers', keys: ['a'], own: true, on: () => S.view === 'answers', run: () => setView(S.view === 'answers' ? 'all' : 'answers') },
  { label: 'Share this session', keys: [], own: true, when: () => S.public && S.cur, run: openSharing },
  { label: 'Next turn', keys: ['j', ']'], run: () => jump(Math.min(S.on + 1, S.turns.length - 1)) },
  { label: 'Previous turn', keys: ['k', '['], run: prevTurn },
  { label: 'First message', keys: ['g'], run: () => { scroller.scrollTop = 0; } },
  { label: 'Latest message, following new ones', keys: ['G'], run: stick },
  { label: 'Expand or collapse all tool calls', keys: ['e'], own: true, run: toggleAll },
  { label: 'Copy the message at the reading line', keys: ['y'], run: copyCurrent },
  { label: 'Show or hide the sessions', keys: ['s'], own: true, run: () => toggleSide() },
  { label: 'Find a session', keys: ['/'], own: true, run: findSession },
  { label: 'Group the sessions by folder', keys: [], own: true, on: () => S.group === 'folder', run: toggleGrouping },
  { label: 'Larger text', keys: ['+'], run: () => setScale(1) },
  { label: 'Smaller text', keys: ['-'], run: () => setScale(-1) },
  { label: 'Full width: text across the whole window', keys: ['w'], on: () => S.wide, run: () => setWide(!S.wide) },
  { label: 'Switch between light and dark', keys: [], run: switchTheme },
  { label: 'Pixel style: square edges drawn in pixels', keys: [], on: () => root.dataset.style === 'pixel', run: () => setLook(root.dataset.style === 'pixel' ? 'minimal' : 'pixel') },
  { label: 'Sign in on another device', keys: [], own: true, when: () => S.public, run: openSignin },
];

/** The commands this page offers now. */
function commands() {
  return COMMANDS.filter(c => !(SHARE && c.own) && (!c.when || c.when()));
}

const pal = { el: $('#palette'), q: $('#pal-q'), list: $('#pal-list'), shown: [], sel: 0 };

function openPalette() {
  pal.q.value = '';
  pal.sel = 0;
  pal.el.hidden = false;
  renderSettings();
  renderPalette();
  pal.q.focus();
}

function closePalette() {
  pal.el.hidden = true;
}

function renderPalette() {
  const words = pal.q.value.toLowerCase().split(/\s+/).filter(Boolean);
  pal.shown = commands().filter(c => words.every(w => `${c.label} ${c.keys.join(' ')}`.toLowerCase().includes(w)));
  pal.sel = Math.max(0, Math.min(pal.sel, pal.shown.length - 1));
  pal.list.innerHTML = pal.shown.map((c, k) =>
    `<div class="cmd${k === pal.sel ? ' sel' : ''}" data-k="${k}" role="option">` +
    `<span>${esc(c.label)}${c.on && c.on() ? '<span class="check">✓</span>' : ''}</span>` +
    `<span class="keys">${c.keys.map(x => `<kbd>${esc(x)}</kbd>`).join('')}</span></div>`).join('') ||
    '<div class="none">No matching command</div>';
  const sel = pal.list.children[pal.sel];
  if (sel) sel.scrollIntoView({ block: 'nearest' });
}

function runCommand(c) {
  closePalette();
  if (c) c.run();
}

// ---------- sharing ----------

const sh = { el: $('#sharing'), view: 'chat', days: 7 };

/** The share dialog: pick what the link shows and how long it works, and see the open links. */
function openSharing() {
  sh.view = S.view === 'answers' ? 'answers' : 'chat';
  $('#sh-list').innerHTML = '';
  sh.el.hidden = false;
  renderSharing();
  loadShares();
}

function renderSharing() {
  $('#sh-what').textContent = sh.view === 'answers' ? 'the prompts and the final answers' : "the prompts and the agent's messages";
  for (const b of $$('#sh-view button')) b.classList.toggle('on', b.dataset.v === sh.view);
  for (const b of $$('#sh-days button')) b.classList.toggle('on', (b.dataset.d ? +b.dataset.d : null) === sh.days);
  slide($('#sh-view'));
  slide($('#sh-days'));
}

function sharesUrl() {
  return `api/s/${encodeURIComponent(S.cur.id)}/shares`;
}

async function loadShares() {
  const c = S.cur;
  const r = await fetch(sharesUrl());
  if (!r.ok || S.cur !== c) return;
  const list = await r.json();
  $('#sh-list').innerHTML = list.length ? '<div class="sh-h">Open links</div>' + list.map(s =>
    `<div class="shr" data-id="${esc(s.id)}" data-url="${esc(s.url)}">` +
    `<div class="shr-m">${esc(VIEW_NAMES[s.view])} · ${s.expires ? 'until ' + esc(when(s.expires)) : 'no expiry'}</div>` +
    `<a href="${esc(s.url)}" target="_blank" rel="noopener">${esc(s.url)}</a>` +
    '<button data-act="copy">Copy</button><button data-act="stop">Stop</button></div>').join('') : '';
}

async function createShare() {
  const r = await fetch(sharesUrl(), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ view: sh.view, days: sh.days }),
  });
  if (!r.ok) return toast('Could not share this session');
  const s = await r.json();
  loadShares();
  copyText(s.url).then(() => toast('Link copied'), () => toast('Link created'));
}

async function onShareRow(e) {
  const b = e.target.closest('button');
  if (!b) return;
  const row = b.closest('.shr');
  if (b.dataset.act === 'copy') {
    copyText(row.dataset.url).then(() => toast('Link copied'), () => toast('Could not copy'));
    return;
  }
  const r = await fetch('api/shares/' + encodeURIComponent(row.dataset.id), { method: 'DELETE' });
  toast(r.ok || r.status === 404 ? 'Link stopped' : 'Could not stop the link');
  loadShares();
}

/** A login link for another device, as a QR code and as text. */
async function openSignin() {
  const r = await fetch('api/login-link');
  if (!r.ok) return toast('Could not make a login link');
  const d = await r.json();
  $('#si-qr').innerHTML = d.qr;
  $('#si-url').textContent = d.url;
  $('#signin').hidden = false;
}

function closeModals() {
  for (const m of $$('.modal')) m.hidden = true;
}

/** A guest's page: the shared snapshot, without the session list or anything to change it. */
async function openShare() {
  $('#hint').textContent = '';
  const r = await fetch('api/share/' + encodeURIComponent(SHARE));
  if (!r.ok) {
    $('#hint').textContent = 'This link has expired or was stopped.';
    return;
  }
  const snap = await r.json();
  S.view = snap.view;
  root.classList.add('chat');
  root.classList.toggle('answers', snap.view === 'answers');
  S.cur = { id: SHARE, meta: snap.meta, items: [], els: [] };
  for (const it of snap.items) put(it);
  renderHead();
  buildRail();
  spy();
}

// ---------- copying a selection as Markdown ----------

// Stand for a paragraph break and a line break until the converted pieces are joined; each
// absorbs the whitespace around it.
const BREAK = '\u0001';
const LINE = '\u0002';

// Parts of the conversation that are interface rather than transcript.
const CHROME = 'button, .acts, summary, .notice, img.att, .cut';

/** Joins converted pieces: each run of paragraph breaks becomes one blank line. */
function paragraphs(s) {
  return s.replace(/\s*\u0001[\s\u0001]*/g, '\n\n').replace(/\s*\u0002\s*/g, '\n').replace(/^\n+|\n+$/g, '');
}

/** Whether `range` takes in all of the contents of `el`. */
function covers(range, el) {
  const r = document.createRange();
  r.selectNodeContents(el);
  return range.compareBoundaryPoints(Range.START_TO_START, r) <= 0 && range.compareBoundaryPoints(Range.END_TO_END, r) >= 0;
}

/** Wraps `s` in an emphasis mark, keeping surrounding spaces outside it. */
function around(mark, s) {
  const m = s.match(/^(\s*)([\s\S]*?)(\s*)$/);
  return m[2] ? m[1] + mark + m[2] + mark + m[3] : s;
}

function codeSpan(s) {
  const tick = '`'.repeat(Math.max(0, ...(s.match(/`+/g) || []).map(r => r.length)) + 1);
  const pad = s.startsWith('`') || s.endsWith('`') ? ' ' : '';
  return tick + pad + s + pad + tick;
}

/**
 * Markdown for the part of `from` inside `range`. A message the range takes in whole is copied
 * from its source; a part of one is converted back, with formulas as TeX, code blocks as fences,
 * and emphasis, links, lists, quotes and tables in Markdown syntax. What the current view
 * hides, closed folds and buttons are left out.
 */
function rangeMarkdown(range, from) {
  const clip = t => {
    let s = t.data;
    if (t === range.endContainer) s = s.slice(0, range.endOffset);
    if (t === range.startContainer) s = s.slice(range.startOffset);
    return s;
  };
  // With `whole`, the node is converted in full even where the selection does not reach.
  const kids = (el, whole) => Array.from(el.childNodes, n => md(n, whole)).join('');
  const fence = (pre, whole) => {
    const code = $(':scope > code', pre);
    const body = kids(code || pre, whole).replace(/\n$/, '');
    // A raw HTML block is shown as it was written.
    if (!code) return body;
    const lang = (code.className.match(/language-([\w+#.-]+)/) || [])[1] || '';
    const f = '`'.repeat(Math.max(2, ...(body.match(/`+/g) || []).map(r => r.length)) + 1);
    return `${f}${lang}\n${body}\n${f}`;
  };
  const list = (el, whole) => {
    const ordered = el.tagName === 'OL';
    let n = ordered ? +(el.getAttribute('start') || 1) : 1;
    const out = [];
    for (const li of el.children) {
      const num = n++;
      if (li.tagName !== 'LI' || (!whole && !range.intersectsNode(li))) continue;
      const mark = ordered ? `${num}. ` : '- ';
      const lines = paragraphs(kids(li, whole)).split('\n');
      out.push(mark + lines.map((l, k) => (k && l ? ' '.repeat(mark.length) + l : l)).join('\n'));
    }
    return out.join('\n');
  };
  const table = (t, whole) => {
    // Rows are copied whole, with the header, so the result is still a table.
    const cell = c => paragraphs(kids(c, true)).replace(/\s*\n\s*/g, ' ').replace(/\|/g, '\\|');
    const head = t.tHead && t.tHead.rows[0];
    const body = Array.from(t.rows).filter(r => r !== head && (whole || range.intersectsNode(r)));
    const first = head || body.shift();
    if (!first) return '';
    const row = r => `| ${Array.from(r.cells, cell).join(' | ')} |`;
    const rule = Array.from(first.cells, c => ({ left: ':---', center: ':---:', right: '---:' })[c.style.textAlign] || '---');
    return [row(first), `| ${rule.join(' | ')} |`, ...body.map(row)].join('\n');
  };
  const md = (n, whole) => {
    if (!whole && !range.intersectsNode(n)) return '';
    if (n.nodeType === Node.TEXT_NODE) return whole ? n.data : clip(n);
    if (n.nodeType !== Node.ELEMENT_NODE) return '';
    if (n.tagName === 'BR') return '\n';
    if (n.matches(CHROME) || !n.getClientRects().length) return '';
    const item = n.parentElement && n.parentElement.classList.contains('item') && S.cur.items[+n.parentElement.dataset.i];
    if (item && !whole && n.classList.contains('md') && covers(range, n)) {
      return BREAK + item.texts[$$(':scope > .md', n.parentElement).indexOf(n)] + BREAK;
    }
    if (n.classList.contains('math')) {
      const tex = (n.dataset.tex ?? n.textContent).trim();
      return n.classList.contains('math-display') ? `${BREAK}$$\n${tex}\n$$${BREAK}` : `$${tex}$`;
    }
    const inner = () => kids(n, whole);
    switch (n.tagName) {
      case 'P': case 'DIV': case 'ARTICLE': case 'DETAILS': return BREAK + inner() + BREAK;
      case 'H1': case 'H2': case 'H3': case 'H4': case 'H5': case 'H6':
        return BREAK + '#'.repeat(+n.tagName[1]) + ' ' + inner().trim() + BREAK;
      case 'STRONG': case 'B': return around('**', inner());
      case 'EM': case 'I': return around('*', inner());
      case 'DEL': case 'S': return around('~~', inner());
      case 'CODE': return codeSpan(inner());
      case 'PRE': return BREAK + fence(n, whole) + BREAK;
      case 'A': {
        const href = n.getAttribute('href');
        return href && href !== '#' ? `[${inner()}](${href})` : inner();
      }
      case 'IMG': return `![${n.alt}](${n.getAttribute('src') || ''})`;
      case 'INPUT': return n.type === 'checkbox' ? (n.checked ? '[x] ' : '[ ] ') : '';
      case 'BLOCKQUOTE': return BREAK + paragraphs(inner()).replace(/^/gm, '> ').replace(/^> $/gm, '>') + BREAK;
      // A list inside a list item follows its text on the next line.
      case 'UL': case 'OL': return (n.parentElement.tagName === 'LI' ? LINE : BREAK) + list(n, whole) + BREAK;
      case 'TABLE': return BREAK + table(n, whole) + BREAK;
      case 'HR': return BREAK + '---' + BREAK;
      case 'SUP': case 'SUB': case 'KBD': case 'U': {
        const tag = n.tagName.toLowerCase();
        return `<${tag}>${inner()}</${tag}>`;
      }
      default: return inner();
    }
  };
  return paragraphs(md(from, false));
}

/** Markdown for the selection in the conversation, or null to leave copying to the browser. */
function selectionMarkdown() {
  const sel = getSelection();
  if (!sel.rangeCount || sel.isCollapsed) return null;
  const range = sel.getRangeAt(0);
  let el = range.commonAncestorContainer;
  if (el.nodeType !== Node.ELEMENT_NODE) el = el.parentElement;
  // Inside a single code block, its text is what to copy.
  if (!el || !conv.contains(el) || el.closest('pre')) return null;
  // Start at the enclosing block, so emphasis, links and formulas around the selection count.
  const from = el.closest('p, li, h1, h2, h3, h4, h5, h6, th, td, .md, .prompt, .item') || conv;
  return rangeMarkdown(range, from) || null;
}

// ---------- wiring ----------

function route() {
  const m = location.hash.match(/^#\/([^/]+)(?:\/(\d+))?/);
  if (!m) {
    if (narrow.matches) root.classList.add('side-open');
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

/**
 * Dragging the edge of the session list sets its width, and dragging it well past its narrowest
 * width hides the list until the drag comes back; a double click resets the width.
 */
function wireGrip() {
  const grip = $('#grip');
  grip.addEventListener('pointerdown', e => {
    if (e.button !== 0) return;
    e.preventDefault();
    grip.setPointerCapture(e.pointerId);
    root.classList.add('resizing');
    let w = 0;
    const move = ev => {
      const hide = ev.clientX < SIDE_MIN / 2;
      if (hide !== root.classList.contains('side-closed')) toggleSide(!hide);
      if (hide) return;
      w = Math.round(Math.max(SIDE_MIN, Math.min(ev.clientX, 640, innerWidth * 0.6)));
      root.style.setProperty('--side-w', w + 'px');
    };
    grip.addEventListener('pointermove', move);
    grip.addEventListener('lostpointercapture', () => {
      grip.removeEventListener('pointermove', move);
      root.classList.remove('resizing');
      if (w) localStorage.setItem('ah.sideW', w);
    }, { once: true });
  });
  grip.addEventListener('dblclick', () => {
    root.style.removeProperty('--side-w');
    localStorage.removeItem('ah.sideW');
  });
}

function wire() {
  for (const b of $$('#agents button')) b.classList.toggle('on', b.dataset.agent === S.agent);
  slide($('#agents'));
  // Buttons change width when a font arrives.
  document.fonts.addEventListener('loadingdone', slideAll);
  renderViewButton();
  $('#agents').addEventListener('click', e => {
    const b = e.target.closest('button');
    if (!b) return;
    S.agent = b.dataset.agent;
    localStorage.setItem('ah.agent', S.agent);
    for (const x of $$('#agents button')) x.classList.toggle('on', x === b);
    slide($('#agents'));
    renderList();
  });
  $('#filter').addEventListener('input', e => {
    S.filter = e.target.value;
    renderList();
  });
  $('#menu').addEventListener('click', () => toggleSide());
  $('#main').addEventListener('click', e => {
    if (!e.target.closest('#menu')) root.classList.remove('side-open');
  });
  $('#view').addEventListener('click', () => setView(VIEWS[(VIEWS.indexOf(S.view) + 1) % VIEWS.length]));
  $('#group').addEventListener('click', toggleGrouping);
  $('#list').addEventListener('click', e => {
    const f = e.target.closest('.folder');
    if (f) toggleFolder(f.dataset.cwd);
  });
  $('#cmds').addEventListener('click', openPalette);
  $('#share').addEventListener('click', openSharing);
  $('#sh-view').addEventListener('click', e => {
    const b = e.target.closest('button');
    if (!b) return;
    sh.view = b.dataset.v;
    renderSharing();
  });
  $('#sh-days').addEventListener('click', e => {
    const b = e.target.closest('button');
    if (!b) return;
    sh.days = b.dataset.d ? +b.dataset.d : null;
    renderSharing();
  });
  $('#sh-create').addEventListener('click', createShare);
  $('#sh-list').addEventListener('click', onShareRow);
  $('#si-copy').addEventListener('click', () => {
    copyText($('#si-url').textContent).then(() => toast('Link copied'), () => toast('Could not copy'));
  });
  for (const m of $$('.modal')) {
    m.addEventListener('mousedown', e => {
      if (e.target === m) m.hidden = true;
    });
  }
  $('#bottom').addEventListener('click', stick);
  wireGrip();

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
    copyText(itemMarkdown(it)).then(() => {
      b.innerHTML = ICON_DONE;
      setTimeout(() => { b.innerHTML = ICON_COPY; }, 1200);
    }, () => toast('Could not copy'));
  });

  document.addEventListener('copy', e => {
    const md = selectionMarkdown();
    if (md == null) return;
    e.clipboardData.setData('text/plain', md);
    e.preventDefault();
  });

  // `toggle` does not bubble; listen in the capture phase.
  document.addEventListener('toggle', e => {
    const d = e.target;
    if (d.dataset && d.dataset.src && d.open && !d.dataset.loaded) loadBody(d);
  }, true);

  pal.q.addEventListener('input', () => {
    pal.sel = 0;
    renderPalette();
  });
  pal.q.addEventListener('keydown', e => {
    const n = pal.shown.length;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      if (n) pal.sel = (pal.sel + (e.key === 'ArrowDown' ? 1 : n - 1)) % n;
      renderPalette();
    } else if (e.key === 'Enter') {
      runCommand(pal.shown[pal.sel]);
    } else if (e.key === 'Escape') {
      closePalette();
    } else {
      return;
    }
    e.preventDefault();
  });
  pal.list.addEventListener('click', e => {
    const d = e.target.closest('.cmd');
    if (d) runCommand(pal.shown[+d.dataset.k]);
  });
  pal.list.addEventListener('mousemove', e => {
    const d = e.target.closest('.cmd');
    if (!d || +d.dataset.k === pal.sel) return;
    pal.sel = +d.dataset.k;
    for (const x of pal.list.children) x.classList.toggle('sel', x === d);
  });
  pal.el.addEventListener('mousedown', e => {
    if (e.target === pal.el) closePalette();
  });
  // The settings keep the palette open, and the focus in its search field.
  $('#pal-set').addEventListener('mousedown', e => e.preventDefault());
  $('#pal-set').addEventListener('click', e => {
    const b = e.target.closest('button');
    if (!b) return;
    if (b.dataset.scale != null) setScale(+b.dataset.scale);
    else if (b.dataset.theme) setTheme(b.dataset.theme);
    else if (b.dataset.look) setLook(b.dataset.look);
    else if (b.dataset.face) setFont(b.dataset.face);
    else setWide(b.dataset.wide === '1');
  });

  document.addEventListener('keydown', e => {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      if (pal.el.hidden) openPalette();
      else closePalette();
      return;
    }
    if (e.target.closest('input, textarea') || e.metaKey || e.ctrlKey || e.altKey) return;
    if ($$('.modal').some(m => !m.hidden)) {
      if (e.key === 'Escape') closeModals();
      return;
    }
    if (e.key === 'Escape') {
      root.classList.remove('side-open');
      return;
    }
    const c = e.key === '?' ? { run: openPalette } : commands().find(c => c.keys.includes(e.key));
    if (!c) return;
    e.preventDefault();
    c.run();
  });

  if (!SHARE) window.addEventListener('hashchange', route);
  document.addEventListener('visibilitychange', followWhileShown);
  setInterval(() => {
    renderList();
    if (S.cur) renderHead();
  }, 60000);
}

(async function init() {
  wire();
  if (SHARE) return openShare();
  await loadSessions();
  if (!document.hidden) followList(false);
  route();
})();
