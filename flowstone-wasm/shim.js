// Monkey-patches window.fetch and window.EventSource so the unchanged
// static/graph.js can run against an in-page Flowstone wasm instance
// instead of the Axum server's /api/* endpoints.
//
// The native server's endpoint shapes (see src/server.rs) are reproduced
// here in JavaScript, running Datalog queries against Flowstone.run().
// FTS-backed search falls back to a plain substring sweep because the
// wasm build has `fts` disabled (see flowstone-core's `fts` feature).

export function installShim(fs) {
  const originalFetch = window.fetch.bind(window);

  window.fetch = async function (input, init) {
    const url = typeof input === 'string' ? input : (input && input.url) || '';
    if (!url.startsWith('/api/')) {
      return originalFetch(input, init);
    }
    const u = new URL(url, location.href);
    try {
      switch (u.pathname) {
        case '/api/graph':        return jsonResponse(buildGraph(fs));
        case '/api/tags':         return jsonResponse(buildTags(fs));
        case '/api/note':         return jsonResponse(buildNote(fs, u.searchParams.get('path')));
        case '/api/missing-tags': return jsonResponse(buildMissingTags(fs, u.searchParams.get('note')));
        case '/api/search':       return jsonResponse(buildSearch(fs, u.searchParams.get('q')));
      }
    } catch (e) {
      console.error('[flowstone-shim]', u.pathname, e);
      return jsonResponse({ error: String(e && e.message || e) });
    }
    return new Response('Not Found', { status: 404 });
  };

  // Stub EventSource for /api/events so graph.js does not sit in a retry
  // loop against a missing SSE endpoint. The zip-driven build has nothing
  // to push updates for anyway.
  const OrigES = window.EventSource;
  window.EventSource = function (url) {
    if (typeof url === 'string' && url.startsWith('/api/events')) {
      return {
        addEventListener() {},
        removeEventListener() {},
        close() {},
        readyState: 1,
      };
    }
    return new OrigES(url);
  };
}

// ---- query helpers ----

function run(fs, script, params) {
  const out = fs.run(script, params || '', true);
  const parsed = JSON.parse(out);
  if (parsed.ok === false) {
    throw new Error(parsed.message || 'cozo query failed');
  }
  return parsed;
}

function jsonResponse(obj) {
  return new Response(JSON.stringify(obj), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

// ---- /api/graph ----

function buildGraph(fs) {
  const notesRes = run(fs, '?[path, title] := *notes{path, title}');
  const linksRes = run(fs, '?[source, target] := *links[source, target], *notes{path: target}');

  const nodeById = new Map();
  for (const row of notesRes.rows || []) {
    const [id, title] = row;
    if (!id) continue;
    nodeById.set(id, { id, title, in_degree: 0, out_degree: 0, is_hub: false });
  }

  const links = [];
  for (const row of linksRes.rows || []) {
    const [source, target] = row;
    if (!source || !target) continue;
    if (nodeById.has(source) && nodeById.has(target)) {
      links.push({ source, target });
      nodeById.get(source).out_degree++;
      nodeById.get(target).in_degree++;
    }
  }

  for (const n of nodeById.values()) {
    n.is_hub = n.in_degree >= 4;
  }

  const nodes = Array.from(nodeById.values()).sort((a, b) => a.id.localeCompare(b.id));
  return { nodes, links };
}

// ---- /api/tags ----

function buildTags(fs) {
  const notesRes = run(fs, '?[path] := *notes{path}');
  const noteSet = new Set((notesRes.rows || []).map(r => r[0]));

  const linksRes = run(
    fs,
    '?[target, count(source)] := *links[source, target] :order -count(source)'
  );
  const tags = [];
  for (const row of linksRes.rows || []) {
    const [target, count] = row;
    if (!target) continue;
    tags.push({ target, count, resolved: noteSet.has(target) });
  }
  return { tags };
}

// ---- /api/note ----

function buildNote(fs, path) {
  if (!path) return { ok: false, path: '', title: '', body: '' };
  const res = run(
    fs,
    '?[title, body] := *notes{path, title, body}, path = $p',
    JSON.stringify({ p: path })
  );
  const rows = res.rows || [];
  if (rows.length === 0) return { ok: false, path, title: '', body: '' };
  return { ok: true, path, title: rows[0][0] || '', body: rows[0][1] || '' };
}

// ---- /api/missing-tags ----
//
// Ported from src/server.rs build_missing_tags. Reads the vocabulary of
// tag targets from the links relation, strips existing wiki-links from
// each note body, then alternation-regexes the tag vocabulary across
// what's left, recording the first occurrence of each missing tag with
// a ±40-char snippet for context.

function buildMissingTags(fs, note) {
  const tagsRes = run(fs, '?[target] := *links[_, target]');
  const tagTargets = (tagsRes.rows || [])
    .map(r => r[0])
    .filter(s => typeof s === 'string' && s.length >= 3);
  if (tagTargets.length === 0) return { hits: [] };

  const script = note
    ? '?[path, body] := *notes{path, body}, path = $path'
    : '?[path, body] := *notes{path, body}';
  const params = note ? JSON.stringify({ path: note }) : '';
  const notesRes = run(fs, script, params);

  const wikiLinkRe = /\[\[[^\]]*\]\]/g;
  const hits = [];
  for (const row of notesRes.rows || []) {
    const [notePath, body] = row;
    if (!notePath || !body) continue;

    const stripped = body.replace(wikiLinkRe, m => ' '.repeat(m.length));

    const alternation = tagTargets
      .filter(t => t !== notePath)
      .map(escapeRegex);
    if (alternation.length === 0) continue;

    const pattern = new RegExp('\\b(' + alternation.join('|') + ')\\b', 'gi');
    const seen = new Set();
    let m;
    while ((m = pattern.exec(stripped)) !== null) {
      const matched = m[0].toLowerCase();
      if (seen.has(matched)) continue;
      seen.add(matched);
      hits.push({
        note_path: notePath,
        missing_tag: matched,
        snippet: snippetAround(stripped, m.index, m.index + m[0].length),
      });
    }
  }
  return { hits };
}

function escapeRegex(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function snippetAround(text, start, end) {
  const RADIUS = 40;
  const preStart = Math.max(0, start - RADIUS);
  const postEnd = Math.min(text.length, end + RADIUS);
  const prefix = preStart > 0 ? '…' : '';
  const suffix = postEnd < text.length ? '…' : '';
  const body = text.slice(preStart, postEnd).replace(/\n/g, ' ').replace(/\s+/g, ' ').trim();
  return prefix + body + suffix;
}

// ---- /api/search ----
//
// FTS is disabled in the browser wasm build (tantivy needs threads), so
// we fall back to a plain case-insensitive substring sweep over titles
// and bodies. Score is always 0 — purely ordering-neutral, the UI only
// uses it for the hit set.

function buildSearch(fs, q) {
  if (!q) return { hits: [] };
  const needle = q.trim().toLowerCase();
  if (!needle) return { hits: [] };
  const res = run(fs, '?[path, title, body] := *notes{path, title, body}');
  const hits = [];
  for (const row of res.rows || []) {
    const [path, title, body] = row;
    if (!path) continue;
    const t = (title || '').toLowerCase();
    const b = (body || '').toLowerCase();
    if (t.includes(needle) || b.includes(needle)) {
      hits.push({ path, title: title || '', score: 0 });
    }
  }
  return { hits };
}
