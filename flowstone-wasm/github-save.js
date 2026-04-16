// github-save.js — wasm-only module: Edit / Save-to-GitHub for note detail panel.
(function () {
  const LS = {
    get: k => localStorage.getItem('flowstone_gh_' + k) || '',
    set: (k, v) => localStorage.setItem('flowstone_gh_' + k, v),
  };

  // ---- multi-repo storage ----
  function getRepos() {
    try { return JSON.parse(localStorage.getItem('flowstone_repos') || '[]'); }
    catch { return []; }
  }
  function saveRepos(repos) { localStorage.setItem('flowstone_repos', JSON.stringify(repos)); }
  function getActiveId()    { return localStorage.getItem('flowstone_active_repo') || ''; }
  function setActiveId(id)  { localStorage.setItem('flowstone_active_repo', id); }

  function applyRepo(r) {
    LS.set('repo',     r.repo     || '');
    LS.set('token',    r.token    || '');
    LS.set('branch',   r.branch   || 'main');
    LS.set('template', r.template || '');
    if (r.zipUrl) localStorage.setItem('flowstone_zip_url', r.zipUrl);
    setActiveId(r.id);
  }

  // Migrate pre-multi-repo localStorage into the repos array on first use.
  function migrateIfNeeded() {
    if (getRepos().length > 0) return;
    const repo   = LS.get('repo');
    const zipUrl = localStorage.getItem('flowstone_zip_url') || '';
    if (!repo && !zipUrl) return;
    const id = crypto.randomUUID();
    saveRepos([{
      id,
      name:     repo || zipUrl.split('/').pop()?.replace('.zip', '') || 'Default',
      zipUrl,
      repo,
      branch:   LS.get('branch') || 'main',
      token:    LS.get('token'),
      template: LS.get('template'),
    }]);
    setActiveId(id);
  }

  // ---- draft auto-save ----
  const draftKey  = path => 'flowstone_draft_' + (path || '__new__');
  const saveDraft  = (path, content) =>
    localStorage.setItem(draftKey(path), JSON.stringify({ content, savedAt: Date.now() }));
  const loadDraft  = path => { try { return JSON.parse(localStorage.getItem(draftKey(path))); } catch { return null; } };
  const clearDraft = path => localStorage.removeItem(draftKey(path));

  // ---- stylesheet ----
  const style = document.createElement('style');
  style.textContent = `
    #gh-settings-btn {
      background: transparent; border: none; color: #7f8fa6;
      font-size: 18px; cursor: pointer; padding: 4px 8px; line-height: 1;
    }
    #gh-settings-btn:hover { color: #e94560; }
    #gh-new-btn { margin-left: auto; }
    .gh-btn {
      background: transparent; color: #a0b0c0;
      border: 1px solid #0f3460; padding: 4px 10px;
      font-size: 11px; cursor: pointer; border-radius: 3px; font-family: inherit;
    }
    .gh-btn:hover { border-color: #e94560; color: #e94560; }
    .gh-primary { background: #e94560; color: #fff; border-color: #e94560; }
    .gh-primary:hover { background: #ff5a7a; border-color: #ff5a7a; color: #fff; }
    #gh-edit-controls { display: flex; gap: 8px; margin-bottom: 10px; flex-wrap: wrap; align-items: center; }
    #gh-panel-status { font-size: 11px; padding: 4px 8px; border-radius: 3px; margin-bottom: 8px; width: 100%; }
    .gh-danger { border-color: #6a1414; color: #f4a0a0; }
    .gh-danger:hover { border-color: #e94560; color: #e94560; }
    #gh-settings-overlay {
      position: fixed; inset: 0; background: rgba(0,0,0,0.75);
      z-index: 2000; display: flex; align-items: center; justify-content: center;
    }
    #gh-settings-panel {
      background: #1a1a2e; border: 1px solid #0f3460;
      padding: 24px; width: min(480px, 90vw);
      max-height: 90vh; overflow-y: auto;
    }
    #gh-repo-label {
      font-size: 11px; color: #7f8fa6; cursor: pointer;
      max-width: 140px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    }
    #gh-repo-label:hover { color: #e94560; }
    #gh-settings-panel h3 { margin: 0 0 14px; color: #e94560; font-size: 14px; }
    .gh-label {
      display: block; font-size: 11px; color: #7f8fa6;
      margin: 10px 0 3px; text-transform: uppercase; letter-spacing: 0.04em;
    }
    .gh-input {
      width: 100%; box-sizing: border-box; background: #0f0f1a; color: #e0e0e0;
      border: 1px solid #0f3460; padding: 6px 8px;
      font-family: ui-monospace, monospace; font-size: 13px;
    }
    .gh-input:focus { outline: none; border-color: #e94560; }
    .gh-settings-note { font-size: 11px; color: #555; margin: 12px 0 0; line-height: 1.5; }
    .gh-settings-btns { display: flex; gap: 8px; margin-top: 16px; justify-content: flex-end; }

    /* ---- split editor modal ---- */
    #gh-editor-overlay {
      position: fixed; inset: 0; z-index: 1500;
      background: #1a1a2e; display: flex; flex-direction: column;
    }
    #gh-editor-bar {
      display: flex; align-items: center; gap: 8px;
      padding: 8px 16px; background: #16213e;
      border-bottom: 1px solid #0f3460; flex-shrink: 0;
    }
    #gh-editor-title {
      font-size: 12px; color: #7f8fa6; flex: 1;
      overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    }
    #gh-modal-status { font-size: 11px; padding: 2px 8px; border-radius: 3px; }
    #gh-editor-panes { flex: 1; display: flex; overflow: hidden; }
    #gh-editor-ta {
      flex: 1; resize: none; border: none;
      border-right: 1px solid #0f3460;
      background: #0f0f1a; color: #e0e0e0;
      padding: 16px; outline: none;
      font-family: ui-monospace, 'SF Mono', Menlo, monospace;
      font-size: 13px; line-height: 1.6;
    }
    #gh-editor-preview {
      flex: 1; overflow-y: auto; padding: 16px;
      background: #1a1a2e; font-size: 13px; line-height: 1.55;
    }
  `;
  document.head.appendChild(style);

  // ---- settings modal ----
  function openSettings() {
    migrateIfNeeded();
    document.getElementById('gh-settings-overlay')?.remove();

    const overlay = document.createElement('div');
    overlay.id = 'gh-settings-overlay';
    const panel = document.createElement('div');
    panel.id = 'gh-settings-panel';

    let repos    = getRepos();
    let activeId = getActiveId();
    let editingId = activeId || repos[0]?.id || null;

    function collectForm() {
      const vals = {};
      panel.querySelectorAll('[data-key]').forEach(el => { vals[el.dataset.key] = el.value.trim(); });
      return vals;
    }

    function render() {
      panel.innerHTML = '';

      const h3 = document.createElement('h3');
      h3.textContent = 'GitHub Repos';
      panel.appendChild(h3);

      // Repo picker
      const pickerRow = document.createElement('div');
      pickerRow.style.cssText = 'display:flex;gap:8px;margin-bottom:14px;';
      const sel = document.createElement('select');
      sel.className = 'gh-input';
      sel.style.flex = '1';
      repos.forEach(r => {
        const opt = document.createElement('option');
        opt.value = r.id;
        opt.textContent = (r.id === activeId ? '★ ' : '') + (r.name || r.repo || 'Unnamed');
        if (r.id === editingId) opt.selected = true;
        sel.appendChild(opt);
      });
      const addOpt = document.createElement('option');
      addOpt.value = '__new__';
      addOpt.textContent = '+ New repo…';
      if (!editingId) addOpt.selected = true;
      sel.appendChild(addOpt);
      sel.onchange = () => { editingId = sel.value === '__new__' ? null : sel.value; render(); };
      pickerRow.appendChild(sel);
      panel.appendChild(pickerRow);

      // Edit form for selected repo
      const current = repos.find(r => r.id === editingId) || {};
      const fields = [
        { key: 'name',   label: 'Display name',            type: 'text',     ph: 'My Notes',      val: current.name    || '' },
        { key: 'zipUrl', label: 'Zip URL',                 type: 'text',     ph: 'https://…',     val: current.zipUrl  || '' },
        { key: 'repo',   label: 'Repository (owner/repo)', type: 'text',     ph: 'octocat/notes', val: current.repo    || '' },
        { key: 'branch', label: 'Branch',                  type: 'text',     ph: 'main',          val: current.branch  || '' },
        { key: 'token',  label: 'Personal Access Token',   type: 'password', ph: 'ghp_…',         val: current.token   || '' },
      ];
      for (const f of fields) {
        const label = document.createElement('label');
        label.className = 'gh-label';
        label.textContent = f.label;
        const input = document.createElement('input');
        input.className = 'gh-input';
        input.type = f.type;
        input.placeholder = f.ph;
        input.value = f.val;
        input.dataset.key = f.key;
        panel.appendChild(label);
        panel.appendChild(input);
      }
      const tmplLabel = document.createElement('label');
      tmplLabel.className = 'gh-label';
      tmplLabel.textContent = 'New note template';
      const tmplArea = document.createElement('textarea');
      tmplArea.className = 'gh-input';
      tmplArea.rows = 4;
      tmplArea.placeholder = '# {{title}}\n\n';
      tmplArea.style.cssText = 'resize:vertical;font-family:ui-monospace,monospace;font-size:12px;line-height:1.5;';
      tmplArea.value = current.template || '';
      tmplArea.dataset.key = 'template';
      panel.appendChild(tmplLabel);
      panel.appendChild(tmplArea);

      const note = document.createElement('p');
      note.className = 'gh-settings-note';
      note.textContent = 'Token stored in browser localStorage only. Needs Contents: write access.';
      panel.appendChild(note);

      const btns = document.createElement('div');
      btns.className = 'gh-settings-btns';

      const cancelBtn = document.createElement('button');
      cancelBtn.className = 'gh-btn';
      cancelBtn.textContent = 'Cancel';
      cancelBtn.onclick = () => overlay.remove();

      const saveBtn = document.createElement('button');
      saveBtn.className = 'gh-btn';
      saveBtn.textContent = 'Save';
      saveBtn.onclick = () => {
        const vals = collectForm();
        const existing = repos.find(r => r.id === editingId);
        if (existing) {
          Object.assign(existing, vals);
          if (existing.id === activeId) applyRepo(existing);
        } else {
          const id = crypto.randomUUID();
          repos.push({ id, ...vals, branch: vals.branch || 'main' });
          editingId = id;
        }
        saveRepos(repos);
        render();
      };

      const switchBtn = document.createElement('button');
      switchBtn.className = 'gh-btn gh-primary';
      switchBtn.textContent = editingId === activeId ? 'Reload zip' : 'Switch & Load';
      switchBtn.onclick = () => {
        const vals = collectForm();
        const existing = repos.find(r => r.id === editingId);
        let target;
        if (existing) {
          Object.assign(existing, vals);
          target = existing;
        } else {
          const id = crypto.randomUUID();
          target = { id, ...vals, branch: vals.branch || 'main' };
          repos.push(target);
        }
        saveRepos(repos);
        applyRepo(target);
        location.reload();
      };

      btns.append(cancelBtn);
      // Allow deleting non-active repos only
      if (editingId && editingId !== activeId) {
        const delBtn = document.createElement('button');
        delBtn.className = 'gh-btn gh-danger';
        delBtn.textContent = 'Delete';
        delBtn.onclick = () => {
          repos.splice(repos.findIndex(r => r.id === editingId), 1);
          saveRepos(repos);
          editingId = repos[0]?.id || null;
          render();
        };
        btns.append(delBtn);
      }
      btns.append(saveBtn, switchBtn);
      panel.appendChild(btns);
    }

    render();
    overlay.appendChild(panel);
    overlay.addEventListener('click', e => { if (e.target === overlay) overlay.remove(); });
    document.body.appendChild(overlay);
    panel.querySelector('.gh-input')?.focus();
  }

  // ---- header buttons ----
  function addHeaderButtons() {
    const chip = document.getElementById('reload-chip');
    const parent = chip.parentElement;

    const newBtn = document.createElement('button');
    newBtn.id = 'gh-new-btn';
    newBtn.title = 'New note';
    newBtn.textContent = '+ New';
    newBtn.className = 'gh-btn';
    newBtn.style.cssText = 'margin-left:auto;';
    newBtn.onclick = () => showEditor('', '', true);
    parent.insertBefore(newBtn, chip);

    const repos  = getRepos();
    const active = repos.find(r => r.id === getActiveId());
    const label  = active?.name || active?.repo || '';
    if (label) {
      const repoLabel = document.createElement('span');
      repoLabel.id = 'gh-repo-label';
      repoLabel.textContent = label;
      repoLabel.title = 'Switch repos';
      repoLabel.onclick = openSettings;
      parent.insertBefore(repoLabel, chip);
    }

    const settingsBtn = document.createElement('button');
    settingsBtn.id = 'gh-settings-btn';
    settingsBtn.title = 'GitHub Repos';
    settingsBtn.textContent = '⚙';
    settingsBtn.onclick = openSettings;
    parent.insertBefore(settingsBtn, chip);
  }

  // ---- note path from #detail-meta <code> ----
  function currentNotePath() {
    return document.querySelector('#detail-meta code')?.textContent?.trim() || '';
  }

  // ---- inject / remove note controls (Edit · Rename · Delete) ----
  function injectNoteControls() {
    if (document.getElementById('gh-edit-controls')) return;
    const controls = document.createElement('div');
    controls.id = 'gh-edit-controls';
    controls.append(makeEditBtn(), makeRenameBtn(), makeDeleteBtn());
    document.getElementById('note-sections').prepend(controls);
  }

  function makeEditBtn() {
    const btn = document.createElement('button');
    btn.className = 'gh-btn';
    btn.textContent = 'Edit';
    btn.onclick = enterEditMode;
    return btn;
  }

  function makeRenameBtn() {
    const btn = document.createElement('button');
    btn.className = 'gh-btn';
    btn.textContent = 'Rename';
    btn.onclick = () => renameNote(currentNotePath());
    return btn;
  }

  function makeDeleteBtn() {
    const btn = document.createElement('button');
    btn.className = 'gh-btn gh-danger';
    btn.textContent = 'Delete';
    btn.onclick = () => deleteNote(currentNotePath());
    return btn;
  }

  function restoreNoteControls() {
    const controls = document.getElementById('gh-edit-controls');
    if (!controls) return;
    controls.innerHTML = '';
    controls.append(makeEditBtn(), makeRenameBtn(), makeDeleteBtn());
    document.getElementById('gh-panel-status')?.remove();
  }

  function removeEditControls() {
    document.getElementById('gh-edit-controls')?.remove();
    document.getElementById('gh-panel-status')?.remove();
  }

  // ---- enter edit mode: fetch raw markdown, open modal ----
  async function enterEditMode() {
    const path = currentNotePath();
    if (!path) return;
    try {
      const res = await fetch('/api/note?path=' + encodeURIComponent(path));
      const data = await res.json();
      if (!data.ok) return;
      showEditor(data.body, path);
    } catch (e) {
      console.error('[gh-save] load failed', e);
    }
  }

  // ---- split editor modal ----
  function applyTemplate(title) {
    const tmpl = LS.get('template');
    if (!tmpl) return '';
    return tmpl.replace(/\{\{title\}\}/g, title || '');
  }

  function showEditor(markdown, path, isNew = false) {
    // For new notes, seed the body from the template.
    // If the name is already known (e.g. from an unresolved link click),
    // substitute {{title}} immediately so the preview looks right.
    if (isNew && !markdown) {
      markdown = applyTemplate(path);
    }
    document.getElementById('gh-editor-overlay')?.remove();

    const overlay = document.createElement('div');
    overlay.id = 'gh-editor-overlay';

    // top bar
    const bar = document.createElement('div');
    bar.id = 'gh-editor-bar';

    let pathInput;
    if (isNew) {
      pathInput = document.createElement('input');
      pathInput.id = 'gh-editor-title';
      pathInput.className = 'gh-input';
      pathInput.placeholder = 'Note name (e.g. My New Note)';
      pathInput.style.cssText = 'flex:1;padding:3px 8px;font-size:12px;';
    } else {
      pathInput = document.createElement('span');
      pathInput.id = 'gh-editor-title';
      pathInput.textContent = path;
    }

    const statusEl = document.createElement('span');
    statusEl.id = 'gh-modal-status';

    const cancelBtn = document.createElement('button');
    cancelBtn.className = 'gh-btn';
    cancelBtn.textContent = 'Cancel';
    cancelBtn.onclick = () => closeEditor(isNew ? null : path);

    const saveBtn = document.createElement('button');
    saveBtn.className = 'gh-btn gh-primary';
    saveBtn.textContent = 'Save to GitHub';
    saveBtn.onclick = () => saveFromModal(isNew ? null : path, isNew);

    bar.append(pathInput, statusEl, cancelBtn, saveBtn);

    // panes
    const panes = document.createElement('div');
    panes.id = 'gh-editor-panes';

    const ta = document.createElement('textarea');
    ta.id = 'gh-editor-ta';
    ta.setAttribute('spellcheck', 'true');

    // Auto-restore draft if one exists and differs from the saved content.
    const draftPath = isNew ? null : path;
    const draft = loadDraft(draftPath);
    if (draft && draft.content !== markdown) {
      ta.value = draft.content;
      const ago = Math.max(1, Math.round((Date.now() - draft.savedAt) / 60000));
      setTimeout(() => setModalStatus(`Draft auto-restored (saved ${ago}m ago)`), 0);
    } else {
      ta.value = markdown;
    }

    const preview = document.createElement('div');
    preview.id = 'gh-editor-preview';
    preview.className = 'markdown-body';

    const updatePreview = () => {
      preview.innerHTML = typeof marked !== 'undefined'
        ? marked.parse(ta.value)
        : ta.value.replace(/&/g,'&amp;').replace(/</g,'&lt;');
    };
    updatePreview();

    let debounce, draftDebounce;
    ta.addEventListener('input', () => {
      clearTimeout(debounce);
      debounce = setTimeout(updatePreview, 150);
      clearTimeout(draftDebounce);
      draftDebounce = setTimeout(() => saveDraft(draftPath, ta.value), 1000);
    });

    ta.addEventListener('keydown', e => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key === 's') { e.preventDefault(); saveFromModal(isNew ? null : path, isNew); }
      if (e.key === 'Escape')   { e.preventDefault(); closeEditor(isNew ? null : path); }
    });

    panes.append(ta, preview);
    overlay.append(bar, panes);
    document.body.appendChild(overlay);
    ta.focus();
  }

  function closeEditor(path) {
    clearDraft(path); // path is null for new notes, that's the correct draft key
    document.getElementById('gh-editor-overlay')?.remove();
    if (path) window.flowstone?.loadBody(path);
  }

  // ---- shared API config ----
  function getApiConfig() {
    const repo   = LS.get('repo');
    const token  = LS.get('token');
    const branch = LS.get('branch') || 'main';
    if (!repo || !token) { openSettings(); return null; }
    const slash = repo.indexOf('/');
    if (slash < 1) { setPanelStatus('Repository must be owner/repo.', 'err'); return null; }
    return {
      owner:    repo.slice(0, slash),
      repoName: repo.slice(slash + 1),
      branch,
      headers: {
        'Authorization': `Bearer ${token}`,
        'Accept': 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28',
      },
    };
  }

  function setPanelStatus(msg, kind) {
    let el = document.getElementById('gh-panel-status');
    if (!el) {
      el = document.createElement('div');
      el.id = 'gh-panel-status';
      document.getElementById('gh-edit-controls')?.after(el);
    }
    el.textContent = msg;
    el.style.background = kind === 'err' ? '#3a1414' : kind === 'ok' ? '#143a1a' : '#111';
    el.style.color      = kind === 'err' ? '#f4a0a0' : kind === 'ok' ? '#8dd5a8' : '#888';
  }

  // ---- rename ----
  function renameNote(path) {
    if (!path) return;
    const controls = document.getElementById('gh-edit-controls');
    if (!controls) return;
    controls.innerHTML = '';

    const input = document.createElement('input');
    input.className = 'gh-input';
    input.value = path;
    input.style.cssText = 'flex:1;padding:3px 8px;font-size:12px;min-width:0;';

    const confirmBtn = document.createElement('button');
    confirmBtn.className = 'gh-btn gh-primary';
    confirmBtn.textContent = '✓';
    confirmBtn.onclick = () => executeRename(path, input.value.trim());

    const cancelBtn = document.createElement('button');
    cancelBtn.className = 'gh-btn';
    cancelBtn.textContent = '✗';
    cancelBtn.onclick = restoreNoteControls;

    input.addEventListener('keydown', e => {
      if (e.key === 'Enter')  executeRename(path, input.value.trim());
      if (e.key === 'Escape') restoreNoteControls();
    });

    controls.append(input, confirmBtn, cancelBtn);
    input.focus();
    input.select();
  }

  async function executeRename(oldPath, newPath) {
    if (!newPath || newPath === oldPath) { restoreNoteControls(); return; }
    const cfg = getApiConfig();
    if (!cfg) return;
    const { owner, repoName, branch, headers } = cfg;
    const base = `https://api.github.com/repos/${owner}/${repoName}/contents`;

    setPanelStatus('Fetching current file…');
    try {
      // 1. GET old file — we need both SHA and raw base64 content
      const getRes = await fetch(`${base}/${oldPath}.md?ref=${encodeURIComponent(branch)}`, { headers });
      if (!getRes.ok) throw new Error(`GET ${getRes.status} ${getRes.statusText}`);
      const { sha, content: b64 } = await getRes.json();
      const content = b64.replace(/\s/g, ''); // strip whitespace GitHub adds

      // 2. PUT at new path (no sha = create)
      setPanelStatus('Creating new file…');
      const putRes = await fetch(`${base}/${newPath}.md`, {
        method: 'PUT',
        headers: { ...headers, 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: `rename ${oldPath} → ${newPath}`, content, branch }),
      });
      if (!putRes.ok) throw new Error(`PUT ${putRes.status}: ${await putRes.text()}`);

      // 3. DELETE old path
      setPanelStatus('Removing old file…');
      const delRes = await fetch(`${base}/${oldPath}.md`, {
        method: 'DELETE',
        headers: { ...headers, 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: `rename ${oldPath} → ${newPath}`, sha, branch }),
      });
      if (!delRes.ok) throw new Error(`DELETE ${delRes.status}: ${await delRes.text()}`);

      // Mirror the rename into the in-memory cozo: upsert under newPath
      // with the decoded body (so wiki-links parse), then drop oldPath.
      const decoded = base64ToUtf8(b64);
      await syncLocalDb(newPath, decoded);
      await syncLocalDelete(oldPath);

      setPanelStatus(`Renamed to ${newPath}.`, 'ok');
      restoreNoteControls();
      await window.flowstone?.selectByPath?.(newPath);
    } catch (e) {
      setPanelStatus(e.message, 'err');
      restoreNoteControls();
    }
  }

  // ---- delete ----
  async function deleteNote(path) {
    if (!path) return;
    if (!window.confirm(`Delete "${path}"?\n\nThis cannot be undone.`)) return;
    const cfg = getApiConfig();
    if (!cfg) return;
    const { owner, repoName, branch, headers } = cfg;
    const apiUrl = `https://api.github.com/repos/${owner}/${repoName}/contents/${path}.md`;

    setPanelStatus('Fetching SHA…');
    try {
      const getRes = await fetch(`${apiUrl}?ref=${encodeURIComponent(branch)}`, { headers });
      if (!getRes.ok) throw new Error(`GET ${getRes.status} ${getRes.statusText}`);
      const { sha } = await getRes.json();

      setPanelStatus('Deleting…');
      const delRes = await fetch(apiUrl, {
        method: 'DELETE',
        headers: { ...headers, 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: `delete ${path}`, sha, branch }),
      });
      if (!delRes.ok) throw new Error(`DELETE ${delRes.status}: ${await delRes.text()}`);

      await syncLocalDelete(path);
      await window.flowstone?.reload?.();
      document.getElementById('details').hidden = true;
    } catch (e) {
      setPanelStatus(e.message, 'err');
    }
  }

  // Mirror a just-committed note into the in-memory cozo so the graph
  // reflects the change without requiring a zip reload.
  async function syncLocalDb(path, body) {
    try {
      await fetch('/api/note', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ path, body }),
      });
    } catch (e) {
      console.warn('[gh-save] local cozo sync failed', e);
    }
  }

  async function syncLocalDelete(path) {
    try {
      await fetch('/api/note', {
        method: 'DELETE',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ path }),
      });
    } catch (e) {
      console.warn('[gh-save] local cozo delete failed', e);
    }
  }

  // GitHub returns note content as base64. Decode to UTF-8 so we can
  // re-parse wiki-links for the in-memory cozo on rename.
  function base64ToUtf8(b64) {
    const bin = atob(b64.replace(/\s/g, ''));
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return new TextDecoder().decode(bytes);
  }

  // ---- GitHub API ----
  async function saveFromModal(path, isNew) {
    const ta = document.getElementById('gh-editor-ta');
    if (!ta) return;

    let notePath = path;
    if (isNew) {
      const input = document.getElementById('gh-editor-title');
      notePath = input?.value?.trim();
      if (!notePath) { setModalStatus('Enter a note name.', 'err'); input?.focus(); return; }
    }

    const content = ta.value.replace(/\{\{title\}\}/g, notePath);
    const ok = await saveToGitHub(content, notePath, isNew);
    if (!ok) return;

    await syncLocalDb(notePath, content);
    clearDraft(isNew ? null : notePath);
    document.getElementById('gh-editor-overlay')?.remove();
    await window.flowstone?.selectByPath?.(notePath);
  }

  async function saveToGitHub(content, notePath, isNew = false) {
    const repo   = LS.get('repo');
    const token  = LS.get('token');
    const branch = LS.get('branch') || 'main';

    if (!repo || !token) { openSettings(); return false; }

    const slash = repo.indexOf('/');
    if (slash < 1) { setModalStatus('Repository must be owner/repo.', 'err'); return false; }

    const owner    = repo.slice(0, slash);
    const repoName = repo.slice(slash + 1);
    const filePath = notePath + '.md';
    const apiUrl   = `https://api.github.com/repos/${owner}/${repoName}/contents/${filePath}`;
    const headers  = {
      'Authorization': `Bearer ${token}`,
      'Accept': 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
    };

    try {
      let sha;
      if (!isNew) {
        setModalStatus('Fetching SHA…');
        const getRes = await fetch(`${apiUrl}?ref=${encodeURIComponent(branch)}`, { headers });
        if (!getRes.ok) throw new Error(`GET ${getRes.status} ${getRes.statusText}`);
        sha = (await getRes.json()).sha;
      }

      setModalStatus('Saving…');
      const body = {
        message: `${isNew ? 'add' : 'update'} ${notePath}`,
        content: utf8ToBase64(content),
        branch,
      };
      if (sha) body.sha = sha;

      const putRes = await fetch(apiUrl, {
        method: 'PUT',
        headers: { ...headers, 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!putRes.ok) throw new Error(`PUT ${putRes.status}: ${await putRes.text()}`);
      setModalStatus('Saved.', 'ok');
      return true;
    } catch (e) {
      setModalStatus(e.message, 'err');
      return false;
    }
  }

  function setModalStatus(msg, kind) {
    const el = document.getElementById('gh-modal-status');
    if (!el) return;
    el.textContent = msg;
    el.style.background = kind === 'err' ? '#3a1414' : kind === 'ok' ? '#143a1a' : '#111';
    el.style.color      = kind === 'err' ? '#f4a0a0' : kind === 'ok' ? '#8dd5a8' : '#888';
  }

  function utf8ToBase64(str) {
    const bytes = new TextEncoder().encode(str);
    let bin = '';
    for (const b of bytes) bin += String.fromCharCode(b);
    return btoa(bin);
  }

  // ---- MutationObserver: inject/remove note controls ----
  function watchDetails() {
    const details      = document.getElementById('details');
    const noteSections = document.getElementById('note-sections');

    const update = () => {
      if (!details.hidden && !noteSections.hidden) {
        injectNoteControls();
      } else {
        removeEditControls();
      }
    };

    const obs = new MutationObserver(update);
    obs.observe(details,      { attributes: true, attributeFilter: ['hidden'] });
    obs.observe(noteSections, { attributes: true, attributeFilter: ['hidden'] });
    update();
  }

  function init() {
    migrateIfNeeded();
    addHeaderButtons();
    watchDetails();

    // Expose the new-note flow so graph.js's createNoteAndSelect routes
    // through here instead of the in-memory POST /api/note path (which
    // doesn't commit back to GitHub and is lost on reload).
    window.flowstone = window.flowstone || {};
    window.flowstone.editNew = (path) => showEditor('', path, true);

    // Clicking an unresolved [[wiki-link]] in a note body opens the
    // new-note editor pre-filled with that name. graph.js's handler
    // runs first but no-ops (no node found); ours fires next and acts.
    document.getElementById('detail-body').addEventListener('click', (e) => {
      const a = e.target.closest('a[data-note-link].unresolved');
      if (!a) return;
      showEditor('', a.dataset.noteLink, true);
    });

    document.addEventListener('keydown', e => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key === 'n' && !document.getElementById('gh-editor-overlay')) {
        e.preventDefault();
        showEditor('', '', true);
      }
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
