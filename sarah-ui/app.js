// ── Element references ────────────────────────────────────
const $ = (id) => document.getElementById(id);
const statusEl = $('status');         // sr-only aria-live
const statusDotEl = $('statusDot');
const responseEl = $('response');       // inside response-overlay
const responseOverlay = $('responseOverlay');
const eventsEl = $('events');
const preflightEl = $('preflight');
const promptEl = $('prompt');
const avatarContainer = $('avatarContainer');
const staticAvatar = $('staticAvatar');
const live2dCanvas = $('live2dCanvas');
const stateBadge = $('stateBadge');
const demoModeBanner = $('demoModeBanner');

// Persona topbar
const personaIconEl = $('personaIcon');
const personaNameEl = $('personaName');
const costumeTagEl = $('costumeTag');

// Token ring (quota) + session meta
const tokenRingArc = $('tokenRingArc');
const tokenPctEl = $('tokenPct');          // session % — bottom bar
const sessionTokenLabelEl = $('sessionTokenLabel'); // session count — bottom bar
const ringQuotaValEl = $('ringQuotaVal');      // quota % — ring overlay
const ringQuotaSubEl = $('ringQuotaSubLabel'); // plan label below %

// Props / flash
const propsOverlay = $('propsOverlay');
const receiveFlash = $('receiveFlash');

// Thought bubble (replaces thinkingStream panel)
const thoughtBubble = $('thoughtBubble');
const thoughtTextEl = $('thoughtText');

// Todo
const todoListEl = $('todoList');       // side panel full list
const todoOverlayEl = $('todoOverlay');
const todoOverlayListEl = $('todoOverlayList');
const todoCountEl = $('todoCount');      // stab badge
const todoProgressEl = $('todoProgress');  // overlay head badge

// Side panel
const sidePanelEl = $('sidePanel');
const moreBtn = $('moreBtn');

// Mic / voice popup
const micBtn = $('micBtn');
const micArrowBtn = $('micArrowBtn');
const voicePopup = $('voicePopup');

// Skills / code
const skillsListEl = $('skillsList');
const skillsCountEl = $('skillsCount');
const codeOutputEl = $('codeOutput');
const codeLangBadgeEl = $('codeLangBadge');

// ── State ─────────────────────────────────────────────────
let live2dModel = null;  // kept for backward-compat; model is now owned by live2dAdapter
let micMuted = false;

// _demoStateIdx — see cycle setup below (after ACTIVITY_LABELS + triggerLive2DMotion)
let _demoStateIdx = 0;
let thoughtHideTimer = null;
const TOKEN_BUDGET = 10000;
const TOKEN_RING_C = 2 * Math.PI * 140;
const activeProps = new Map();
const skillsMap = new Map();
const todos = [];

// ── New: model / agent / attachment state ─────────────────
let selectedModel = 'gpt-4o';
let selectedAgent = 'ask';
const attachments = [];   // {id, name, type, isImage, content, dataUrl?}
let quotaMode = false; // true when ring shows copilot quota remaining

const ACTIVITY_LABELS = {
  Idle: '💤 待机',
  ThinkingLight: '🤔 思考中',
  ThinkingDeep: '🧠 深度推理',
  Planning: '📋 制定计划',
  TodoProgress: '✅ 执行步骤',
  UsingTool: '🔧 调用工具',
  InvokingSkill: '⚡ 调用技能',
  GeneratingCode: '💻 生成代码',
  Speaking: '🗣️ 回答中',
  Celebrating: '🎉 完成',
};

const TOOL_ICON = {
  terminal: '⌨️',
  browser: '🌐',
  filesystem: '📁',
  search: '🔍',
  code: '💻',
  mcp: '🔮',
  system: '🧩',
};

// ─────────────────────────────────────────────────────────
// STATUS — dot + sr-only element
// ─────────────────────────────────────────────────────────
function setStatus(message, isError = false) {
  if (statusEl) statusEl.textContent = message;
  if (!statusDotEl) return;
  statusDotEl.title = message;
  if (isError) {
    statusDotEl.className = 'status-dot error';
  } else if (message.includes('处理') || message.includes('正在')) {
    statusDotEl.className = 'status-dot processing';
  } else if (message.includes('预览') || message.includes('离线')) {
    statusDotEl.className = 'status-dot offline';
  } else {
    statusDotEl.className = 'status-dot';
  }
}

// ─────────────────────────────────────────────────────────
// THOUGHT BUBBLE — replaces thinking stream panel
// ─────────────────────────────────────────────────────────
function appendThinking(text) {
  if (!thoughtTextEl || !thoughtBubble) return;

  const current = thoughtTextEl.textContent || '';
  const newText = (current === '' ? '' : current + ' ') + text;
  // Keep visible portion short so bubble stays compact
  thoughtTextEl.textContent = newText.length > 180
    ? '…' + newText.slice(-160)
    : newText;

  thoughtBubble.classList.add('show');
  stateBadge?.classList.add('pulse');
  setTimeout(() => stateBadge?.classList.remove('pulse'), 350);

  clearTimeout(thoughtHideTimer);
  thoughtHideTimer = setTimeout(() => {
    thoughtBubble.classList.remove('show');
    setTimeout(() => {
      if (thoughtTextEl) thoughtTextEl.textContent = '';
    }, 350);
  }, 5000);
}

// ─────────────────────────────────────────────────────────
// RESPONSE OVERLAY
// ─────────────────────────────────────────────────────────
let _streamTimer = null;

/** Cancel any in-progress typewriter animation */
function cancelStream() {
  if (_streamTimer !== null) {
    clearTimeout(_streamTimer);
    _streamTimer = null;
  }
}

/**
 * Render `text` into responseEl with a typewriter effect.
 * Renders full Markdown at each step so formatting appears incrementally.
 */
function renderSummary(summary) {
  if (!responseEl) return;
  const fullText = summary.answer || '';
  if (!fullText.trim()) return;

  cancelStream();
  responseEl.innerHTML = '';
  responseOverlay?.classList.add('visible');

  // Configure marked: safe, no async
  if (window.marked) {
    window.marked.setOptions({ breaks: true, gfm: true });
  }

  // Typewriter: reveal one character at a time
  // Use variable speed: faster for whitespace, slower for CJK
  let pos = 0;
  const len = fullText.length;

  function tick() {
    if (pos >= len) {
      // Final render — full markdown
      _renderMarkdown(fullText);
      // Auto-scroll to bottom
      responseEl.scrollTop = responseEl.scrollHeight;
      return;
    }

    // Advance 1–3 chars per tick depending on content
    const ch = fullText[pos];
    const step = /[\u4e00-\u9fff\u3000-\u303f]/.test(ch) ? 1 : /\s/.test(ch) ? 3 : 2;
    pos = Math.min(pos + step, len);

    _renderMarkdown(fullText.slice(0, pos));
    responseEl.scrollTop = responseEl.scrollHeight;

    // Speed: ~22ms per tick ≈ ~45–90 chars/s — feels natural
    const delay = pos < len ? (/[，。！？,.!?]/.test(fullText[pos - 1]) ? 60 : 22) : 0;
    _streamTimer = setTimeout(tick, delay);
  }

  tick();
}

