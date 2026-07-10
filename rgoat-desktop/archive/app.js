// ── RGoat Desktop Frontend ──
// Uses Tauri invoke() for IPC, listens for agent-event for streaming

const { invoke } = window.__TAURI__?.core || {};
const { listen } = window.__TAURI__?.event || {};

let currentSessionId = null;
let isProcessing = false;

// ── DOM elements ──
const messagesEl = document.getElementById('messages');
const promptInput = document.getElementById('promptInput');
const btnSend = document.getElementById('btnSend');
const providerBadge = document.getElementById('providerBadge');
const providerSelect = document.getElementById('providerSelect');
const modeSelect = document.getElementById('modeSelect');
const sessionInfo = document.getElementById('sessionInfo');
const statusDot = document.getElementById('statusDot');

// ── Initialization ──
async function init() {
  // Check if providers exist
  let hasProviders = false;
  try {
    hasProviders = await invoke('has_configured_provider');
  } catch (e) {
    console.warn('has_configured_provider not available:', e);
  }

  if (!hasProviders) {
    showSetup();
    return;
  }

  showApp();
  await refreshProviders();
  await refreshCurrentProvider();
  setStatus('idle');
}

// ── Provider Management ──
async function refreshProviders() {
  try {
    const providers = await invoke('list_providers');
    providerSelect.innerHTML = '';
    providers.forEach(p => {
      const opt = document.createElement('option');
      opt.value = p.name;
      opt.textContent = `${p.is_current ? '✓ ' : ''}${p.name} (${p.model})`;
      providerSelect.appendChild(opt);
    });
  } catch (e) {
    console.error('Failed to list providers:', e);
  }
}

async function refreshCurrentProvider() {
  try {
    const name = await invoke('get_current_provider');
    providerBadge.textContent = name || 'no provider';
  } catch (e) {
    providerBadge.textContent = 'error';
  }
}

providerSelect.addEventListener('change', async () => {
  const name = providerSelect.value;
  if (!name) return;
  try {
    await invoke('switch_provider', { name });
    await refreshProviders();
    await refreshCurrentProvider();
    addMessage('system', `Switched to provider: ${name}`);
  } catch (e) {
    addMessage('error', `Failed to switch: ${e}`);
  }
});

// ── Event Listener ──
async function setupEventListener() {
  // Tauri v2 event API: listen is from @tauri-apps/api/event
  if (listen) {
    await listen('agent-event', (event) => {
      handleAgentEvent(event.payload);
    });
  } else {
    // Fallback: poll for events (dev mode without @tauri-apps/api)
    console.warn('Tauri event API not available, using fallback');
  }
}

function handleAgentEvent(payload) {
  const { eventType, data } = payload;
  if (!data) return;

  try {
    const agentEvent = typeof data === 'string' ? JSON.parse(data) : data;
    const type = agentEvent.type;

    switch (type) {
      case 'started':
        setStatus('processing');
        break;
      case 'thought':
        if (agentEvent.content) {
          addMessage('thought', agentEvent.content);
        }
        break;
      case 'tool_call':
        addMessage('tool-start', `🔧 ${agentEvent.tool_name}`);
        break;
      case 'tool_result':
        const ok = agentEvent.success ? '✓' : '✗';
        const cls = agentEvent.success ? 'tool-result' : 'tool-result error';
        const summary = summarize(agentEvent.output, 100);
        addMessage(cls, `${ok} ${agentEvent.tool_name}: ${summary}`);
        break;
      case 'approval':
        addMessage('approval', `🛡 ${agentEvent.tool_name} → ${agentEvent.decision}`);
        break;
      case 'message':
        if (agentEvent.content) {
          addMessage('assistant', agentEvent.content);
        }
        break;
      case 'finished':
        if (agentEvent.answer) {
          addMessage('assistant', agentEvent.answer);
        }
        setStatus('idle');
        break;
      case 'error':
        addMessage('error', agentEvent.message);
        setStatus('idle');
        break;
    }
  } catch (e) {
    // Non-AgentEvent format — ignore
  }
}

// ── Message Display ──
function addMessage(role, content) {
  const div = document.createElement('div');
  div.className = `msg ${role}`;
  div.textContent = content;
  messagesEl.appendChild(div);
  messagesEl.scrollTop = messagesEl.scrollHeight;
}

function summarize(text, max) {
  if (!text) return '';
  const firstLine = text.split('\n')[0] || '';
  const total = text.split('\n').length;
  if (total <= 1) return firstLine.length > max ? firstLine.slice(0, max) + '...' : firstLine;
  return `${firstLine.slice(0, max)} (${total} lines)`;
}

// ── Status ──
function setStatus(state) {
  isProcessing = state === 'processing';
  statusDot.className = `status ${state}`;
  btnSend.disabled = isProcessing;
  promptInput.disabled = isProcessing;
}

// ── Send Prompt ──
async function sendPrompt() {
  const prompt = promptInput.value.trim();
  if (!prompt || isProcessing) return;

  addMessage('user', prompt);
  promptInput.value = '';

  try {
    const response = await invoke('send_prompt', {
      request: {
        prompt,
        sessionId: currentSessionId,
        mode: modeSelect.value,
      },
    });
    currentSessionId = response.session_id;
    sessionInfo.textContent = `session: ${currentSessionId.slice(0, 8)}`;
    setStatus('processing');
  } catch (e) {
    addMessage('error', `Error: ${e}`);
    setStatus('idle');
  }
}

// ── Event Handlers ──
btnSend.addEventListener('click', sendPrompt);
promptInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault();
    sendPrompt();
  }
});

document.getElementById('btnClear').addEventListener('click', () => {
  messagesEl.innerHTML = '';
});

document.getElementById('btnNewSession').addEventListener('click', () => {
  currentSessionId = null;
  messagesEl.innerHTML = '';
  sessionInfo.textContent = '';
  addMessage('system', 'New session started. Send your first prompt!');
});

// ── Startup ──
init();
setupEventListener();
addMessage('system', 'Welcome to RGoat Desktop! Type a prompt to get started.');
