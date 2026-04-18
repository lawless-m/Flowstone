// yaml-graph.js — renders the YAML directed-graph tab.
//
// Reads schema metadata via fs.schemas_json() (colour/shape hints) and
// node/edge rows via fs.run() over the yaml_nodes relation plus the
// per-(schema, edge-kind) relations produced by flowstone-core. Force-
// directed layout, arrowhead markers on directed edges, shape per node
// kind. Clicking a node fires window.flowstone.loadYamlNode(path) so
// graph.js / github-save.js can own the detail pane.

(function () {
  const svgSel = () => d3.select('#yaml-graph');

  // "infra.schema" → "infra"; non-alphanumerics → '_'. Mirrors the
  // schema_prefix() helper in flowstone-core/src/yaml_db.rs.
  function schemaPrefix(name) {
    const stem = name.endsWith('.schema') ? name.slice(0, -'.schema'.length) : name;
    return stem.replace(/[^a-zA-Z0-9]/g, '_');
  }

  function runQuery(script) {
    const raw = window.fs.run(script, '', true);
    try { return JSON.parse(raw); } catch { return { rows: [] }; }
  }

  function loadYamlGraph() {
    if (!window.fs) return { nodes: [], links: [], schemas: {} };

    let schemas = {};
    try { schemas = JSON.parse(window.fs.schemas_json() || '{}'); } catch {}

    // Nodes
    const nodeRows = runQuery(
      '?[path, schema, kind, attrs_json] := *yaml_nodes{path, schema, kind, attrs_json}'
    ).rows || [];
    const nodeMap = new Map();
    for (const [path, schema, kind, attrs_json] of nodeRows) {
      nodeMap.set(path, { id: path, schema, kind, attrs_json, phantom: false });
    }

    // Edges — one query per (schema, edge-kind) pair, tagged with kind + colour.
    const links = [];
    for (const [schemaName, schemaDef] of Object.entries(schemas)) {
      const prefix = schemaPrefix(schemaName);
      for (const [kind, spec] of Object.entries(schemaDef.edges || {})) {
        const rel = `${prefix}_${kind}`;
        const res = runQuery(`?[s, t] := *${rel}[s, t]`);
        for (const [s, t] of (res.rows || [])) {
          links.push({
            source: s,
            target: t,
            kind,
            schema: schemaName,
            colour: spec.colour || '#7f8fa6',
            directed: spec.directed !== false,
          });
          // Target may be a yaml file we never saw — give it a placeholder
          // node so the graph stays connected, marked phantom for render.
          if (!nodeMap.has(t)) {
            nodeMap.set(t, { id: t, schema: schemaName, kind: '?', attrs_json: '{}', phantom: true });
          }
        }
      }
    }

    const nodes = [...nodeMap.values()];
    for (const n of nodes) {
      const spec = schemas[n.schema]?.nodes?.[n.kind];
      n.shape = spec?.shape || 'circle';
    }

    return { nodes, links, schemas };
  }

  // Draw one of the named shapes centred at (0,0) into `g`, radius r.
  function drawShape(g, shape, r) {
    switch (shape) {
      case 'diamond':
        g.append('polygon')
          .attr('points', `0,${-r} ${r},0 0,${r} ${-r},0`);
        break;
      case 'hexagon': {
        const pts = [];
        for (let i = 0; i < 6; i++) {
          const a = (Math.PI / 3) * i;
          pts.push(`${r * Math.cos(a)},${r * Math.sin(a)}`);
        }
        g.append('polygon').attr('points', pts.join(' '));
        break;
      }
      case 'box':
        g.append('rect')
          .attr('x', -r).attr('y', -r * 0.7)
          .attr('width', r * 2).attr('height', r * 1.4);
        break;
      case 'cylinder': {
        const h = r * 1.4;
        g.append('rect')
          .attr('x', -r).attr('y', -h / 2 + r * 0.25)
          .attr('width', r * 2).attr('height', h - r * 0.5);
        g.append('ellipse')
          .attr('rx', r).attr('ry', r * 0.25).attr('cy', -h / 2 + r * 0.25);
        g.append('ellipse')
          .attr('rx', r).attr('ry', r * 0.25).attr('cy', h / 2 - r * 0.25);
        break;
      }
      case 'ellipse':
        g.append('ellipse').attr('rx', r * 1.2).attr('ry', r * 0.7);
        break;
      default:
        g.append('circle').attr('r', r);
    }
  }

  function render() {
    const svg = svgSel();
    svg.selectAll('*').remove();

    const { nodes, links, schemas } = loadYamlGraph();
    if (nodes.length === 0) {
      svg.append('text')
        .attr('x', 30).attr('y', 40)
        .attr('fill', '#888').attr('font-family', 'monospace')
        .text('No YAML nodes in this corpus.');
      return;
    }

    const w = window.innerWidth;
    const h = window.innerHeight - 90;
    svg.attr('width', w).attr('height', h);

    // One arrowhead marker per distinct edge colour so arrows match line colour.
    const defs = svg.append('defs');
    const colours = [...new Set(links.map(l => l.colour))];
    for (const c of colours) {
      const id = 'arrow-' + c.replace(/[^a-zA-Z0-9]/g, '');
      defs.append('marker')
        .attr('id', id)
        .attr('viewBox', '0 -5 10 10')
        .attr('refX', 18)
        .attr('refY', 0)
        .attr('markerWidth', 6)
        .attr('markerHeight', 6)
        .attr('orient', 'auto')
        .append('path')
        .attr('d', 'M0,-5L10,0L0,5')
        .attr('fill', c);
    }
    const markerFor = (c) => 'url(#arrow-' + c.replace(/[^a-zA-Z0-9]/g, '') + ')';

    const container = svg.append('g');
    svg.call(d3.zoom().on('zoom', (e) => container.attr('transform', e.transform)));

    const link = container.append('g')
      .attr('stroke-opacity', 0.75)
      .selectAll('line')
      .data(links)
      .join('line')
      .attr('stroke', d => d.colour)
      .attr('stroke-width', 1.5)
      .attr('marker-end', d => d.directed ? markerFor(d.colour) : null);

    const node = container.append('g')
      .selectAll('g')
      .data(nodes)
      .join('g')
      .attr('class', d => 'yaml-node' + (d.phantom ? ' phantom' : ''))
      .style('cursor', 'pointer')
      .call(d3.drag()
        .on('start', (e, d) => { if (!e.active) sim.alphaTarget(0.3).restart(); d.fx = d.x; d.fy = d.y; })
        .on('drag',  (e, d) => { d.fx = e.x; d.fy = e.y; })
        .on('end',   (e, d) => { if (!e.active) sim.alphaTarget(0); d.fx = null; d.fy = null; }))
      .on('click', (_, d) => window.flowstone?.loadYamlNode?.(d));

    const r = 10;
    node.each(function (d) {
      const g = d3.select(this);
      drawShape(g, d.shape, r);
      g.select('*')
        .attr('fill',   d.phantom ? '#333'    : '#1a3a5c')
        .attr('stroke', d.phantom ? '#555'    : '#4a9eff')
        .attr('stroke-width', 1.5)
        .attr('stroke-dasharray', d.phantom ? '3,2' : null);
    });

    node.append('text')
      .attr('dy', r + 12)
      .attr('text-anchor', 'middle')
      .attr('font-family', 'ui-monospace, monospace')
      .attr('font-size', 10)
      .attr('fill', '#c0c0c0')
      .text(d => d.id);

    const sim = d3.forceSimulation(nodes)
      .force('link', d3.forceLink(links).id(d => d.id).distance(70))
      .force('charge', d3.forceManyBody().strength(-200))
      .force('center', d3.forceCenter(w / 2, h / 2))
      .force('collide', d3.forceCollide().radius(r + 8))
      .on('tick', () => {
        link
          .attr('x1', d => d.source.x).attr('y1', d => d.source.y)
          .attr('x2', d => d.target.x).attr('y2', d => d.target.y);
        node.attr('transform', d => `translate(${d.x},${d.y})`);
      });
  }

  function loadYamlNode(d) {
    const details = document.getElementById('details');
    details.hidden = false;
    document.getElementById('detail-title').textContent = d.id;

    const meta = document.getElementById('detail-meta');
    meta.innerHTML = '';
    const ul = document.createElement('ul');
    ul.style.cssText =
      'list-style:none;padding:0;margin:0 0 12px;font-family:ui-monospace,monospace;font-size:12px;';

    const addLi = (label, value) => {
      const li = document.createElement('li');
      li.style.cssText = 'padding:2px 0;color:#c0c0c0;';
      const k = document.createElement('span');
      k.textContent = label + ': ';
      k.style.color = '#7f8fa6';
      li.appendChild(k);
      li.appendChild(document.createTextNode(value));
      ul.appendChild(li);
    };

    addLi('schema', d.schema || '');
    if (d.kind && d.kind !== '?') addLi('kind', d.kind);
    try {
      const attrs = JSON.parse(d.attrs_json || '{}');
      for (const [k, v] of Object.entries(attrs)) {
        addLi(k, typeof v === 'string' ? v : JSON.stringify(v));
      }
    } catch {}
    if (d.phantom) {
      const warn = document.createElement('li');
      warn.style.cssText =
        'padding:4px 8px;margin-top:6px;background:#3a2a14;color:#f4c68a;';
      warn.textContent = 'Referenced by edges but no YAML file found.';
      ul.appendChild(warn);
    }
    meta.appendChild(ul);

    // Try to load a sibling markdown with the same id. loadBody already
    // handles "not found" gracefully (shows a "New" button for creation).
    window.flowstone?.loadBody?.(d.id);
  }

  window.flowstoneYaml = { render };
  window.flowstone = Object.assign(window.flowstone || {}, { loadYamlNode });
})();