function _renderMarkdown(text) {
  if (!responseEl) return;
  if (window.marked) {
    responseEl.innerHTML = window.marked.parse(text);
  } else {
    // Fallback: plain text
    responseEl.textContent = text;
  }
}

function appendResponseChunk(text) {
  if (!responseEl) return;
  responseEl.textContent += text;
  responseOverlay?.classList.add('visible');
}

// ─────────────────────────────────────────────────────────
// TOKEN RING (Copilot quota) + SESSION META (bottom bar)
// ─────────────────────────────────────────────────────────

/**
 * Update the ring arc — fraction is the USED fraction (0 = nothing used, 1 = all used).
 * Low fill = green (plenty left), high fill = red (nearly exhausted).
 */
function _updateRing(fraction) {
  if (!tokenRingArc) return;
  const clamped = Math.max(0, Math.min(1, fraction));
  const dashOffset = TOKEN_RING_C * (1 - clamped);
  tokenRingArc.style.strokeDasharray = String(TOKEN_RING_C);
  tokenRingArc.style.strokeDashoffset = String(dashOffset);
  // Green = low usage (good), yellow = halfway, red = nearly exhausted
  tokenRingArc.style.stroke = clamped < 0.5 ? '#64dc78' : clamped < 0.8 ? '#ffd250' : '#ff7d91';
}

/**
 * Update the BOTTOM BAR with current session token usage.
 * Called every time a response comes back.
 */
function setTokenUsage(totalTokens) {
  const ratio = Math.min(1, totalTokens / TOKEN_BUDGET);
  const pct = Math.round(ratio * 100);
  if (sessionTokenLabelEl) sessionTokenLabelEl.textContent = `${totalTokens.toLocaleString()} tokens`;
  if (tokenPctEl) {
    tokenPctEl.textContent = `${pct}%`;
    // Color-code % by usage intensity
    tokenPctEl.style.color = pct > 80 ? 'var(--danger)' : pct > 50 ? '#ffd250' : '';
  }
  if (avatarContainer) avatarContainer.style.opacity = `${(1 - ratio * 0.4).toFixed(2)}`;
  // Fallback: show session usage on ring only if quota hasn't been fetched yet
  if (!quotaMode) _updateRing(ratio);
}

/**
 * Set the ring from the full Copilot quota object returned by get_provider_quota.
 * cq = { premium_percent_remaining, premium_remaining, premium_entitlement,
 *         premium_unlimited, chat_percent_remaining, plan }
 * Ring shows USED fraction so the arc grows as quota is consumed.
 * If premium_unlimited === true, shows ∞ and fills ring to 0 (all remaining).
 */
// Cached raw quota — used to render the hover detail popup.
let _lastCq = null;

function _renderQuotaDetailPopup(cq) {
  const el = document.getElementById('quotaDetailPopup');
  if (!el || !cq) return;

  const toNum = (v) => (v == null ? null : Number(v));
  const fmtPct = (v) => (v == null || Number.isNaN(v) ? '—' : `${Math.round(v)}%`);
  const fmtNum = (v) => (v == null || Number.isNaN(v) ? '—' : String(Math.round(v)));

  const premPct = toNum(cq.premium_percent_remaining);
  const chatPct = toNum(cq.chat_percent_remaining);
  const rem = toNum(cq.premium_remaining);
  const ent = toNum(cq.premium_entitlement);
  const plan = cq.plan ? String(cq.plan).replace(/_/g, ' ') : '—';
  const unlimited = cq.premium_unlimited === true;

  const color = (pct) => {
    if (pct == null) return 'var(--muted)';
    const used = 100 - pct;
    return used < 50 ? '#64dc78' : used < 80 ? '#ffd250' : '#ff7d91';
  };

  const rows = [
    { label: 'Plan', value: plan, clr: 'var(--text)' },
    { label: 'Unlimited', value: unlimited ? '✅ Yes' : '❌ No', clr: unlimited ? '#64dc78' : 'var(--muted)' },
    { label: 'Premium remaining', value: unlimited ? '∞' : fmtPct(premPct), clr: color(premPct) },
    { label: 'Premium used', value: unlimited ? '0%' : fmtPct(premPct != null ? 100 - premPct : null), clr: 'var(--text)' },
    {
      label: 'Quota (abs)', value: (rem != null && ent != null && !Number.isNaN(rem) && !Number.isNaN(ent))
        ? `${fmtNum(rem)} / ${fmtNum(ent)}` : '—', clr: 'var(--text)'
    },
    { label: 'Chat remaining', value: fmtPct(chatPct), clr: color(chatPct) },
  ];

  el.innerHTML = rows.map((r) =>
    `<div class="quota-row">
       <span class="quota-row-label">${r.label}</span>
       <span class="quota-row-value" style="color:${r.clr}">${r.value}</span>
     </div>`
  ).join('');
}

function setCopilotQuota(cq) {
  if (!cq) return;
  quotaMode = true;

  // Coerce values — backend may return strings instead of numbers
  const toNum = (v) => (v === undefined || v === null ? null : Number(v));

  const isUnlimited = cq.premium_unlimited === true;

  // Prefer premium quota; fall back to chat quota when premium is absent
  const rawPremPct = toNum(cq.premium_percent_remaining);
  const rawChatPct = toNum(cq.chat_percent_remaining);
  const pctRemaining = rawPremPct !== null && !Number.isNaN(rawPremPct)
    ? Math.max(0, Math.min(100, rawPremPct))
    : rawChatPct !== null && !Number.isNaN(rawChatPct)
      ? Math.max(0, Math.min(100, rawChatPct))
      : null;

  const remaining = toNum(cq.premium_remaining);
  const entitlement = toNum(cq.premium_entitlement);

  // Ring arc: used fraction (0 = nothing used, 1 = exhausted)
  const usedFraction = isUnlimited ? 0 : pctRemaining !== null ? (100 - pctRemaining) / 100 : 0;
  _updateRing(usedFraction);

  // Main quota value displayed in badge
  if (ringQuotaValEl) {
    if (isUnlimited) {
      ringQuotaValEl.textContent = '∞';
      ringQuotaValEl.style.color = '#64dc78';
    } else if (pctRemaining !== null) {
      const usedPct = Math.round(100 - pctRemaining);
      // Show "remaining / total" when available, otherwise show used%
      if (remaining !== null && entitlement !== null && !Number.isNaN(remaining) && !Number.isNaN(entitlement)) {
        ringQuotaValEl.textContent = `${remaining}/${entitlement}`;
      } else {
        ringQuotaValEl.textContent = `${usedPct}%`;
      }
      ringQuotaValEl.style.color = usedFraction < 0.5 ? '#64dc78' : usedFraction < 0.8 ? '#ffd250' : '#ff7d91';
    } else {
      // No quota info at all — show placeholder
      ringQuotaValEl.textContent = '—';
      ringQuotaValEl.style.color = '';
    }
  }

  // Sub-label: plan name + used% side by side for quick glance
  const planRaw = cq.plan ? String(cq.plan).replace(/_/g, ' ') : '';
  const pctSuffix = !isUnlimited && pctRemaining !== null
    ? `${Math.round(100 - pctRemaining)}% used`
    : '';
  const planLabel = [planRaw, pctSuffix].filter(Boolean).join(' · ') || 'Premium 已用';
  const subEl = ringQuotaSubEl || document.querySelector('.ring-quota-sub');
  if (subEl) subEl.textContent = planLabel;

  // Tooltip with full detail for hover inspection
  const badgeEl = document.getElementById('ringQuotaBadge');
  if (badgeEl) {
    const absParts = [];
    if (isUnlimited) absParts.push('Unlimited');
    if (remaining !== null && !Number.isNaN(remaining) &&
      entitlement !== null && !Number.isNaN(entitlement))
      absParts.push(`${remaining} / ${entitlement} remaining`);
    if (pctRemaining !== null) absParts.push(`${Math.round(pctRemaining)}% left`);
    if (planRaw) absParts.push(planRaw);
    badgeEl.title = absParts.join(' · ') || 'Quota unavailable';
  }

  if (avatarContainer) avatarContainer.style.opacity = '1';

  // Cache and render the hover detail popup
  _lastCq = cq;
  _renderQuotaDetailPopup(cq);
}

