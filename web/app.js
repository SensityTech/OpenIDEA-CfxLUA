// Protocol with src/docs: one JSON object per message, `op` names it.
// Page -> plugin:
//   {op:"init"}                                       -> init
//   {op:"search", tab, seq, text, filters, selected?, select?, limit}  -> items
//   {op:"rows", tab, seq, text, filters, offset, limit}                -> rows
//   {op:"detail", tab, id}    (runtime type links: id "type:<Class>")  -> detail
//   {op:"insert", tab, id, variant}   (call | handler | netHandler | directive | example) -> notice
//   {op:"prefs", prefs}       (stored as is, no answer)
//   {op:"openUrl", tab, url}  (a web page, opened in the browser) -> url when the IDE does not open it
//   {op:"updateNatives"}     (runs CfxLua: Update Natives) -> refresh
// Plugin -> page:
//   {op:"init", prefs, open?}
//   {op:"items", tab, seq, text, filters, total, count, empty, offset, rows, selected?, detail?, namespaces?}
//   {op:"rows", tab, seq, offset, rows}
//   {op:"detail", tab, id, html, copy, url?, insert}
//   {op:"open", tab, id?, focusSearch?}
//   {op:"notice", text, error}
//   {op:"url", tab, url}     (an address to show in the tab, to be copied)
//   {op:"refresh", tab}      (search the tab again)
// A row: {id, label, icon?, bold?, strike?, parts?: [[text, class]], tip?, kind?, depth?, children?, expanded?}.
'use strict';

