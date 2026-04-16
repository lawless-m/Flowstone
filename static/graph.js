(() => {
  const svg = d3.select('#graph');
  let graph = { nodes: [], links: [] };
  let simulation = null;
  let selected = null;
  let activeTag = null;
  let missingTagSeq = 0;
  let bodySeq = 0;
  let allTags = [];
  let currentView = 'net';
  let tagGraph = { nodes: [], links: [] };
  let tagSimulation = null;
  let tagNetDirty = true;

  // Initialise display state using inline styles so setView's style.display
  // assignments always win over HTML `hidden` attributes + UA stylesheet.
  document.getElementById('cloud').style.display    = 'none';
  document.getElementById('tag-net').style.display  = 'none';
  document.getElementById('tags').style.display     = 'block';

  async function loadTagGraph() {
    try {
      const res = await fetch('/api/tag-graph');
      tagGraph = await res.json();
      tagNetDirty = true;
      if (currentView === 'tagnet') renderTagNet();
    } catch (e) {
      console.error('[tag-graph]', e);
    }
  }

  async function loadGraph() {
    const res = await fetch('/api/graph');
    graph = await res.json();
    render();
    updateStats();
  }

  async function loadTags() {
    try {
      const res = await fetch('/api/tags');
      const data = await res.json();
      renderTags(data.tags || []);
    } catch (e) {
      console.error('[tags]', e);
    }
  }

  function renderTags(tags) {
    allTags = tags;
    const themes = tags.filter(t => t.count >= 2);
    const orphans = tags.filter(t => t.count < 2);
    renderTagList('tag-list', themes);
    renderTagList('orphan-list', orphans);
    const heading = document.getElementById('orphan-heading');
    const counter = document.getElementById('orphan-count');
    heading.hidden = orphans.length === 0;
    counter.textContent = orphans.length ? `(${orphans.length})` : '';
    if (currentView === 'cloud') renderCloud();
  }

  function renderTagList(ulId, tags) {
    const ul = document.getElementById(ulId);
    ul.innerHTML = '';
    for (const t of tags) {
      const li = document.createElement('li');
      if (!t.resolved) li.classList.add('unresolved');
      if (activeTag === t.target) li.classList.add('active');

      const name = document.createElement('span');
      name.className = 'tag-name';
      name.textContent = t.target;

      const count = document.createElement('span');
      count.className = 'tag-count';
      count.textContent = t.count;

      li.appendChild(name);
      li.appendChild(count);
      li.addEventListener('click', () => {
        if (activeTag === t.target) {
          setActiveTag(null);
        } else {
          setActiveTag(t.target);
        }
      });
      if (!t.resolved) {
        li.addEventListener('dblclick', async (e) => {
          e.preventDefault();
          await createNoteAndSelect(t.target);
        });
      }
      ul.appendChild(li);
    }
  }

  const createNoteInFlight = new Set();
  async function createNoteAndSelect(path) {
    // On wasm, github-save.js installs an editor that goes through the
    // PAT / GitHub Contents API — route to it so new notes actually
    // persist. On native, this hook is absent and we write via /api/note.
    if (typeof window.flowstone?.editNew === 'function') {
      window.flowstone.editNew(path);
      return { ok: true };
    }
    if (createNoteInFlight.has(path)) return { ok: false, message: 'already creating' };
    createNoteInFlight.add(path);
    try {
      const r = await fetch('/api/note', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ path }),
      });
      const result = await r.json();
      if (!result.ok) return result;
      await Promise.all([loadGraph(), loadTags(), loadTagGraph()]);
      const newNode = graph.nodes.find(n => n.id === path);
      if (newNode) selectNode(newNode); else loadBody(path);
      return result;
    } catch (e) {
      console.error('[create-note]', e);
      return { ok: false, message: e.message || 'error' };
    } finally {
      createNoteInFlight.delete(path);
    }
  }

  function setActiveTag(target) {
    activeTag = target;
    // Clear search input so the two filters don't overlap confusingly.
    if (target) {
      const input = document.getElementById('search');
      if (input.value) {
        input.value = '';
        searchSeq++;
      }
    }
    // Update active class on both lists.
    document.querySelectorAll('#tag-list li, #orphan-list li').forEach(li => {
      const name = li.querySelector('.tag-name')?.textContent;
      li.classList.toggle('active', name === target);
    });
    // And on the cloud.
    document.querySelectorAll('.cloud-tag').forEach(el => {
      el.classList.toggle('active', el.dataset.tag === target);
    });
    // Apply the filter to the graph.
    if (!target) {
      clearDimming();
      return;
    }
    const matches = new Set();
    for (const l of graph.links) {
      if (linkEnd(l.target) === target) {
        matches.add(linkEnd(l.source));
      }
    }
    // Also include the tag itself as a node if it resolves to a real note.
    matches.add(target);
    applyDimming(matches);
  }

  function applyDimming(matchSet) {
    svg.selectAll('.node circle').attr('opacity', d => matchSet.has(d.id) ? 1 : 0.1);
    svg.selectAll('.node text').attr('opacity', d => matchSet.has(d.id) ? 1 : 0.1);
  }

  function clearDimming() {
    svg.selectAll('.node circle').attr('opacity', 1);
    svg.selectAll('.node text').attr('opacity', 1);
  }

  function linkifyWikiLinks(root) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    const nodes = [];
    let n;
    while ((n = walker.nextNode())) nodes.push(n);

    const known = new Set(graph.nodes.map(x => x.id));

    for (const textNode of nodes) {
      let p = textNode.parentElement;
      let inCode = false;
      while (p && p !== root) {
        if (p.tagName === 'CODE' || p.tagName === 'PRE') { inCode = true; break; }
        p = p.parentElement;
      }
      if (inCode) continue;

      const text = textNode.nodeValue;
      const re = /\[\[([^\]]+)\]\]/g;
      if (!re.test(text)) continue;
      re.lastIndex = 0;

      const frag = document.createDocumentFragment();
      let lastIdx = 0;
      let m;
      while ((m = re.exec(text)) !== null) {
        if (m.index > lastIdx) {
          frag.appendChild(document.createTextNode(text.slice(lastIdx, m.index)));
        }
        const target = m[1].trim();
        const a = document.createElement('a');
        a.href = '#';
        a.dataset.noteLink = target;
        a.textContent = target;
        if (!known.has(target)) a.classList.add('unresolved');
        frag.appendChild(a);
        lastIdx = re.lastIndex;
      }
      if (lastIdx < text.length) {
        frag.appendChild(document.createTextNode(text.slice(lastIdx)));
      }
      textNode.parentNode.replaceChild(frag, textNode);
    }
  }

  async function loadBody(notePath) {
    const mySeq = ++bodySeq;
    const el = document.getElementById('detail-body');
    el.innerHTML = '<em class="empty">Loading…</em>';
    try {
      const res = await fetch('/api/note?path=' + encodeURIComponent(notePath));
      if (mySeq !== bodySeq) return;
      const data = await res.json();
      if (!data.ok) {
        el.innerHTML = '';
        const msg = document.createElement('em');
        msg.className = 'empty';
        msg.textContent = '(not found)';
        el.appendChild(msg);
        const btn = document.createElement('button');
        btn.className = 'new-note-btn';
        btn.textContent = 'New';
        btn.addEventListener('click', async () => {
          btn.disabled = true;
          btn.textContent = 'Creating…';
          const result = await createNoteAndSelect(notePath);
          if (!result.ok) {
            msg.textContent = result.message || 'error';
            btn.remove();
          }
        });
        el.appendChild(btn);
        return;
      }
      const body = data.body || '';
      if (typeof marked !== 'undefined') {
        el.innerHTML = marked.parse(body);
        linkifyWikiLinks(el);
      } else {
        el.textContent = body;
      }
    } catch (e) {
      console.error('[note]', e);
      el.innerHTML = '<em class="empty">(failed to load)</em>';
    }
  }

  async function loadMissingTags(notePath) {
    const mySeq = ++missingTagSeq;
    const ul = document.getElementById('detail-missing-tags');
    ul.innerHTML = '<li class="empty">Scanning…</li>';
    try {
      const res = await fetch('/api/missing-tags?note=' + encodeURIComponent(notePath));
      if (mySeq !== missingTagSeq) return; // a newer selection won
      const data = await res.json();
      renderMissingTags(data.hits || []);
    } catch (e) {
      console.error('[missing-tags]', e);
      ul.innerHTML = '<li class="empty">(failed to load)</li>';
    }
  }

  function renderMissingTags(hits) {
    const ul = document.getElementById('detail-missing-tags');
    ul.innerHTML = '';
    if (hits.length === 0) {
      const li = document.createElement('li');
      li.textContent = '(none)';
      li.className = 'empty';
      ul.appendChild(li);
      return;
    }
    for (const h of hits) {
      const li = document.createElement('li');
      const name = document.createElement('span');
      name.className = 'missing-tag-name';
      name.textContent = '[[' + h.missing_tag + ']]';
      const snippet = document.createElement('span');
      snippet.className = 'missing-tag-snippet';
      snippet.textContent = h.snippet;
      li.appendChild(name);
      li.appendChild(snippet);
      ul.appendChild(li);
    }
  }

  function updateStats() {
    document.getElementById('stats').textContent =
      `${graph.nodes.length} notes · ${graph.links.length} links`;
  }

  function render() {
    svg.selectAll('*').remove();

    const main = document.querySelector('main');
    const w = main.clientWidth;
    const h = main.clientHeight;
    svg.attr('viewBox', `0 0 ${w} ${h}`);

    const container = svg.append('g');

    svg.call(
      d3.zoom()
        .scaleExtent([0.25, 4])
        .on('zoom', (evt) => container.attr('transform', evt.transform))
    );

    const linkSel = container.append('g')
      .attr('class', 'links')
      .selectAll('line')
      .data(graph.links)
      .join('line');

    const nodeSel = container.append('g')
      .attr('class', 'nodes')
      .selectAll('g')
      .data(graph.nodes, d => d.id)
      .join('g')
      .attr('class', 'node')
      .call(drag());

    const radius = d => 4 + Math.sqrt(d.in_degree + 1) * 2;

    // in-degree colour ramp: the hub threshold is 4 (see buildGraph), but
    // notes with 2 or 3 inbound links are still meaningful shared targets.
    // Interpolate between the hub accent and the dim grey rather than have
    // everything below 4 look identical.
    const nodeFill = inDeg => {
      if (inDeg >= 4) return '#e94560'; // hub accent
      if (inDeg === 3) return '#c46078';
      if (inDeg === 2) return '#a88296';
      return '#7f8fa6';                 // 0–1: text-dim
    };

    nodeSel.append('circle')
      .attr('r', radius)
      .attr('fill', d => nodeFill(d.in_degree))
      .attr('stroke', '#0f3460')
      .attr('stroke-width', 1);

    nodeSel.append('text')
      .attr('dx', d => radius(d) + 3)
      .attr('dy', 4)
      .text(d => d.id)
      .attr('fill', '#e0e0e0')
      .attr('font-size', 11)
      .attr('pointer-events', 'none');

    nodeSel.on('click', (evt, d) => {
      evt.stopPropagation();
      selectNode(d);
    });

    svg.on('click', () => selectNode(null));

    simulation = d3.forceSimulation(graph.nodes)
      .force('link', d3.forceLink(graph.links).id(d => d.id).distance(60).strength(0.3))
      .force('charge', d3.forceManyBody().strength(-200))
      .force('center', d3.forceCenter(w / 2, h / 2))
      .force('collide', d3.forceCollide().radius(d => radius(d) + 8));

    simulation.on('tick', () => {
      linkSel
        .attr('x1', d => d.source.x)
        .attr('y1', d => d.source.y)
        .attr('x2', d => d.target.x)
        .attr('y2', d => d.target.y);
      nodeSel.attr('transform', d => `translate(${d.x},${d.y})`);
    });
  }

  function drag() {
    return d3.drag()
      .on('start', (evt) => {
        if (!evt.active) simulation.alphaTarget(0.3).restart();
        evt.subject.fx = evt.subject.x;
        evt.subject.fy = evt.subject.y;
      })
      .on('drag', (evt) => {
        evt.subject.fx = evt.x;
        evt.subject.fy = evt.y;
      })
      .on('end', (evt) => {
        if (!evt.active) simulation.alphaTarget(0);
        evt.subject.fx = null;
        evt.subject.fy = null;
      });
  }

  function linkEnd(v) {
    return (v && v.id) ? v.id : v;
  }

  function showTagDetails(tagName) {
    const toggle = activeTag === tagName;
    setActiveTag(toggle ? null : tagName);
    const aside = document.getElementById('details');
    if (toggle) { aside.hidden = true; return; }

    aside.hidden = false;
    document.getElementById('tag-notes-section').hidden = false;
    document.getElementById('note-sections').hidden = true;
    document.getElementById('detail-title').textContent = '#' + tagName;

    const notes = [];
    for (const l of graph.links) {
      if (linkEnd(l.target) === tagName) notes.push(linkEnd(l.source));
    }
    notes.sort();
    document.getElementById('detail-meta').textContent =
      `${notes.length} note${notes.length !== 1 ? 's' : ''}`;

    const lookup = new Map(graph.nodes.map(n => [n.id, n]));
    const ul = document.getElementById('tag-notes-list');
    ul.innerHTML = '';
    if (notes.length === 0) {
      const li = document.createElement('li');
      li.textContent = '(none)'; li.className = 'empty';
      ul.appendChild(li);
    } else {
      for (const noteId of notes) {
        const li = document.createElement('li');
        const a = document.createElement('a');
        a.href = '#';
        a.textContent = noteId;
        a.onclick = (e) => {
          e.preventDefault();
          const node = lookup.get(noteId);
          if (node) { setView('net'); selectNode(node); }
        };
        li.appendChild(a);
        ul.appendChild(li);
      }
    }
    bodySeq++;
    missingTagSeq++;
  }

  function selectNode(node) {
    selected = node;
    const aside = document.getElementById('details');

    if (!node) {
      aside.hidden = true;
      svg.selectAll('.node circle').attr('opacity', 1);
      svg.selectAll('.node text').attr('opacity', 1);
      svg.selectAll('.links line').attr('stroke-opacity', null);
      return;
    }

    aside.hidden = false;
    document.getElementById('tag-notes-section').hidden = true;
    document.getElementById('note-sections').hidden = false;
    document.getElementById('detail-title').textContent = node.title || node.id;
    document.getElementById('detail-meta').innerHTML =
      `<code>${node.id}</code> · in ${node.in_degree} · out ${node.out_degree}` +
      (node.is_hub ? ' · <span class="hub-tag">hub</span>' : '');

    const backs = graph.links
      .filter(l => linkEnd(l.target) === node.id)
      .map(l => linkEnd(l.source))
      .sort();
    const fwds = graph.links
      .filter(l => linkEnd(l.source) === node.id)
      .map(l => linkEnd(l.target))
      .sort();

    const lookup = new Map(graph.nodes.map(n => [n.id, n]));

    const renderList = (listId, items) => {
      const ul = document.getElementById(listId);
      ul.innerHTML = '';
      if (items.length === 0) {
        const li = document.createElement('li');
        li.textContent = '(none)';
        li.className = 'empty';
        ul.appendChild(li);
        return;
      }
      for (const item of items) {
        const li = document.createElement('li');
        const a = document.createElement('a');
        a.href = '#';
        a.textContent = item;
        a.onclick = (e) => {
          e.preventDefault();
          const target = lookup.get(item);
          if (target) selectNode(target);
        };
        li.appendChild(a);
        ul.appendChild(li);
      }
    };
    renderList('detail-backlinks', backs);
    renderList('detail-forward', fwds);
    loadBody(node.id);
    loadMissingTags(node.id);

    const neighbors = new Set([node.id, ...backs, ...fwds]);
    svg.selectAll('.node circle').attr('opacity', d => neighbors.has(d.id) ? 1 : 0.15);
    svg.selectAll('.node text').attr('opacity', d => neighbors.has(d.id) ? 1 : 0.15);
    svg.selectAll('.links line').attr('stroke-opacity', l => {
      const s = linkEnd(l.source);
      const t = linkEnd(l.target);
      return (s === node.id || t === node.id) ? 0.9 : 0.04;
    });
  }

  let searchTimer = null;
  let searchSeq = 0;

  function renderSearchDropdown(hits) {
    clearSearchDropdown();
    if (!hits.length) return;
    const input = document.getElementById('search');
    const rect  = input.getBoundingClientRect();
    const drop  = document.createElement('div');
    drop.id = 'search-dropdown';
    drop.style.cssText = `top:${rect.bottom + 2}px;left:${rect.left}px;width:${Math.max(rect.width, 260)}px;`;
    for (const h of hits.slice(0, 12)) {
      const item = document.createElement('div');
      item.className = 'search-result-item';
      item.textContent = h.title || h.path;
      item.addEventListener('mousedown', e => {
        e.preventDefault();
        window.flowstone?.loadBody(h.path);
        clearSearchDropdown();
        document.getElementById('search').value = '';
        clearDimming();
      });
      drop.appendChild(item);
    }
    document.body.appendChild(drop);
  }

  function clearSearchDropdown() {
    document.getElementById('search-dropdown')?.remove();
  }

  async function runSearch(q) {
    const mySeq = ++searchSeq;
    try {
      const res = await fetch('/api/search?q=' + encodeURIComponent(q));
      if (mySeq !== searchSeq) return; // a newer keystroke won
      const data = await res.json();
      const hits = data.hits || [];
      applyDimming(new Set(hits.map(h => h.path)));
      renderSearchDropdown(hits);
    } catch (e) {
      console.error('[search]', e);
    }
  }

  const searchInput = document.getElementById('search');
  searchInput.addEventListener('input', (evt) => {
    const q = evt.target.value.trim();
    if (searchTimer) clearTimeout(searchTimer);
    // Search and tag filters are mutually exclusive — clear the active tag
    // as soon as the user starts typing.
    if (activeTag) setActiveTag(null);
    if (!q) {
      searchSeq++;
      clearDimming();
      clearSearchDropdown();
      return;
    }
    searchTimer = setTimeout(() => runSearch(q), 150);
  });
  searchInput.addEventListener('blur', () => setTimeout(clearSearchDropdown, 150));

  document.getElementById('close-details').addEventListener('click', () => selectNode(null));

  document.getElementById('detail-body').addEventListener('click', (e) => {
    const a = e.target.closest('a[data-note-link]');
    if (!a) return;
    e.preventDefault();
    const target = a.dataset.noteLink;
    const node = graph.nodes.find(x => x.id === target);
    if (node) selectNode(node);
  });

  const chip = document.getElementById('reload-chip');
  const es = new EventSource('/api/events');
  es.addEventListener('update-available', () => {
    chip.hidden = false;
  });
  chip.addEventListener('click', async () => {
    chip.hidden = true;
    const prevId = selected ? selected.id : null;
    await Promise.all([loadGraph(), loadTags(), loadTagGraph()]);
    if (prevId) {
      const still = graph.nodes.find(n => n.id === prevId);
      if (still) selectNode(still);
    }
    if (activeTag) setActiveTag(activeTag); // re-apply filter after reload
  });

  const CLOUD_PALETTE = [
    '#f4a8c7', // rose
    '#88c5e8', // sky
    '#8dd5a8', // mint
    '#f0d97b', // straw
    '#c9a8f4', // lavender
    '#f4b88a', // peach
    '#7ecec4', // teal
    '#f29090', // coral
    '#b0d97b', // lime
    '#a8c8f4', // periwinkle
  ];

  function tagColor(name) {
    let h = 0;
    for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0;
    return CLOUD_PALETTE[h % CLOUD_PALETTE.length];
  }

  function renderCloud() {
    const container = document.getElementById('cloud');
    container.innerHTML = '';
    if (!allTags.length) return;

    const rect = container.getBoundingClientRect();
    const W = rect.width;
    const H = rect.height;
    if (W === 0 || H === 0) return;

    const sorted = [...allTags].sort((a, b) => b.count - a.count);
    const maxCount = sorted[0].count;
    const minCount = sorted[sorted.length - 1].count;
    const minSize = 11;
    const maxSize = Math.min(72, Math.floor(Math.min(W, H) / 8));
    const scale = (c) => {
      if (maxCount === minCount) return (minSize + maxSize) / 2;
      const t = (Math.sqrt(c) - Math.sqrt(minCount)) /
                (Math.sqrt(maxCount) - Math.sqrt(minCount));
      return minSize + t * (maxSize - minSize);
    };

    const placed = [];
    const pad = 4;
    const overlaps = (x, y, w, h) => placed.some(p =>
      !(x + w + pad < p.x || p.x + p.w + pad < x ||
        y + h + pad < p.y || p.y + p.h + pad < y));

    for (const t of sorted) {
      const el = document.createElement('span');
      el.className = 'cloud-tag';
      if (!t.resolved) el.classList.add('unresolved');
      if (activeTag === t.target) el.classList.add('active');
      el.dataset.tag = t.target;
      el.style.setProperty('--tag-color', tagColor(t.target));
      el.textContent = t.target;
      el.style.fontSize = scale(t.count) + 'px';
      el.title = `${t.target} · ${t.count}`;
      el.addEventListener('click', () => showTagDetails(t.target));
      container.appendChild(el);

      const b = el.getBoundingClientRect();
      const tw = b.width;
      const th = b.height;

      let x = W / 2 - tw / 2;
      let y = H / 2 - th / 2;
      let found = !overlaps(x, y, tw, th);
      if (!found) {
        const step = 4;
        const maxR = Math.hypot(W, H);
        outer: for (let r = step; r < maxR; r += step) {
          const angles = Math.max(12, Math.floor(r / 3));
          for (let i = 0; i < angles; i++) {
            const a = (i / angles) * Math.PI * 2;
            const cx = W / 2 + r * Math.cos(a) - tw / 2;
            const cy = H / 2 + r * Math.sin(a) * 0.65 - th / 2;
            if (cx < 2 || cy < 2 || cx + tw > W - 2 || cy + th > H - 2) continue;
            if (!overlaps(cx, cy, tw, th)) {
              x = cx; y = cy; found = true;
              break outer;
            }
          }
        }
      }
      if (!found) { el.remove(); continue; }
      el.style.left = x + 'px';
      el.style.top = y + 'px';
      placed.push({ x, y, w: tw, h: th });
    }
  }

  function renderTagNet() {
    if (!tagNetDirty) return;
    tagNetDirty = false;

    const svgEl = document.getElementById('tag-net');
    const tn = d3.select('#tag-net');
    tn.selectAll('*').remove();

    const main = document.querySelector('main');
    const w = main.clientWidth;
    const h = main.clientHeight;
    tn.attr('viewBox', `0 0 ${w} ${h}`);

    if (!tagGraph.nodes.length) {
      tn.append('text')
        .attr('x', w / 2).attr('y', h / 2)
        .attr('text-anchor', 'middle')
        .attr('fill', '#7f8fa6').attr('font-size', 13)
        .text('No tag co-occurrences found (need tags used in ≥ 2 notes)');
      return;
    }

    if (tagSimulation) tagSimulation.stop();

    const container = tn.append('g');
    tn.call(
      d3.zoom().scaleExtent([0.2, 4])
        .on('zoom', evt => container.attr('transform', evt.transform))
    );

    const maxCount  = d3.max(tagGraph.nodes, d => d.count) || 1;
    const maxWeight = d3.max(tagGraph.links, d => d.weight) || 1;
    const radius    = d => 4 + Math.sqrt(d.count / maxCount) * 14;

    const linkSel = container.append('g').attr('class', 'links')
      .selectAll('line').data(tagGraph.links).join('line')
        .attr('stroke', '#0f3460')
        .attr('stroke-width', d => 1 + (d.weight / maxWeight) * 4)
        .attr('stroke-opacity', d => 0.3 + (d.weight / maxWeight) * 0.5);

    const nodeSel = container.append('g').attr('class', 'nodes')
      .selectAll('g').data(tagGraph.nodes, d => d.id).join('g')
        .attr('class', 'node')
        .call(d3.drag()
          .on('start', evt => {
            if (!evt.active) tagSimulation.alphaTarget(0.3).restart();
            evt.subject.fx = evt.subject.x;
            evt.subject.fy = evt.subject.y;
          })
          .on('drag', evt => { evt.subject.fx = evt.x; evt.subject.fy = evt.y; })
          .on('end',  evt => {
            if (!evt.active) tagSimulation.alphaTarget(0);
            evt.subject.fx = null; evt.subject.fy = null;
          })
        );

    nodeSel.append('circle')
      .attr('r', radius)
      .attr('fill', d => tagColor(d.id))
      .attr('fill-opacity', d => d.resolved ? 1 : 0.6)
      .attr('stroke', '#0f3460')
      .attr('stroke-width', 1);

    nodeSel.append('text')
      .attr('dx', d => radius(d) + 3).attr('dy', 4)
      .text(d => d.id)
      .attr('fill', '#e0e0e0').attr('font-size', 11)
      .attr('pointer-events', 'none');

    nodeSel.on('click', (evt, d) => {
      evt.stopPropagation();
      showTagDetails(d.id);
    });
    tn.on('click', () => {
      document.getElementById('details').hidden = true;
    });

    tagSimulation = d3.forceSimulation(tagGraph.nodes)
      .force('link',    d3.forceLink(tagGraph.links).id(d => d.id)
                          .distance(d => 80 - (d.weight / maxWeight) * 40)
                          .strength(0.4))
      .force('charge',  d3.forceManyBody().strength(-150))
      .force('center',  d3.forceCenter(w / 2, h / 2))
      .force('collide', d3.forceCollide().radius(d => radius(d) + 6));

    tagSimulation.on('tick', () => {
      linkSel
        .attr('x1', d => d.source.x).attr('y1', d => d.source.y)
        .attr('x2', d => d.target.x).attr('y2', d => d.target.y);
      nodeSel.attr('transform', d => `translate(${d.x},${d.y})`);
    });
  }

  function setView(view) {
    if (view === currentView) return;
    currentView = view;
    document.querySelectorAll('.view-tab').forEach(b => {
      b.classList.toggle('active', b.dataset.view === view);
    });
    document.getElementById('graph').style.display    = view === 'net'     ? 'block' : 'none';
    document.getElementById('cloud').style.display   = view === 'cloud'   ? 'block' : 'none';
    document.getElementById('tag-net').style.display = view === 'tagnet'  ? 'block' : 'none';
    document.getElementById('tags').style.display    = view === 'net'     ? 'block' : 'none';
    if (view === 'cloud')  renderCloud();
    if (view === 'tagnet') renderTagNet();
  }

  document.querySelectorAll('.view-tab').forEach(b => {
    b.addEventListener('click', () => setView(b.dataset.view));
  });

  window.addEventListener('resize', () => {
    if (currentView === 'cloud')  renderCloud();
    if (currentView === 'tagnet') { tagNetDirty = true; renderTagNet(); }
  });

  window.flowstone = { loadBody, showTagDetails };

  loadGraph();
  loadTags();
  loadTagGraph();
})();