// ─────────────────────────────────────────────────────────
// RECEIVE FLASH
// ─────────────────────────────────────────────────────────
function flashReceive() {
  if (!receiveFlash) return;
  receiveFlash.classList.remove('show');
  void receiveFlash.offsetWidth;
  receiveFlash.classList.add('show');
}

// ─────────────────────────────────────────────────────────
// TODOS — overlay (compact) + side panel (full)
// ─────────────────────────────────────────────────────────
function renderTodos() {
  const done = todos.filter((t) => t.done).length;

  // ── Compact overlay ──
  if (todoOverlayListEl) {
    if (todos.length === 0) {
      todoOverlayEl?.classList.remove('show');
    } else {
      todoOverlayEl?.classList.add('show');
      if (todoProgressEl) todoProgressEl.textContent = `${done}/${todos.length}`;
      const shown = todos.slice(-4);
      todoOverlayListEl.innerHTML = shown.map((t) =>
        `<li class="todo-item ${t.done ? 'done' : ''}">${t.done ? '✓' : '○'} ${t.title.slice(0, 28)}</li>`
      ).join('');
    }
  }

  // ── Side panel full list ──
  if (todoListEl) {
    if (todos.length === 0) {
      todoListEl.innerHTML = '<li class="todo-empty">暂无任务</li>';
      if (todoCountEl) todoCountEl.textContent = '';
    } else {
      todoListEl.innerHTML = todos.map((t) =>
        `<li class="todo-item ${t.done ? 'done' : ''}">${t.done ? '☑' : '☐'} ${t.title}</li>`
      ).join('');
      if (todoCountEl) todoCountEl.textContent = `${done}/${todos.length}`;
    }
  }
}

// ─────────────────────────────────────────────────────────
// SKILLS — static list (from backend + disk scan) + session call history
// ─────────────────────────────────────────────────────────
async function initSkills() {
  try {
    const data = await invoke('list_skills');
    const skills = data.skills || [];
    const grid = $('skillsList');
    if (!grid) return;
    grid.className = 'skills-card-grid';
    if (skills.length === 0) {
      grid.innerHTML = '<span class="skills-empty">暂无技能定义</span>';
      return;
    }
    grid.innerHTML = skills.map((s) => {
      const icon = s.icon || '⚡';
      const isCustom = s.type === 'custom';
      return `
        <div class="skill-card${isCustom ? ' skill-card--custom' : ''}" title="${s.description || ''}">
          <div class="skill-card-top">
            <span class="skill-card-icon">${icon}</span>
            <span class="skill-card-name">${s.name}</span>
            ${isCustom ? `<span class="skill-source-badge" title="来自 ${s.source || 'custom'}">${s.source || '📄'}</span>` : ''}
          </div>
          <div class="skill-card-desc">${s.description || ''}</div>
        </div>`;
    }).join('');
  } catch (e) {
    console.debug('list_skills unavailable:', e);
  }
}

function renderSkillHistory() {
  const historyEl = $('skillsHistory');
  if (!historyEl) return;
  const rows = [...skillsMap.values()];
  if (rows.length === 0) {
    historyEl.innerHTML = '<span class="skills-empty">本次会话暂无调用</span>';
    if (skillsCountEl) skillsCountEl.textContent = '';
    return;
  }
  historyEl.innerHTML = rows.map((s) =>
    `<div class="skill-history-row"><span>${s.icon}</span><span>${s.name}</span><span style="margin-left:auto;opacity:0.6">×${s.count}</span></div>`
  ).join('');
  if (skillsCountEl) skillsCountEl.textContent = String(rows.length);
}

// Legacy alias kept so parseDomainEventMessage still works
function renderSkills() { renderSkillHistory(); }

function registerSkill(skillName) {
  const name = skillName || 'unknown';
  const prev = skillsMap.get(name) || { name, icon: '⚡', count: 0 };
  prev.count += 1;
  skillsMap.set(name, prev);
  renderSkillHistory();
}

// ─────────────────────────────────────────────────────────
// MODEL PICKER  (with hide/show persistence via localStorage)
// ─────────────────────────────────────────────────────────
const _HIDDEN_KEY = 'hiddenModels';

function _loadHiddenModels() {
  try { return new Set(JSON.parse(localStorage.getItem(_HIDDEN_KEY) || '[]')); }
  catch { return new Set(); }
}

function _saveHiddenModels(set) {
  localStorage.setItem(_HIDDEN_KEY, JSON.stringify([...set]));
}

// allModels kept so we can re-render after show/hide toggle
let _allModels = [];
let _hiddenModels = _loadHiddenModels();

