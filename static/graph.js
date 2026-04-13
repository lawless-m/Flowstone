(() => {
  const svg = d3.select('#graph');
  let graph = { nodes: [], links: [] };
  let simulation = null;
  let selected = null;
  let activeTag = null;
  let missingTagSeq = 0;
  let bodySeq = 0;

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
    const themes = tags.filter(t => t.count >= 2);
    const orphans = tags.filter(t => t.count < 2);
    renderTagList('tag-list', themes);
    renderTagList('orphan-list', orphans);
    const heading = document.getElementById('orphan-heading');
    const counter = document.getElementById('orphan-count');
    heading.hidden = orphans.length === 0;
    counter.textContent = orphans.length ? `(${orphans.length})` : '';
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
      ul.appendChild(li);
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
        el.innerHTML = '<em class="empty">(not found)</em>';
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

    nodeSel.append('circle')
      .attr('r', radius)
      .attr('fill', d => d.is_hub ? '#e94560' : '#7f8fa6')
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
  async function runSearch(q) {
    const mySeq = ++searchSeq;
    try {
      const res = await fetch('/api/search?q=' + encodeURIComponent(q));
      if (mySeq !== searchSeq) return; // a newer keystroke won
      const data = await res.json();
      const hits = new Set((data.hits || []).map(h => h.path));
      applyDimming(hits);
    } catch (e) {
      console.error('[search]', e);
    }
  }
  document.getElementById('search').addEventListener('input', (evt) => {
    const q = evt.target.value.trim();
    if (searchTimer) clearTimeout(searchTimer);
    // Search and tag filters are mutually exclusive — clear the active tag
    // as soon as the user starts typing.
    if (activeTag) setActiveTag(null);
    if (!q) {
      searchSeq++;
      clearDimming();
      return;
    }
    searchTimer = setTimeout(() => runSearch(q), 150);
  });

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
    await Promise.all([loadGraph(), loadTags()]);
    if (prevId) {
      const still = graph.nodes.find(n => n.id === prevId);
      if (still) selectNode(still);
    }
    if (activeTag) setActiveTag(activeTag); // re-apply filter after reload
  });

  loadGraph();
  loadTags();
})();