(() => {
  const host = window.openidea || { postMessage() {}, onMessage() {} };
  const send = (message) => host.postMessage(JSON.stringify(message));

  /** Rows per page of the natives list. */
  const BLOCK = 200;
  const ROW_HEIGHT = 22;
  const SIZES = [70, 80, 90, 100, 110, 120, 135, 150, 175, 200];
  const NOTHING = "<p class='empty'>Select an item to see its documentation.</p>";

  const ICONS = {
    insert: '<path d="M2 8h8M7 5l3 3-3 3M13.5 3v10"/>',
    netInsert: '<path d="M6 2v3M10 2v3M4 5h8v2.5a4 4 0 0 1-8 0zM8 11.5V14"/>',
    copy: '<rect x="5.5" y="5.5" width="8" height="8.5" rx="1"/><path d="M3 10.5V3a1 1 0 0 1 1-1h6"/>',
    web: '<circle cx="8" cy="8" r="6"/><path d="M2 8h12M8 2c2.2 2 2.2 10 0 12M8 2c-2.2 2-2.2 10 0 12"/>',
    filter: '<path d="M2 3h12l-4.5 5.5V13l-3-1.5V8.5z"/>',
    refresh: '<path d="M13 8a5 5 0 1 1-1.46-3.54M13.5 2v3h-3"/>',
    zoomOut: '<circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5L14 14M5 7h4"/>',
    zoomIn: '<circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5L14 14M5 7h4M7 5v4"/>',
    zoomReset: '<circle cx="7" cy="7" r="4.5"/><path d="M10.5 10.5L14 14M5 6h4M5 8h4"/>',
    expand: '<path d="M4 4l4 4 4-4M4 9l4 4 4-4"/>',
    collapse: '<path d="M4 7l4-4 4 4M4 12l4-4 4 4"/>',
  };

  const SIDES = [['', 'All sides'], ['client', 'Client'], ['server', 'Server'], ['shared', 'Shared']];
  const zoom = [{ zoom: -1 }, { zoom: 1 }, { zoom: 0 }];
  const sep = { sep: true };
  const online = { icon: 'web', title: 'Open Online Documentation', run: (t) => openUrl(t), enabled: (d) => !!(d && d.url) };
  const insertButton = (variant, icon, title) => ({ icon, title, run: (t) => insert(t, variant), enabled: (d) => !!(d && d.insert.includes(variant)) });
  const copyButton = (key, title) => ({ icon: 'copy', title, run: (t) => copyDetail(t, key), enabled: (d) => !!(d && d.copy[key]) });

  /** The tabs, in order, with what differs between them. */
  const TABS = [
    {
      id: 'natives', title: 'Natives', key: 'Natives', stacked: 0.4, placeholder: 'Name, NATIVE_NAME or hash', label: 'Search natives',
      action: 'call', copy: 'name',
      filters: [
        { key: 'set', title: 'Natives sets to search: the sets enabled for this project, one set, or every set',
          options: [['project', 'Project natives'], ['all', 'All sets'], ['GTA5', 'FiveM'], ['RDR3', 'RedM'], ['SERVER', 'Server']] },
        { key: 'namespace', title: 'Namespace', options: [['', 'All namespaces']] },
        { key: 'side', title: 'Side (client, server or shared)', options: SIDES },
      ],
      toolbar: [
        insertButton('call', 'insert', 'Insert Call at Caret'),
        { icon: 'copy', title: 'Copy', menu: [['name', 'Copy Name'], ['signature', 'Copy Signature'], ['hash', 'Copy Hash'], ['nativeName', 'Copy Native Name']] },
        online, sep,
        { icon: 'filter', title: 'Filter Options', toggles: [['aliases', 'Show Deprecated Aliases'], ['unnamed', 'Show Unnamed Natives']] },
        { icon: 'refresh', title: 'Update Natives Now', run: (t) => updateNatives(t), enabled: () => !updatingNatives },
        sep, ...zoom,
      ],
    },
    {
      id: 'runtime', title: 'Runtime API', key: 'Runtime', stacked: 0.45, placeholder: 'Class, function or field', label: 'Search the runtime API',
      tree: true, nothing: "<p class='empty'>Select a declaration to see its documentation.</p>",
      toolbar: [
        { icon: 'expand', title: 'Expand All', run: (t) => expandAll(t, true), enabled: () => true },
        { icon: 'collapse', title: 'Collapse All', run: (t) => expandAll(t, false), enabled: () => true },
        sep, ...zoom,
      ],
    },
    {
      id: 'events', title: 'Events', key: 'Events', stacked: 0.4, placeholder: 'Event name or description', label: 'Search events',
      action: 'handler', copy: 'name',
      filters: [
        { key: 'side', title: 'Side where the event is raised', options: SIDES },
        { key: 'game', title: 'Game', options: [['', 'All games'], ['gta5', 'FiveM'], ['rdr3', 'RedM']] },
      ],
      toolbar: [
        insertButton('handler', 'insert', 'Insert Event Handler'),
        insertButton('netHandler', 'netInsert', 'Insert Network Event Handler'),
        copyButton('name', 'Copy Name'), online, sep, ...zoom,
      ],
    },
    {
      id: 'manifest', title: 'Manifest', key: 'Manifest', stacked: 0.4, placeholder: 'Directive name or description', label: 'Search manifest directives',
      action: 'directive', copy: 'name', filters: [],
      toolbar: [insertButton('directive', 'insert', 'Insert Directive'), copyButton('name', 'Copy Name'), online, sep, ...zoom],
    },
    {
      id: 'syntax', title: 'Syntax', key: 'Syntax', stacked: 0.4, placeholder: 'Feature, operator or keyword', label: 'Search syntax features',
      action: 'example', copy: 'example', filters: [],
      toolbar: [insertButton('example', 'insert', 'Insert Example'), copyButton('example', 'Copy Example'), sep, ...zoom],
    },
    {
      id: 'guides', title: 'Guides', key: 'Guides', stacked: 0.3, placeholder: 'Title, keyword or text', label: 'Search guides',
      copy: 'link', filters: [],
      toolbar: [copyButton('link', 'Copy Guide Link'), sep, ...zoom],
    },
    {
      id: 'escrow', title: 'Escrow', key: 'Escrow', stacked: 0.3, placeholder: 'Resource name or folder', label: 'Search resources',
      copy: 'report', filters: [],
      nothing: "<p class='empty'>Select a resource to see its Asset Escrow pre-publish report.</p>",
      toolbar: [copyButton('report', 'Copy Report'), sep, ...zoom],
    },
  ];
  const TAB = Object.fromEntries(TABS.map((t) => [t.id, t]));

  let prefs = { fontPercent: 100, nativesSet: 'project', showAliases: true, showUnnamed: true, splits: {}, history: {}, tab: 'natives' };
  let initialized = false;
  let updatingNatives = false;
  let active = null;
  let menu = null;


  function h(tag, attrs, ...children) {
    const el = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (v == null || v === false) continue;
      if (k === 'class') el.className = v;
      else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
      else el.setAttribute(k, v === true ? '' : v);
    }
    for (const c of children.flat()) if (c != null) el.append(c);
    return el;
  }

  function svg(name) {
    const span = document.createElement('span');
    span.innerHTML = `<svg viewBox="0 0 16 16" aria-hidden="true">${ICONS[name]}</svg>`;
    return span.firstChild;
  }

  function chevron() {
    const span = document.createElement('span');
    span.innerHTML = '<svg class="chevron" viewBox="0 0 7 16" aria-hidden="true"><path d="M1 7l2.5 2.5L6 7"/></svg>';
    return span.firstChild;
  }

  let savePending = 0;
  function savePrefs() {
    clearTimeout(savePending);
    savePending = setTimeout(() => send({ op: 'prefs', prefs }), 400);
  }

  let noticeTimer = 0;
  function notice(text, { error = false, copyText = null } = {}) {
    const box = document.getElementById('notice');
    box.replaceChildren();
    box.classList.toggle('error', error);
    box.append(h('span', { class: 'text' }, text));
    let input = null;
    if (copyText != null) {
      input = h('input', { readonly: true, 'aria-label': 'Text to copy' });
      input.value = copyText;
      box.append(input);
    }
    box.append(h('button', { type: 'button', onclick: () => { box.hidden = true; } }, 'Close'));
    box.hidden = false;
    if (input) {
      input.focus();
      input.select();
    }
    clearTimeout(noticeTimer);
    if (!error && copyText == null) noticeTimer = setTimeout(() => { box.hidden = true; }, 3500);
  }

  async function copy(text) {
    try {
      if (!navigator.clipboard) throw new Error('no clipboard');
      await navigator.clipboard.writeText(text);
      notice('Copied to the clipboard.');
    } catch {
      notice('The clipboard is not available here: press Ctrl+C (Cmd+C) to copy the selected text.', { copyText: text });
    }
  }


  const state = {};
  const ui = {};

  function initialFilters(t) {
    const f = { set: 'project', namespace: '', side: '', game: '', aliases: true, unnamed: true };
    if (t.id === 'natives') Object.assign(f, { set: prefs.nativesSet || 'project', aliases: prefs.showAliases !== false, unnamed: prefs.showUnnamed !== false });
    return f;
  }

  function build(t) {
    state[t.id] = {
      text: '', filters: initialFilters(t), seq: 0, started: false, loaded: false, total: 0, rows: [], blocks: new Set(),
      count: '', empty: 'Loading…', selected: -1, selId: null, pending: null, detail: null, typeRequest: null,
      all: [], visible: [], expanded: new Set(), namespaces: [],
    };
    const tabButton = h('button', { type: 'button', role: 'tab', id: `tab-${t.id}`, 'aria-controls': `pane-${t.id}`, 'aria-selected': 'false',
      onclick: () => activate(t.id) }, t.title);
    document.getElementById('tabs').append(tabButton);

    const input = h('input', { type: 'search', placeholder: t.placeholder, 'aria-label': t.label, list: `history-${t.id}`, spellcheck: 'false', autocomplete: 'off' });
    const history = h('datalist', { id: `history-${t.id}` });
    const selects = {};
    const count = h('span', { class: 'count' });
    const filters = (t.filters || []).map((f) => {
      const select = h('select', { title: f.title, 'aria-label': f.title });
      fillSelect(select, f.options, state[t.id].filters[f.key]);
      select.addEventListener('change', () => filterChanged(t, f.key, select.value));
      selects[f.key] = select;
      return select;
    });
    const inner = h('div', { class: 'inner' });
    const empty = h('div', { class: 'empty-text' }, 'Loading…');
    const list = h('div', { class: 'list', tabindex: '0', role: t.tree ? 'tree' : 'listbox', 'aria-label': t.title }, inner, empty);
    const listPanel = h('div', { class: 'list-panel' + (t.tree ? ' tree' : '') },
      h('div', { class: 'search' }, input, history),
      t.tree ? null : h('div', { class: 'filters' }, filters, count),
      list);
    const toolbar = h('div', { class: 'toolbar', role: 'toolbar', 'aria-label': `${t.title} actions` });
    const url = h('span', { class: 'url' });
    const urlbar = h('div', { class: 'urlbar', hidden: true },
      h('span', null, 'Open in a browser:'), url,
      h('button', { type: 'button', onclick: () => copy(url.textContent) }, 'Copy'),
      h('button', { type: 'button', 'aria-label': 'Close', onclick: () => { urlbar.hidden = true; } }, '×'));
    const main = h('main', { class: 'doc' });
    main.innerHTML = t.nothing || NOTHING;
    const page = h('div', { class: 'page', tabindex: '-1' }, main);
    const divider = h('div', { class: 'divider', role: 'separator', 'aria-orientation': 'vertical' });
    const split = h('div', { class: 'split stacked' }, listPanel, divider, h('div', { class: 'detail-panel' }, toolbar, urlbar, page));
    const pane = h('section', { class: 'pane', id: `pane-${t.id}`, role: 'tabpanel', 'aria-labelledby': `tab-${t.id}`, hidden: true }, split);
    document.getElementById('panes').append(pane);
    ui[t.id] = { tabButton, input, history, selects, count, list, inner, empty, listPanel, toolbar, urlbar, url, main, page, divider, split, pane, mode: 'stacked', buttons: [] };

    buildToolbar(t);
    fillHistory(t);
    let timer = 0;
    input.addEventListener('input', () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        state[t.id].text = input.value;
        search(t);
      }, 80);
    });
    input.addEventListener('keydown', (e) => searchKey(t, e));
    list.addEventListener('keydown', (e) => listKey(t, e));
    list.addEventListener('scroll', () => renderList(t));
    list.addEventListener('mousedown', (e) => rowClick(t, e));
    list.addEventListener('dblclick', (e) => {
      const pos = rowPosition(e);
      if (t.tree && pos != null && !e.target.dataset.twisty) toggle(t, dataIndex(t, pos));
    });
    page.addEventListener('click', (e) => linkClick(t, e));
    page.addEventListener('auxclick', (e) => linkClick(t, e));
    divider.addEventListener('pointerdown', (e) => drag(t, e));
    new ResizeObserver(() => layout(t)).observe(split);
  }

  function fillSelect(select, options, value) {
    select.replaceChildren(...options.map(([v, label]) => h('option', { value: v }, label)));
    select.value = options.some(([v]) => v === value) ? value : options[0][0];
  }

  function fillHistory(t) {
    const items = (prefs.history && prefs.history[t.id]) || [];
    ui[t.id].history.replaceChildren(...items.map((text) => h('option', { value: text })));
  }

  function remember(t) {
    const text = state[t.id].text.trim();
    if (!text) return;
    prefs.history = prefs.history || {};
    const items = (prefs.history[t.id] || []).filter((x) => x !== text);
    items.unshift(text);
    prefs.history[t.id] = items.slice(0, 10);
    fillHistory(t);
    savePrefs();
  }


  function buildToolbar(t) {
    const bar = ui[t.id].toolbar;
    for (const item of t.toolbar) {
      if (item.sep) {
        bar.append(h('span', { class: 'sep', role: 'separator' }));
        continue;
      }
      if (item.zoom != null) {
        const z = item.zoom;
        const title = z < 0 ? 'Decrease Documentation Font Size' : z > 0 ? 'Increase Documentation Font Size' : 'Reset Documentation Font Size';
        const button = h('button', { type: 'button', title, 'aria-label': title, onclick: () => resize(z) }, svg(z < 0 ? 'zoomOut' : z > 0 ? 'zoomIn' : 'zoomReset'));
        button.dataset.zoom = String(z);
        bar.append(button);
        continue;
      }
      const button = h('button', { type: 'button', title: item.title, 'aria-label': item.title }, svg(item.icon), item.menu || item.toggles ? chevron() : null);
      if (item.menu) {
        button.setAttribute('aria-haspopup', 'menu');
        button.addEventListener('click', () => {
          const d = state[t.id].detail;
          openMenu(button, item.menu.map(([key, title]) => ({ title, disabled: !(d && d.copy[key]), run: () => copyDetail(t, key) })));
        });
        ui[t.id].buttons.push({ button, enabled: (d) => !!(d && Object.keys(d.copy).length) });
      } else if (item.toggles) {
        button.setAttribute('aria-haspopup', 'menu');
        button.addEventListener('click', () => {
          const f = state[t.id].filters;
          openMenu(button, item.toggles.map(([key, title]) => ({ title, checked: f[key], run: () => toggleFilter(t, key) })));
        });
      } else {
        button.addEventListener('click', () => item.run(t));
        ui[t.id].buttons.push({ button, enabled: item.enabled });
      }
      bar.append(button);
    }
    updateToolbar(t);
  }

  function updateToolbar(t) {
    const d = state[t.id].detail;
    for (const { button, enabled } of ui[t.id].buttons) button.disabled = !enabled(d);
    updateZoom(t);
  }

  function updateZoom(t) {
    const p = prefs.fontPercent;
    for (const button of ui[t.id].toolbar.querySelectorAll('button[data-zoom]')) {
      const z = Number(button.dataset.zoom);
      button.disabled = z < 0 ? p <= SIZES[0] : z > 0 ? p >= SIZES[SIZES.length - 1] : p === 100;
      if (z === 0) {
        const title = `Reset Documentation Font Size (${p}%)`;
        button.title = title;
        button.setAttribute('aria-label', title);
      }
    }
  }

  function openMenu(anchor, items) {
    closeMenu();
    const box = h('div', { class: 'menu', role: 'menu' });
    for (const item of items) {
      const attrs = { type: 'button', role: item.checked == null ? 'menuitem' : 'menuitemcheckbox', disabled: item.disabled };
      if (item.checked != null) attrs['aria-checked'] = String(item.checked);
      box.append(h('button', Object.assign(attrs, { onclick: () => { closeMenu(); item.run(); } }), item.title));
    }
    document.body.append(box);
    const r = anchor.getBoundingClientRect();
    box.style.left = `${Math.max(4, Math.min(r.left, window.innerWidth - box.offsetWidth - 4))}px`;
    box.style.top = `${r.bottom + 2}px`;
    menu = box;
    const first = box.querySelector('button:not(:disabled)');
    if (first) first.focus();
  }

  function closeMenu() {
    if (menu) menu.remove();
    menu = null;
  }

  document.addEventListener('mousedown', (e) => { if (menu && !menu.contains(e.target)) closeMenu(); });
  document.addEventListener('keydown', (e) => {
    if (!menu) return;
    const buttons = [...menu.querySelectorAll('button:not(:disabled)')];
    const i = buttons.indexOf(document.activeElement);
    if (e.key === 'Escape') closeMenu();
    else if (e.key === 'ArrowDown' && buttons.length) buttons[(i + 1) % buttons.length].focus();
    else if (e.key === 'ArrowUp' && buttons.length) buttons[(i - 1 + buttons.length) % buttons.length].focus();
    else return;
    e.preventDefault();
  });

  function resize(direction) {
    const p = prefs.fontPercent;
    const next = direction < 0 ? [...SIZES].reverse().find((s) => s < p) : direction > 0 ? SIZES.find((s) => s > p) : 100;
    if (next == null || next === p) return;
    applyFont(next);
    savePrefs();
  }

  function applyFont(percent) {
    prefs.fontPercent = SIZES.includes(percent) ? percent : 100;
    document.documentElement.style.setProperty('--doc-scale', String(prefs.fontPercent / 100));
    for (const t of TABS) updateZoom(t);
  }


  function activate(id) {
    const t = TAB[id] || TABS[0];
    active = t.id;
    for (const other of TABS) {
      ui[other.id].pane.hidden = other.id !== t.id;
      ui[other.id].tabButton.setAttribute('aria-selected', String(other.id === t.id));
      ui[other.id].tabButton.tabIndex = other.id === t.id ? 0 : -1;
    }
    if (prefs.tab !== t.id) {
      prefs.tab = t.id;
      savePrefs();
    }
    const st = state[t.id];
    if (!st.started) {
      st.started = true;
      search(t);
    }
    layout(t);
  }

  /** Shows item `id` of tab `tabId` (a link, `show-docs`). */
  function open(tabId, id, focusSearch) {
    const t = TAB[tabId];
    if (!t) return;
    const st = state[t.id];
    if (id) st.pending = id;
    const started = st.started;
    activate(t.id);
    if (id && started) search(t);
    if (focusSearch) {
      ui[t.id].input.focus();
      ui[t.id].input.select();
    }
  }

  document.getElementById('tabs').addEventListener('keydown', (e) => {
    const i = TABS.findIndex((t) => t.id === active);
    const next = e.key === 'ArrowRight' ? i + 1 : e.key === 'ArrowLeft' ? i - 1 : null;
    if (next == null) return;
    const t = TABS[(next + TABS.length) % TABS.length];
    activate(t.id);
    ui[t.id].tabButton.focus();
    e.preventDefault();
  });


  function layout(t) {
    const u = ui[t.id];
    const width = u.split.clientWidth;
    if (!width) return;
    const side = u.mode === 'side' ? width >= 660 : width >= 700;
    u.mode = side ? 'side' : 'stacked';
    u.split.classList.toggle('side', side);
    u.split.classList.toggle('stacked', !side);
    u.divider.setAttribute('aria-orientation', side ? 'vertical' : 'horizontal');
    const saved = prefs.splits && prefs.splits[t.key] && prefs.splits[t.key][u.mode];
    const p = typeof saved === 'number' ? saved : side ? 0.34 : t.stacked;
    u.listPanel.style.flexBasis = `${Math.min(0.9, Math.max(0.1, p)) * 100}%`;
    renderList(t);
  }

  function drag(t, e) {
    const u = ui[t.id];
    e.preventDefault();
    u.divider.setPointerCapture(e.pointerId);
    let p = null;
    const move = (ev) => {
      const r = u.split.getBoundingClientRect();
      const raw = u.mode === 'side' ? (ev.clientX - r.left) / r.width : (ev.clientY - r.top) / r.height;
      p = Math.min(0.9, Math.max(0.1, raw));
      u.listPanel.style.flexBasis = `${p * 100}%`;
      renderList(t);
    };
    const up = () => {
      u.divider.removeEventListener('pointermove', move);
      u.divider.removeEventListener('pointerup', up);
      if (p == null) return;
      prefs.splits = prefs.splits || {};
      prefs.splits[t.key] = Object.assign({}, prefs.splits[t.key], { [u.mode]: Math.round(p * 1000) / 1000 });
      savePrefs();
    };
    u.divider.addEventListener('pointermove', move);
    u.divider.addEventListener('pointerup', up);
  }


  function search(t) {
    const st = state[t.id];
    st.seq += 1;
    send({ op: 'search', tab: t.id, seq: st.seq, text: st.text, filters: st.filters, selected: st.selId, select: st.pending, limit: t.tree ? 100000 : BLOCK });
  }

  function filterChanged(t, key, value) {
    const st = state[t.id];
    st.filters[key] = value;
    if (t.id === 'natives' && key === 'set') {
      prefs.nativesSet = value;
      savePrefs();
    }
    search(t);
  }

  function toggleFilter(t, key) {
    const st = state[t.id];
    st.filters[key] = !st.filters[key];
    if (key === 'aliases') prefs.showAliases = st.filters[key];
    if (key === 'unnamed') prefs.showUnnamed = st.filters[key];
    savePrefs();
    search(t);
  }

  function onItems(m) {
    const t = TAB[m.tab];
    const st = t && state[t.id];
    if (!st || m.seq !== st.seq) return;
    const u = ui[t.id];
    st.loaded = true;
    st.pending = null;
    if (m.text !== st.text) {
      st.text = m.text;
      u.input.value = m.text;
    }
    st.filters = Object.assign(st.filters, m.filters);
    if (m.namespaces) {
      st.namespaces = m.namespaces;
      if (st.filters.namespace && !m.namespaces.includes(st.filters.namespace)) {
        st.filters.namespace = '';
        fillSelect(u.selects.namespace, [['', 'All namespaces'], ...m.namespaces.map((n) => [n, n])], '');
        search(t);
        return;
      }
      fillSelect(u.selects.namespace, [['', 'All namespaces'], ...m.namespaces.map((n) => [n, n])], st.filters.namespace);
    }
    for (const [key, select] of Object.entries(u.selects)) {
      if (key !== 'namespace' && select.value !== st.filters[key]) select.value = st.filters[key];
    }
    st.total = m.total;
    st.count = m.count;
    st.empty = m.empty;
    st.rows = [];
    m.rows.forEach((r, k) => { st.rows[m.offset + k] = r; });
    st.blocks = new Set([0]);
    st.selected = m.selected == null ? -1 : m.selected;
    if (t.tree) {
      st.all = m.rows;
      st.expanded = new Set(m.rows.filter((r) => r.expanded).map((r) => r.id));
      revealAncestors(st, st.selected);
      computeVisible(st);
    }
    st.selId = st.selected >= 0 ? (rowAt(st, st.selected) || m.detail || {}).id || null : null;
    st.typeRequest = null;
    u.count.textContent = st.count;
    showDetail(t, m.detail || null);
    renderList(t);
    scrollToSelection(t);
  }

  function onRows(m) {
    const st = state[m.tab];
    if (!st || m.seq !== st.seq) return;
    m.rows.forEach((r, k) => { st.rows[m.offset + k] = r; });
    renderList(TAB[m.tab]);
  }

  function onDetail(m) {
    const t = TAB[m.tab];
    const st = t && state[t.id];
    if (!st || (m.id !== st.selId && m.id !== st.typeRequest)) return;
    showDetail(t, m);
  }

  function showDetail(t, d) {
    const st = state[t.id];
    const u = ui[t.id];
    st.detail = d;
    u.main.innerHTML = d ? d.html : t.nothing || NOTHING;
    u.page.scrollTop = 0;
    u.urlbar.hidden = true;
    updateToolbar(t);
  }


  function rowAt(st, i) {
    return st.all.length ? st.all[i] : st.rows[i];
  }

  /** Rows shown: every row, or the expanded part of the runtime tree. */
  function shownCount(t) {
    const st = state[t.id];
    return t.tree ? st.visible.length : st.total;
  }

  function dataIndex(t, pos) {
    return t.tree ? state[t.id].visible[pos] : pos;
  }

  function position(t, index) {
    if (!t.tree) return index;
    const pos = state[t.id].visible.indexOf(index);
    return pos < 0 ? null : pos;
  }

  function computeVisible(st) {
    st.visible = [];
    let hideBelow = Infinity;
    st.all.forEach((r, k) => {
      if (r.depth > hideBelow) return;
      hideBelow = Infinity;
      st.visible.push(k);
      if (r.children && !st.expanded.has(r.id)) hideBelow = r.depth;
    });
  }

  function revealAncestors(st, index) {
    if (index < 0 || !st.all[index]) return;
    let depth = st.all[index].depth;
    for (let k = index - 1; k >= 0 && depth > 0; k--) {
      if (st.all[k].depth < depth) {
        st.expanded.add(st.all[k].id);
        depth = st.all[k].depth;
      }
    }
  }

  function renderList(t) {
    if (!t) return;
    const st = state[t.id];
    const u = ui[t.id];
    const count = shownCount(t);
    u.inner.style.height = `${count * ROW_HEIGHT}px`;
    u.empty.hidden = count > 0;
    u.empty.textContent = st.loaded ? st.empty : 'Loading…';
    if (!count || !u.list.clientHeight) {
      u.inner.replaceChildren();
      return;
    }
    const first = Math.max(0, Math.floor(u.list.scrollTop / ROW_HEIGHT) - 5);
    const last = Math.min(count - 1, Math.ceil((u.list.scrollTop + u.list.clientHeight) / ROW_HEIGHT) + 5);
    if (!t.tree) ensureRows(t, first, last);
    const els = [];
    for (let pos = first; pos <= last; pos++) els.push(rowElement(t, pos));
    u.inner.replaceChildren(...els);
  }

  function ensureRows(t, first, last) {
    const st = state[t.id];
    for (let b = Math.floor(first / BLOCK); b <= Math.floor(last / BLOCK); b++) {
      if (st.blocks.has(b)) continue;
      st.blocks.add(b);
      send({ op: 'rows', tab: t.id, seq: st.seq, text: st.text, filters: st.filters, offset: b * BLOCK, limit: BLOCK });
    }
  }

  function rowElement(t, pos) {
    const st = state[t.id];
    const index = dataIndex(t, pos);
    const row = rowAt(st, index);
    const selected = index === st.selected;
    const el = h('div', { class: 'row', role: t.tree ? 'treeitem' : 'option', 'aria-selected': String(selected), 'data-pos': String(pos) });
    el.style.top = `${pos * ROW_HEIGHT}px`;
    if (!row) {
      el.classList.add('placeholder');
      el.textContent = '…';
      return el;
    }
    if (t.tree) {
      el.style.paddingLeft = `${4 + row.depth * 16}px`;
      const open = st.expanded.has(row.id);
      if (row.children) el.setAttribute('aria-expanded', String(open));
      el.append(h('span', { class: row.children ? 'twisty' : 'twisty leaf', 'data-twisty': '1' }, row.children ? (open ? '▾' : '▸') : ''));
    }
    if (row.kind === 'category') el.classList.add('category');
    if (row.icon) el.append(h('span', { class: 'ico', 'data-i': row.icon }));
    el.append(h('span', { class: ['label', row.bold && 'bold', row.strike && 'strike'].filter(Boolean).join(' ') }, row.label));
    for (const [text, cls] of row.parts || []) el.append(h('span', { class: `part ${cls}` }, text));
    if (row.tip) el.title = row.tip;
    return el;
  }

  function rowPosition(e) {
    const el = e.target.closest('.row');
    return el ? Number(el.dataset.pos) : null;
  }

  function rowClick(t, e) {
    if (e.button !== 0) return;
    const pos = rowPosition(e);
    if (pos == null) return;
    const index = dataIndex(t, pos);
    if (t.tree && e.target.dataset.twisty) {
      toggle(t, index);
      e.preventDefault();
      return;
    }
    select(t, index);
  }

  function select(t, index) {
    const st = state[t.id];
    const count = t.tree ? st.all.length : st.total;
    if (index < 0 || index >= count || index === st.selected) return;
    st.selected = index;
    const row = rowAt(st, index);
    st.selId = row ? row.id : null;
    st.typeRequest = null;
    renderList(t);
    scrollToSelection(t);
    if (st.selId) send({ op: 'detail', tab: t.id, id: st.selId });
  }

  /** Selects the row `delta` shown rows away from the selection. */
  function move(t, delta) {
    const st = state[t.id];
    const count = shownCount(t);
    if (!count) return;
    const current = st.selected < 0 ? null : position(t, st.selected);
    const pos = current == null ? 0 : Math.max(0, Math.min(count - 1, current + delta));
    select(t, dataIndex(t, pos));
  }

  function scrollToSelection(t) {
    const st = state[t.id];
    const u = ui[t.id];
    const pos = st.selected < 0 ? null : position(t, st.selected);
    if (pos == null) return;
    const top = pos * ROW_HEIGHT;
    if (top < u.list.scrollTop) u.list.scrollTop = top;
    else if (top + ROW_HEIGHT > u.list.scrollTop + u.list.clientHeight) u.list.scrollTop = top + ROW_HEIGHT - u.list.clientHeight;
    renderList(t);
  }

  function pageRows(t) {
    return Math.max(1, Math.floor(ui[t.id].list.clientHeight / ROW_HEIGHT) - 1);
  }

  function searchKey(t, e) {
    const keys = { ArrowDown: 1, ArrowUp: -1, PageDown: pageRows(t), PageUp: -pageRows(t) };
    if (e.key in keys) {
      move(t, keys[e.key]);
      e.preventDefault();
    } else if (e.key === 'Enter') {
      remember(t);
      defaultAction(t);
      e.preventDefault();
    }
  }

  function listKey(t, e) {
    const st = state[t.id];
    const keys = { ArrowDown: 1, ArrowUp: -1, PageDown: pageRows(t), PageUp: -pageRows(t), Home: -Infinity, End: Infinity };
    if (e.key in keys) {
      const d = keys[e.key];
      move(t, Number.isFinite(d) ? d : d < 0 ? -shownCount(t) : shownCount(t));
    } else if (e.key === 'Enter') {
      if (t.tree) toggle(t, st.selected);
      else defaultAction(t);
    } else if (t.tree && (e.key === 'ArrowRight' || e.key === 'ArrowLeft')) {
      treeKey(t, e.key === 'ArrowRight');
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'c' && !String(window.getSelection())) {
      const text = t.copy ? st.detail && st.detail.copy[t.copy] : (rowAt(st, st.selected) || {}).label;
      if (text) copy(text);
    } else {
      return;
    }
    e.preventDefault();
  }

  function defaultAction(t) {
    if (t.action) insert(t, t.action);
  }


  function toggle(t, index) {
    const st = state[t.id];
    const row = st.all[index];
    if (!row || !row.children) return;
    if (st.expanded.has(row.id)) st.expanded.delete(row.id);
    else st.expanded.add(row.id);
    computeVisible(st);
    renderList(t);
  }

  function treeKey(t, right) {
    const st = state[t.id];
    const row = st.all[st.selected];
    if (!row) return;
    const open = st.expanded.has(row.id);
    if (right) {
      if (row.children && !open) toggle(t, st.selected);
      else if (row.children) select(t, st.selected + 1);
    } else if (row.children && open) {
      toggle(t, st.selected);
    } else {
      for (let k = st.selected - 1; k >= 0; k--) {
        if (st.all[k].depth < row.depth) {
          select(t, k);
          break;
        }
      }
    }
  }

  function expandAll(t, expand) {
    const st = state[t.id];
    st.expanded = new Set(expand ? st.all.filter((r) => r.children).map((r) => r.id) : []);
    computeVisible(st);
    renderList(t);
    scrollToSelection(t);
  }


  function updateNatives(t) {
    if (updatingNatives) return;
    updatingNatives = true;
    updateToolbar(t);
    notice('Updating natives…');
    send({ op: 'updateNatives' });
  }

  function onRefresh(id) {
    const t = TABS.find((x) => x.id === id);
    if (!t) return;
    if (id === 'natives') updatingNatives = false;
    updateToolbar(t);
    document.getElementById('notice').hidden = true;
    if (state[t.id].started) search(t);
  }

  function insert(t, variant) {
    const st = state[t.id];
    if (!st.selId || !st.detail || !st.detail.insert.includes(variant)) return;
    send({ op: 'insert', tab: t.id, id: st.selId, variant });
  }

  function copyDetail(t, key) {
    const d = state[t.id].detail;
    if (d && d.copy[key]) copy(d.copy[key]);
  }

  /** Opens a web page in the browser, through the plugin: this frame opens nothing by itself. */
  function openUrl(t, href) {
    const url = href || (state[t.id].detail && state[t.id].detail.url);
    if (!url) return;
    if (/^https?:/i.test(url)) send({ op: 'openUrl', tab: t.id, url });
    else showUrl(t, url);
  }

  /** Shows an address the browser was not given, to be copied. */
  function showUrl(t, url) {
    const u = ui[t.id];
    u.url.textContent = url;
    u.url.title = url;
    u.urlbar.hidden = false;
  }

  /** Links of a page: anchors, other pages of the tool window, online pages. */
  function linkClick(t, e) {
    const a = e.target.closest && e.target.closest('a[href]');
    if (!a || e.defaultPrevented || e.button > 1) return;
    e.preventDefault();
    const href = a.getAttribute('href');
    if (href.startsWith('#')) {
      const target = ui[t.id].main.querySelector(`[id="${CSS.escape(href.slice(1))}"], [name="${CSS.escape(href.slice(1))}"]`);
      if (target) target.scrollIntoView();
    } else if (href.startsWith('cfxdoc:')) {
      open('guides', href.slice(7).split('#')[0].trim());
    } else if (href.startsWith('cfxnative:')) {
      open('natives', href.slice(10));
    } else if (href.startsWith('cfxtype:')) {
      const st = state[t.id];
      st.typeRequest = `type:${href.slice(8)}`;
      send({ op: 'detail', tab: t.id, id: st.typeRequest });
    } else if (/^(https?:|mailto:)/i.test(href)) {
      openUrl(t, href);
    }
  }


  function applyPrefs(p) {
    if (p && typeof p === 'object') prefs = Object.assign(prefs, p);
    applyFont(Number(prefs.fontPercent) || 100);
    for (const t of TABS) {
      state[t.id].filters = initialFilters(t);
      for (const [key, select] of Object.entries(ui[t.id].selects)) select.value = state[t.id].filters[key] || '';
      fillHistory(t);
    }
  }

  host.onMessage((raw) => {
    let m;
    try {
      m = typeof raw === 'string' ? JSON.parse(raw) : raw;
    } catch {
      return;
    }
    if (!m || typeof m !== 'object') return;
    switch (m.op) {
      case 'init':
        // A refresh posted while the page was hidden is lost.
        if (updatingNatives) onRefresh('natives');
        if (!initialized) {
          initialized = true;
          applyPrefs(m.prefs);
          if (m.open) open(m.open.tab, m.open.id, m.open.focusSearch);
          else activate(prefs.tab);
        } else if (m.open) {
          open(m.open.tab, m.open.id, m.open.focusSearch);
        }
        break;
      case 'items': onItems(m); break;
      case 'rows': onRows(m); break;
      case 'detail': onDetail(m); break;
      case 'open': open(m.tab, m.id, m.focusSearch); break;
      case 'notice': notice(m.text, { error: !!m.error }); break;
      case 'url': {
        const t = TABS.find((x) => x.id === m.tab);
        if (t && m.url) showUrl(t, m.url);
        break;
      }
      case 'refresh': onRefresh(m.tab); break;
      default: break;
    }
  });

  for (const t of TABS) build(t);
  applyFont(100);

  // Messages to a hidden view are lost: ask for the state again whenever the page is shown.
  let tries = 0;
  const hello = () => {
    if (initialized || tries++ >= 5) return;
    send({ op: 'init' });
    setTimeout(hello, 1500);
  };
  hello();
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible' && initialized) send({ op: 'init' });
  });
  window.addEventListener('resize', () => { if (active) layout(TAB[active]); });
})();