function _renderModelDropdown(dropdown, models) {
  const shown = models.filter((m) => !_hiddenModels.has(m.id));
  const hidden = models.filter((m) => _hiddenModels.has(m.id));

  // Optionally group by "group" field
  const grouped = shown.reduce((acc, m) => {
    const g = m.group || m.provider || '';
    (acc[g] = acc[g] || []).push(m);
    return acc;
  }, {});

  let html = '';
  for (const [grp, items] of Object.entries(grouped)) {
    if (grp) html += `<div class="model-group-label">${grp}</div>`;
    html += items.map((m) => `
      <div class="model-option${m.id === selectedModel ? ' selected' : ''}"
           data-id="${m.id}" data-provider="${m.provider}" role="option"
           aria-selected="${m.id === selectedModel}">
        <div class="model-option-info">
          <div class="model-option-name">${m.name}</div>
        </div>
        <div class="model-option-actions">
          ${m.supportsVision ? '<span class="model-option-badge vision" title="支持图像">👁</span>' : ''}
          <button class="model-hide-btn" data-hide-id="${m.id}" title="隐藏此模型" aria-label="隐藏 ${m.name}">✕</button>
        </div>
      </div>`).join('');
  }

  // Hidden models section (collapsed toggle)
  if (hidden.length > 0) {
    html += `<div class="model-group-label model-hidden-toggle" id="modelHiddenToggle" style="cursor:pointer" title="点击展开隐藏的模型">
               🙈 ${hidden.length} 个已隐藏
             </div>
             <div id="modelHiddenList" style="display:none">` +
      hidden.map((m) => `
               <div class="model-option model-option--hidden"
                    data-id="${m.id}" data-provider="${m.provider}" role="option"
                    aria-selected="${m.id === selectedModel}">
                 <div class="model-option-info">
                   <div class="model-option-name" style="opacity:0.5">${m.name}</div>
                 </div>
                 <button class="model-hide-btn model-show-btn" data-show-id="${m.id}" title="重新显示 ${m.name}" aria-label="显示 ${m.name}">👁</button>
               </div>`).join('') +
      '</div>';
  }

  dropdown.innerHTML = html;

  // Bind click to select model
  dropdown.querySelectorAll('.model-option').forEach((el) => {
    el.addEventListener('click', (e) => {
      if (e.target.closest('.model-option-actions button')) return; // let button handle
      selectModel(el.dataset.id, el.dataset.provider,
        el.querySelector('.model-option-name')?.textContent || el.dataset.id);
    });
  });

  // Bind hide buttons
  dropdown.querySelectorAll('.model-hide-btn').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const id = btn.dataset.hideId;
      if (id) {
        _hiddenModels.add(id);
        _saveHiddenModels(_hiddenModels);
        // If current model hidden, fall back to first visible
        if (selectedModel === id) {
          const first = _allModels.find((m) => !_hiddenModels.has(m.id));
          if (first) selectModel(first.id, first.provider, first.name);
        }
        _renderModelDropdown(dropdown, _allModels);
      }
    });
  });

  // Bind show buttons
  dropdown.querySelectorAll('.model-show-btn').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const id = btn.dataset.showId;
      if (id) {
        _hiddenModels.delete(id);
        _saveHiddenModels(_hiddenModels);
        _renderModelDropdown(dropdown, _allModels);
      }
    });
  });

  // Bind hidden toggle
  document.getElementById('modelHiddenToggle')?.addEventListener('click', () => {
    const list = document.getElementById('modelHiddenList');
    if (list) list.style.display = list.style.display === 'none' ? 'block' : 'none';
  });
}

async function initModelPicker() {
  try {
    const data = await invoke('list_models');
    _allModels = data.models || [];
    if (!_allModels.length) return;

    const defaultId = data.default || _allModels[0].id;
    const initial = _allModels.find((m) => m.id === defaultId) || _allModels[0];
    selectedModel = initial.id;
    const nameEl = $('modelName');
    if (nameEl) nameEl.textContent = initial.name;

    const dropdown = $('modelDropdown');
    if (!dropdown) return;
    _renderModelDropdown(dropdown, _allModels);
  } catch (e) {
    console.debug('list_models unavailable:', e);
  }
}

async function selectModel(id, provider, displayName) {
  selectedModel = id;
  const nameEl = $('modelName');
  if (nameEl) nameEl.textContent = displayName || id;

  document.querySelectorAll('.model-option').forEach((el) => {
    el.classList.toggle('selected', el.dataset.id === id);
    el.setAttribute('aria-selected', String(el.dataset.id === id));
  });

  closeModelDropdown();
  try {
    await invoke('set_active_model', { provider: provider || 'openai', model: id });
    setStatus(`模型已切换 → ${displayName || id} ✅`);
  } catch (e) {
    console.warn('set_active_model:', e);
  }
}

function openModelDropdown() {
  $('modelDropdown')?.classList.add('open');
  $('modelPickerBtn')?.classList.add('open');
  $('modelPickerBtn')?.setAttribute('aria-expanded', 'true');
}

function closeModelDropdown() {
  $('modelDropdown')?.classList.remove('open');
  $('modelPickerBtn')?.classList.remove('open');
  $('modelPickerBtn')?.setAttribute('aria-expanded', 'false');
}

$('modelPickerBtn')?.addEventListener('click', (e) => {
  e.stopPropagation();
  const isOpen = $('modelDropdown')?.classList.contains('open');
  if (isOpen) closeModelDropdown(); else openModelDropdown();
});

document.addEventListener('click', (e) => {
  if (!$('modelPickerWrap')?.contains(e.target)) closeModelDropdown();
});

// ─────────────────────────────────────────────────────────
// AGENT PICKER
// ─────────────────────────────────────────────────────────
const BUILTIN_AGENT_ICONS = { ask: '💬', plan: '📋', code: '💻', agent: '🤖' };

// Dynamic prefix map — populated by initAgentPicker().
// Built-in entries seeded here as fallback.
const agentPrefixes = new Map([
  ['ask', ''],
  ['plan', '请逐步思考并列出详细执行计划，然后回答：\n\n'],
  ['code', '请以代码为主要输出格式，提供完整可运行代码：\n\n'],
  ['agent', '请作为自主 Agent，分析任务并使用所有可用工具/技能完成它：\n\n'],
]);

async function initAgentPicker() {
  const dropdown = $('agentDropdown');
  if (!dropdown) return;

  // ── Fallback built-in list ────────────────────────────
  let agents = [
    { id: 'ask', name: 'Ask', type: 'builtin', icon: '💬', description: '单轮问答，直接回复' },
    { id: 'plan', name: 'Plan', type: 'builtin', icon: '📋', description: '逐步推理，制定计划' },
    { id: 'code', name: 'Code', type: 'builtin', icon: '💻', description: '代码为主，给出完整实现' },
    { id: 'agent', name: 'Agent', type: 'builtin', icon: '🤖', description: '自主 Agent，调用工具完成任务' },
  ];

  try {
    const data = await invoke('list_agents');
    if (data.agents?.length) agents = data.agents;
  } catch (e) {
    console.debug('list_agents unavailable:', e);
  }

  // Seed prefix map for custom agents
  agents.forEach((a) => {
    if (!agentPrefixes.has(a.id)) {
      agentPrefixes.set(a.id, `[${a.name}模式] `);
    }
  });

  // Set initial display
  const initial = agents[0];
  selectedAgent = initial.id;
  const nameEl = $('agentModeName');
  const iconEl = $('agentModeIcon');
  if (nameEl) nameEl.textContent = initial.name;
  if (iconEl) iconEl.textContent = initial.icon || BUILTIN_AGENT_ICONS[initial.id] || '✨';

  // Render dropdown
  dropdown.innerHTML = agents.map((a) => {
    const icon = a.icon || BUILTIN_AGENT_ICONS[a.id] || '✨';
    const isCustom = a.type === 'custom';
    return `
      <div class="model-option agent-option${a.id === selectedAgent ? ' selected' : ''}"
           data-id="${a.id}" data-icon="${icon}"
           role="option" aria-selected="${a.id === selectedAgent}">
        <div class="agent-option-icon">${icon}</div>
        <div class="model-option-info">
          <div class="model-option-name">${a.name}</div>
          <div class="model-option-desc">${a.description || ''}</div>
        </div>
        ${isCustom ? `<span class="model-option-badge agent-source-badge" title="来自 ${a.source || 'custom'}">📄</span>` : ''}
      </div>`;
  }).join('');

  dropdown.querySelectorAll('.agent-option').forEach((el) => {
    el.addEventListener('click', () => {
      const name = el.querySelector('.model-option-name')?.textContent || el.dataset.id;
      selectAgent(el.dataset.id, el.dataset.icon, name);
    });
  });
}

