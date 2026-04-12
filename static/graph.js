(() => {
  const svg = d3.select('#graph');
  let graph = { nodes: [], links: [] };
  let simulation = null;
  let selected = null;

  async function loadGraph() {
    const res = await fetch('/api/graph');
    graph = await res.json();
    render();
    updateStats();
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
  function clearSearch() {
    svg.selectAll('.node circle').attr('opacity', 1);
    svg.selectAll('.node text').attr('opacity', 1);
  }
  async function runSearch(q) {
    const mySeq = ++searchSeq;
    try {
      const res = await fetch('/api/search?q=' + encodeURIComponent(q));
      if (mySeq !== searchSeq) return; // a newer keystroke won
      const data = await res.json();
      const hits = new Set((data.hits || []).map(h => h.path));
      svg.selectAll('.node').each(function(d) {
        const hit = hits.has(d.id);
        d3.select(this).select('circle').attr('opacity', hit ? 1 : 0.1);
        d3.select(this).select('text').attr('opacity', hit ? 1 : 0.1);
      });
    } catch (e) {
      console.error('[search]', e);
    }
  }
  document.getElementById('search').addEventListener('input', (evt) => {
    const q = evt.target.value.trim();
    if (searchTimer) clearTimeout(searchTimer);
    if (!q) {
      searchSeq++;
      clearSearch();
      return;
    }
    searchTimer = setTimeout(() => runSearch(q), 150);
  });

  document.getElementById('close-details').addEventListener('click', () => selectNode(null));

  const chip = document.getElementById('reload-chip');
  const es = new EventSource('/api/events');
  es.addEventListener('update-available', () => {
    chip.hidden = false;
  });
  chip.addEventListener('click', async () => {
    chip.hidden = true;
    const prevId = selected ? selected.id : null;
    await loadGraph();
    if (prevId) {
      const still = graph.nodes.find(n => n.id === prevId);
      if (still) selectNode(still);
    }
  });

  loadGraph();
})();