function selectAgent(id, icon, displayName) {
  selectedAgent = id;
  const nameEl = $('agentModeName');
  const iconEl = $('agentModeIcon');
  if (nameEl) nameEl.textContent = displayName || id;
  if (iconEl) iconEl.textContent = icon || '💬';
  document.querySelectorAll('.agent-option').forEach((el) => {
    el.classList.toggle('selected', el.dataset.id === id);
    el.setAttribute('aria-selected', String(el.dataset.id === id));
  });
  closeAgentDropdown();
}

function openAgentDropdown() {
  $('agentDropdown')?.classList.add('open');
  $('agentPickerBtn')?.classList.add('open');
  $('agentPickerBtn')?.setAttribute('aria-expanded', 'true');
}

function closeAgentDropdown() {
  $('agentDropdown')?.classList.remove('open');
  $('agentPickerBtn')?.classList.remove('open');
  $('agentPickerBtn')?.setAttribute('aria-expanded', 'false');
}

$('agentPickerBtn')?.addEventListener('click', (e) => {
  e.stopPropagation();
  const isOpen = $('agentDropdown')?.classList.contains('open');
  if (isOpen) closeAgentDropdown(); else openAgentDropdown();
});

document.addEventListener('click', (e) => {
  if (!$('agentPickerWrap')?.contains(e.target)) closeAgentDropdown();
});

// ─────────────────────────────────────────────────────────
// ATTACHMENTS
// ─────────────────────────────────────────────────────────
function addAttachment(file) {
  const isImage = file.type.startsWith('image/');
  const chip = { id: Date.now() + Math.random(), name: file.name, type: file.type, isImage, content: '' };
  const reader = new FileReader();
  if (isImage) {
    reader.onload = (ev) => {
      chip.dataUrl = ev.target.result;
      chip.content = '';  // images: model sees filename only for now
      attachments.push(chip);
      _renderAttachChips();
    };
    reader.readAsDataURL(file);
  } else {
    reader.onload = (ev) => {
      const text = ev.target.result || '';
      chip.content = `\n\n[附件: ${file.name}]\n\`\`\`\n${text.slice(0, 8000)}\n\`\`\``;
      attachments.push(chip);
      _renderAttachChips();
    };
    reader.readAsText(file);
  }
}

function removeAttachment(id) {
  const idx = attachments.findIndex((a) => a.id === id);
  if (idx >= 0) attachments.splice(idx, 1);
  _renderAttachChips();
}
// expose globally so inline onclick works
window.removeAttachment = removeAttachment;

function _renderAttachChips() {
  const container = $('attachChips');
  if (!container) return;
  if (attachments.length === 0) {
    container.style.display = 'none';
    container.innerHTML = '';
    return;
  }
  container.style.display = 'flex';
  container.innerHTML = attachments.map((a) => `
    <div class="attach-chip">
      ${a.isImage && a.dataUrl
    ? `<img class="attach-chip-img" src="${a.dataUrl}" alt="${a.name}" />`
    : `<span class="attach-chip-icon">${a.isImage ? '🖼️' : '📄'}</span>`}
      <span class="attach-chip-name">${a.name}</span>
      <button class="attach-chip-remove" onclick="removeAttachment(${a.id})" aria-label="移除">✕</button>
    </div>`).join('');
}

// Attach button click
$('attachBtn')?.addEventListener('click', () => $('fileInput')?.click());

// File input change
$('fileInput')?.addEventListener('change', (e) => {
  for (const f of e.target.files) addAttachment(f);
  e.target.value = '';
});

// Paste image from clipboard
promptEl?.addEventListener('paste', (e) => {
  for (const item of (e.clipboardData?.items || [])) {
    if (item.type.startsWith('image/')) {
      e.preventDefault();
      const f = item.getAsFile();
      if (f) addAttachment(f);
    }
  }
});

// Drag & drop onto input bar
const _inputBar = document.querySelector('.input-bar');
_inputBar?.addEventListener('dragover', (e) => { e.preventDefault(); _inputBar.classList.add('drag-over'); });
_inputBar?.addEventListener('dragleave', () => _inputBar.classList.remove('drag-over'));
_inputBar?.addEventListener('drop', (e) => {
  e.preventDefault();
  _inputBar.classList.remove('drag-over');
  for (const f of (e.dataTransfer?.files || [])) addAttachment(f);
});

// ─────────────────────────────────────────────────────────
// PROPS OVERLAY (tool icons near character)
// ─────────────────────────────────────────────────────────
function addProp(toolRaw, action) {
  if (!propsOverlay) return;
  const key = `${toolRaw}-${Date.now()}`;
  const tool = (toolRaw || 'system').toLowerCase();
  const icon = TOOL_ICON[tool] || TOOL_ICON.system;

  const positions = [
    { top: '30px', left: '10px' },
    { top: '110px', right: '8px' },
    { bottom: '130px', left: '6px' },
    { bottom: '60px', right: '6px' },
  ];
  const idx = activeProps.size % positions.length;

  const el = document.createElement('div');
  el.className = 'prop-item';
  el.innerHTML = `<span>${icon}</span><span>${(action || tool).slice(0, 10)}</span>`;
  Object.assign(el.style, positions[idx]);
  propsOverlay.appendChild(el);
  activeProps.set(key, { el, tool });

  if (activeProps.size > 4) {
    const [oldKey, oldVal] = activeProps.entries().next().value;
    oldVal.el.remove();
    activeProps.delete(oldKey);
  }
}

function removeProp(toolRaw) {
  const tool = (toolRaw || 'system').toLowerCase();
  for (const [key, value] of activeProps.entries()) {
    if (value.tool === tool) {
      value.el.classList.add('fade-out');
      setTimeout(() => value.el.remove(), 340);
      activeProps.delete(key);
      break;
    }
  }
}

// ─────────────────────────────────────────────────────────
// CODE OUTPUT
// ─────────────────────────────────────────────────────────
function setCodeBlock(language, preview) {
  if (!codeOutputEl || !codeLangBadgeEl) return;
  codeLangBadgeEl.textContent = language || 'text';
  codeOutputEl.textContent = preview || '';
  // Auto-open side panel to code tab
  openSidePanel();
  activateStab('code');
}

// ─────────────────────────────────────────────────────────
// PERSONA
// ─────────────────────────────────────────────────────────
function updatePersona(payload) {
  if (!payload) return;
  const { icon, display_name, costume_tag, accent_color } = payload;
  if (personaIconEl && icon) personaIconEl.textContent = icon;
  if (personaNameEl && display_name) personaNameEl.textContent = display_name;
  if (costumeTagEl && costume_tag) costumeTagEl.textContent = costume_tag;
  if (accent_color) {
    document.documentElement.style.setProperty('--accent', accent_color);
  }
}

// ─────────────────────────────────────────────────────────
// EVENTS
// ─────────────────────────────────────────────────────────
function appendEventLine(line) {
  if (!eventsEl) return;
  if (eventsEl.textContent === '暂无事件') {
    eventsEl.textContent = line;
  } else {
    eventsEl.textContent += `\n${line}`;
    // Trim to last 200 lines
    const lines = eventsEl.textContent.split('\n');
    if (lines.length > 200) eventsEl.textContent = lines.slice(-200).join('\n');
  }
  eventsEl.scrollTop = eventsEl.scrollHeight;
}

function parseDomainEventMessage(msg) {
  if (!msg) return;

  if (msg.includes('UserInputReceived')) flashReceive();

  if (msg.includes('TokenUsageUpdated')) {
    const match = msg.match(/total_tokens:\s*(\d+)/);
    if (match) setTokenUsage(Number(match[1]));
  }

  if (msg.includes('ToolCallStarted')) {
    const tool = msg.match(/tool:\s*"([^"]+)"/)?.[1] || 'system';
    const action = msg.match(/action:\s*"([^"]+)"/)?.[1] || tool;
    addProp(tool, action);
  }

  if (msg.includes('ToolCallFinished')) {
    const tool = msg.match(/tool:\s*"([^"]+)"/)?.[1] || 'system';
    removeProp(tool);
  }

  if (msg.includes('ModelThinkingChunk')) {
    const text = msg.match(/text:\s*"([\s\S]*)"\s*\}/)?.[1] || 'thinking...';
    appendThinking(text.replace(/\\n/g, '\n'));
  }

  if (msg.includes('AgentPlanCreated')) {
    todos.length = 0;
    const rawList = msg.match(/todos:\s*\[(.*)\]\s*\}/)?.[1] || '';
    rawList.split(',')
      .map((x) => x.trim().replace(/^"|"$/g, ''))
      .filter(Boolean)
      .forEach((title) => todos.push({ title, done: false }));
    renderTodos();
  }

  if (msg.includes('AgentTodoUpdated')) {
    const index = Number(msg.match(/index:\s*(\d+)/)?.[1] || '0');
    const title = msg.match(/title:\s*"([^"]+)"/)?.[1] || `todo-${index}`;
    const done = msg.includes('done: true');
    todos[index] = { title, done };
    renderTodos();
  }

  if (msg.includes('AgentCodeGenerated')) {
    const lang = msg.match(/language:\s*"([^"]+)"/)?.[1] || 'text';
    const preview = msg.match(/preview:\s*"([\s\S]*)"\s*\}/)?.[1] || '';
    setCodeBlock(lang, preview.replace(/\\n/g, '\n'));
  }

  if (msg.includes('SkillInvoked')) {
    const skill = msg.match(/skill:\s*"([^"]+)"/)?.[1] || 'unknown';
    registerSkill(skill);
  }

  if (msg.includes('PersonaChanged')) {
    updatePersona({
      display_name: msg.match(/display_name:\s*"([^"]+)"/)?.[1],
      icon: msg.match(/icon:\s*"([^"]+)"/)?.[1],
      costume_tag: msg.match(/costume_tag:\s*"([^"]+)"/)?.[1],
      accent_color: msg.match(/accent_color:\s*"([^"]+)"/)?.[1],
    });
  }
}

// ─────────────────────────────────────────────────────────
// SIDE PANEL
// ─────────────────────────────────────────────────────────
let _currentStab = 'todo';

function openSidePanel() {
  sidePanelEl?.classList.add('open');
  moreBtn?.classList.add('active');
  moreBtn?.setAttribute('aria-expanded', 'true');
  sidePanelEl?.setAttribute('aria-hidden', 'false');
}

function closeSidePanel() {
  sidePanelEl?.classList.remove('open');
  moreBtn?.classList.remove('active');
  moreBtn?.setAttribute('aria-expanded', 'false');
  sidePanelEl?.setAttribute('aria-hidden', 'true');
}

function toggleSidePanel() {
  if (sidePanelEl?.classList.contains('open')) {
    closeSidePanel();
  } else {
    openSidePanel();
  }
}

function activateStab(name) {
  _currentStab = name;
  document.querySelectorAll('.stab').forEach((btn) => {
    btn.classList.toggle('active', btn.dataset.stab === name);
  });
  document.querySelectorAll('.spanel').forEach((panel) => {
    panel.classList.toggle('active', panel.id === `spanel-${name}`);
  });
  // Load settings data when switching to settings tab
  if (name === 'settings') void loadSettingsPanel();
}

moreBtn?.addEventListener('click', () => {
  toggleSidePanel();
});

document.querySelector('.stab-bar')?.addEventListener('click', (e) => {
  const btn = e.target.closest('.stab');
  if (!btn || !btn.dataset.stab) return;
  const name = btn.dataset.stab;
  if (sidePanelEl?.classList.contains('open') && _currentStab === name) {
    // Same tab clicked again — toggle close
    closeSidePanel();
  } else {
    openSidePanel();
    activateStab(name);
  }
});

// ─────────────────────────────────────────────────────────
// MIC TOGGLE
// ─────────────────────────────────────────────────────────
function setMicMuted(muted) {
  micMuted = muted;
  micBtn?.classList.toggle('muted', muted);
  if ($('micMuteToggle')) $('micMuteToggle').checked = muted;
}

micBtn?.addEventListener('click', () => {
  setMicMuted(!micMuted);
});

// ─────────────────────────────────────────────────────────
// VOICE POPUP
// ─────────────────────────────────────────────────────────
function toggleVoicePopup() {
  const isOpen = voicePopup?.classList.contains('show');
  voicePopup?.classList.toggle('show', !isOpen);
  voicePopup?.setAttribute('aria-hidden', String(isOpen));
  micArrowBtn?.classList.toggle('open', !isOpen);
}

micArrowBtn?.addEventListener('click', (e) => {
  e.stopPropagation();
  toggleVoicePopup();
});

$('closeVoicePopup')?.addEventListener('click', () => {
  voicePopup?.classList.remove('show');
  voicePopup?.setAttribute('aria-hidden', 'true');
  micArrowBtn?.classList.remove('open');
});

$('micMuteToggle')?.addEventListener('change', (e) => {
  setMicMuted(e.target.checked);
});

$('voice-btn-mic')?.addEventListener('click', () => openSystemPrefs('Microphone'));

// Close popup when clicking outside
document.addEventListener('click', (e) => {
  if (voicePopup?.classList.contains('show') &&
    !voicePopup.contains(e.target) &&
    e.target !== micArrowBtn) {
    voicePopup.classList.remove('show');
    voicePopup.setAttribute('aria-hidden', 'true');
    micArrowBtn?.classList.remove('open');
  }
});

// ─────────────────────────────────────────────────────────
// TEXTAREA — auto-expand + Enter to send
// ─────────────────────────────────────────────────────────
const MAX_TEXTAREA_H = 120;

if (promptEl) {
  promptEl.addEventListener('input', () => {
    promptEl.style.height = 'auto';
    promptEl.style.height = Math.min(promptEl.scrollHeight, MAX_TEXTAREA_H) + 'px';
    // Show overflow when at max height
    promptEl.style.overflowY = promptEl.scrollHeight > MAX_TEXTAREA_H ? 'auto' : 'hidden';
  });

  promptEl.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void submit('text');
    }
  });
}

// ─────────────────────────────────────────────────────────
// CLEAR RESPONSE
// ─────────────────────────────────────────────────────────
$('clearResponseBtn')?.addEventListener('click', () => {
  if (responseEl) responseEl.textContent = '';
  responseOverlay?.classList.remove('visible');
});

// Clear all logs button (in settings panel)
$('clearAllBtn')?.addEventListener('click', () => {
  if (responseEl) responseEl.textContent = '';
  if (eventsEl) eventsEl.textContent = '暂无事件';
  if (preflightEl) preflightEl.textContent = '等待检测...';
  if (thoughtTextEl) thoughtTextEl.textContent = '';
  if (todoListEl) todoListEl.innerHTML = '<li class="todo-empty">暂无任务</li>';
  if (todoOverlayListEl) todoOverlayListEl.innerHTML = '';
  if (codeOutputEl) codeOutputEl.textContent = '暂无代码';
  todos.length = 0;
  skillsMap.clear();
  renderTodos();
  renderSkills();
  thoughtBubble?.classList.remove('show');
  responseOverlay?.classList.remove('visible');
  setStatus('日志已清空 ✅');
});

// ─────────────────────────────────────────────────────────
// WINDOW SIZING
// ─────────────────────────────────────────────────────────
function syncLayout() {
  // Side panel is now a fixed overlay — no window resize needed.
  // Keep function as no-op so existing call sites are harmless.
}

// ─────────────────────────────────────────────────────────
// TAURI IPC
// ─────────────────────────────────────────────────────────
async function waitForTauri(maxMs = 5000) {
  if (window.__TAURI__?.invoke) return true;
  const step = 80;
  let elapsed = 0;
  while (elapsed < maxMs) {
    await new Promise((r) => setTimeout(r, step));
    elapsed += step;
    if (window.__TAURI__?.invoke) return true;
  }
  return false;
}

async function invoke(command, payload) {
  if (!window.__TAURI__?.invoke)
    throw new Error('Tauri IPC 不可用。请通过 cargo tauri dev 启动。');
  return window.__TAURI__.invoke(command, payload);
}

// ─────────────────────────────────────────────────────────
// SUBMIT (text / voice)
// ─────────────────────────────────────────────────────────
async function submit(mode) {
  const rawInput = promptEl?.value.trim();
  if (!rawInput && attachments.length === 0) {
    setStatus('请输入内容后再发送', true);
    return;
  }

  // Build the prompt: agent prefix + user text + file attachments
  let input = agentPrefixes.get(selectedAgent) ?? '';
  input += rawInput || '';

  // Append text file contents
  for (const att of attachments) {
    if (!att.isImage) input += att.content;
  }
  // Mention image attachments by name (vision support can be added later)
  const imgs = attachments.filter((a) => a.isImage);
  if (imgs.length > 0) {
    input += `\n\n[已附加 ${imgs.length} 张图片: ${imgs.map((a) => a.name).join(', ')}]`;
  }

  if (!input.trim()) { setStatus('请输入内容后再发送', true); return; }

  setStatus(mode === 'voice' ? '正在处理语音输入...' : '正在处理文本输入...');
  try {
    const summary = await invoke(mode === 'voice' ? 'submit_voice' : 'submit_text', { input });
    renderSummary(summary);
    setStatus('处理完成 ✅');
    if (promptEl) {
      promptEl.value = '';
      promptEl.style.height = 'auto';
      promptEl.style.overflowY = 'hidden';
    }
    // Clear attachments after send
    attachments.length = 0;
    _renderAttachChips();
  } catch (error) {
    setStatus(`处理失败：${error}`, true);
  }
}

// ─────────────────────────────────────────────────────────
// LIVE2D — Cubism 4 SDK (loaded via vendor/*.js, not CDN)
// ─────────────────────────────────────────────────────────

async function initLive2D() {
  try {
    if (typeof PIXI === 'undefined' ||
      typeof L2D === 'undefined' ||
      typeof window.live2dAdapter === 'undefined') {
      throw new Error('Cubism SDK not ready — check vendor scripts in index.html');
    }

    live2dAdapter.init(
      live2dCanvas,
      'assets',
      'dujiaoshou_4',
      () => {
        // Model fully loaded and idle is playing
        staticAvatar.style.display = 'none';
        live2dCanvas.style.display = 'block';
        setStatus('Live2D 模型加载成功 ✨');
        console.log('[Live2D] motions:', live2dAdapter.listMotions().join(', '));
      }
    );
  } catch (err) {
    console.warn('Live2D 初始化失败，保留动态 CSS 占位:', err.message);
  }
}

// Maps backend activity names → dujiaoshou_4 motion names.
// Motion stems come from motions/*.motion3.json filenames.
const MOTION_MAP = {
  Idle: 'idle',          // peaceful waiting
  ThinkingLight: 'home',          // calm, relaxed
  ThinkingDeep: 'main_3',        // concentrated
  Planning: 'mission',       // determined
  TodoProgress: 'main_1',        // busy & active
  UsingTool: 'touch_special', // special action
  InvokingSkill: 'main_2',        // skilled motion
  GeneratingCode: 'main_3',        // focused output
  Speaking: 'touch_head',    // engaged conversation
  Celebrating: 'complete',      // success!
};

function triggerLive2DMotion(state, activity) {
  const name = MOTION_MAP[activity] || 'idle';
  if (window.live2dAdapter) window.live2dAdapter.startMotion(name);
}

// ─────────────────────────────────────────────────────────
// STATE DEMO — ↻ button + avatar click + 8-second auto-cycle
// ─────────────────────────────────────────────────────────

// Exposed so the auto-timer below can call it without restructuring the IIFE.
let _applyDemoState = null;

(function bindStateCycle() {
  const STATES = Object.keys(ACTIVITY_LABELS);

  function applyDemoState(idx) {
    const activity = STATES[idx];
    const cssState = activity === 'Idle' ? 'Idle' : 'Active';
    if (avatarContainer) {
      avatarContainer.className =
        `avatar-container state-${cssState} activity_${activity.toLowerCase()}`;
    }
    if (stateBadge) {
      stateBadge.textContent = ACTIVITY_LABELS[activity] || activity;
      stateBadge.classList.add('pulse');
      setTimeout(() => stateBadge.classList.remove('pulse'), 350);
    }
    triggerLive2DMotion(cssState, activity);
  }

  // Share with the auto-timer.
  _applyDemoState = applyDemoState;

  $('stateCycleBtn')?.addEventListener('click', () => {
    _demoStateIdx = (_demoStateIdx + 1) % STATES.length;
    applyDemoState(_demoStateIdx);
  });

  $('avatarContainer')?.addEventListener('click', (e) => {
    if (e.target.tagName === 'BUTTON') return;
    _demoStateIdx = (_demoStateIdx + 1) % STATES.length;
    applyDemoState(_demoStateIdx);
  });
}());

// ── 自动动作循环：每 8 秒触发下一个动作（仅在 Live2D 模型已加载后生效）──────────
(function startAutoMotionCycle() {
  const STATES = Object.keys(ACTIVITY_LABELS);
  setInterval(() => {
    // 仅当 Live2D 适配器已就绪时才触发，避免模型未加载时报错
    if (!window.live2dAdapter) return;
    _demoStateIdx = (_demoStateIdx + 1) % STATES.length;
    if (_applyDemoState) _applyDemoState(_demoStateIdx);
  }, 8000);
}());

// ─────────────────────────────────────────────────────────
// PREFLIGHT
// ─────────────────────────────────────────────────────────
function renderPreflight(preflight) {
  const lines = [
    `isMacOS:            ${preflight.isMacos}`,
    `automationEnabled:  ${preflight.automationEnabled}`,
    `accessibility:      ${preflight.accessibility}`,
    `microphone:         ${preflight.microphone}`,
    `screenRecording:    ${preflight.screenRecording}`,
    `frontmostApp:       ${preflight.frontmostApp ?? 'unknown'}`,
  ];
  if (preflight.frontmostAppError) lines.push(`frontmostAppError: ${preflight.frontmostAppError}`);
  if (preflightEl) preflightEl.textContent = lines.join('\n');
}

async function loadRecentEvents() {
  try {
    const events = await invoke('recent_events');
    if (!Array.isArray(events) || events.length === 0) {
      if (eventsEl) eventsEl.textContent = '暂无事件';
      return;
    }
    if (eventsEl) eventsEl.textContent = events.join('\n');
    for (const ev of events) parseDomainEventMessage(ev);
  } catch (error) {
    setStatus(`读取事件失败：${error}`, true);
  }
}

// ─────────────────────────────────────────────────────────
// SETTINGS PANEL
// ─────────────────────────────────────────────────────────
async function openSystemPrefs(pane) {
  try { await invoke('open_system_prefs', { pane }); }
  catch (e) { setStatus(`无法打开系统偏好: ${e}`, true); }
}

async function saveApiKey(key, value) {
  try {
    await invoke('save_api_key', { key, value });
    setStatus(`已保存 ${key} ✅`);
  } catch (e) {
    setStatus(`保存失败：${e}`, true);
  }
}

function updatePermBadge(id, status) {
  const el = document.getElementById(id);
  if (!el) return;
  const s = String(status).toLowerCase();
  const isGranted = s === 'granted' || s === 'true';
  const isDenied = s === 'denied' || s === 'false';
  el.textContent = isGranted ? '✓ 已授权' : isDenied ? '✗ 已拒绝' : '? 未知';
  el.className = 'perm-status ' + (isGranted ? 'granted' : isDenied ? 'denied' : 'unknown');
}

function renderProviderStatus(p) {
  const el = $('providerStatus');
  if (!el) return;
  el.innerHTML = '';
  [
    ['Anthropic', p.anthropicKey, 'Claude 系列'],
    ['OpenAI', p.openaiKey, 'GPT 系列'],
    ['Copilot', p.copilotToken, 'GitHub Copilot'],
    ['claude CLI', p.claudeCli, 'CLI'],
    ['codex CLI', p.codexCli, 'CLI'],
    ['gemini CLI', p.geminiCli, 'Gemini'],
  ].forEach(([name, ok, hint]) => {
    const chip = document.createElement('div');
    chip.className = `provider-chip ${ok ? 'ok' : 'fail'}`;
    chip.title = ok ? `${name}: 已配置 (${hint})` : `${name}: 未配置或 Token 格式无效`;
    chip.innerHTML = `<span class="dot"></span><span>${name}</span>`;
    el.appendChild(chip);
  });
}

async function loadSettingsPanel() {
  try {
    const preflight = await invoke('macos_preflight');
    updatePermBadge('perm-accessibility', preflight.accessibility);
    updatePermBadge('perm-microphone', preflight.microphone);
    updatePermBadge('perm-screen', preflight.screenRecording);
    updatePermBadge('perm-automation', preflight.automationEnabled ? 'Granted' : 'Denied');
    // Voice popup perm badge
    updatePermBadge('voice-perm-mic', preflight.microphone);
    renderPreflight(preflight);
  } catch (_) { }

  try {
    const providers = await invoke('detect_providers');
    renderProviderStatus(providers);
  } catch (_) { }
}

// Permission buttons
$('btn-perm-accessibility')?.addEventListener('click', () => openSystemPrefs('Accessibility'));
$('btn-perm-microphone')?.addEventListener('click', () => openSystemPrefs('Microphone'));
$('btn-perm-screen')?.addEventListener('click', () => openSystemPrefs('ScreenCapture'));
$('btn-perm-automation')?.addEventListener('click', () => openSystemPrefs('Automation'));

// API key buttons
$('save-anthropic')?.addEventListener('click', () => {
  void saveApiKey('ANTHROPIC_API_KEY', $('input-anthropic')?.value || '');
});
$('save-openai')?.addEventListener('click', () => {
  void saveApiKey('OPENAI_API_KEY', $('input-openai')?.value || '');
});
$('save-copilot')?.addEventListener('click', () => {
  void saveApiKey('COPILOT_API_TOKEN', $('input-copilot')?.value || '');
});

// ─────────────────────────────────────────────────────────
// INIT
// ─────────────────────────────────────────────────────────
async function init() {
  syncLayout();
  void initLive2D();
  setTokenUsage(0);
  renderTodos();

  const tauriReady = await waitForTauri(5000);
  if (!tauriReady) {
    if (demoModeBanner) demoModeBanner.style.display = 'block';
    setStatus('预览模式（无 Tauri 后端）');
    if (preflightEl) preflightEl.textContent = '需要 Tauri 桌面环境运行完整功能。\n请用: cargo tauri dev';
    return;
  }

  try {
    const health = await invoke('health_check');
    if (health !== 'ok') throw new Error(`health check failed: ${health}`);
    setStatus('已连接 orchestrator 内核');
  } catch (error) {
    setStatus(`初始化失败：${error}`, true);
    return;
  }

  try {
    const preflight = await invoke('macos_preflight');
    renderPreflight(preflight);
    updatePermBadge('voice-perm-mic', preflight.microphone);
  } catch (error) {
    if (preflightEl) preflightEl.textContent = `预检失败: ${error}`;
  }

  // Initialise agent/model pickers, skills panel, and Copilot quota ring
  void initAgentPicker();
  void initModelPicker();
  void initSkills();

  async function refreshQuota() {
    try {
      const quotaData = await invoke('get_provider_quota');
      const cq = quotaData?.copilot;
      if (cq) setCopilotQuota(cq);
    } catch (_) { }
  }
  await refreshQuota();
  // Refresh quota every 5 minutes
  setInterval(refreshQuota, 5 * 60 * 1000);

  await loadRecentEvents();

  if (window.__TAURI__?.event?.listen) {
    await window.__TAURI__.event.listen('domain-event', (evt) => {
      const msg = evt?.payload?.message;
      if (!msg) return;
      appendEventLine(msg);
      parseDomainEventMessage(msg);
    });

    await window.__TAURI__.event.listen('avatar-state', (evt) => {
      const payload = evt?.payload;
      if (!payload) return;
      const { state, activity, activityHint } = payload;
      if (avatarContainer) {
        avatarContainer.className = `avatar-container state-${state} ${activityHint || 'activity_idle'}`;
      }
      if (stateBadge) {
        stateBadge.textContent = ACTIVITY_LABELS[activity] || activityHint || activity || state;
      }
      triggerLive2DMotion(state, activity);
    });
  }
}

void init();
