// ── Client Entry Point ────────────────────────────────────────────────────
// Dependencies (loaded via inline <script> in content.ts before this file):
//   toast.js, dom-helpers.js, markdown.js, terminal.js, plan-ui.js, session-ui.js, stream.js

// Element refs for session-ui.js
slist = document.getElementById('sessionList');
nsb = document.getElementById('newSessionBtn');
sp = document.getElementById('settingsPanel');
sBtn = document.getElementById('settingsBtn');
wp = document.getElementById('workPanel');
wBtn = document.getElementById('workBtn');
wStatus = document.getElementById('workStatus');
wLabel = document.getElementById('workspaceLabel');
cP = document.getElementById('cfgP');
cK = document.getElementById('cfgK');
cE = document.getElementById('cfgE');
cM = document.getElementById('cfgM');
cT = document.getElementById('cfgT');
tV = document.getElementById('tVal');
tLab = document.getElementById('tLab');
cTo = document.getElementById('cfgTo');
cR = document.getElementById('cfgR');
cRE = document.getElementById('cfgRE');
reGrp = document.getElementById('reasoningEffortGroup');
cShell = document.getElementById('cfgShell');
cfgMT = document.getElementById('cfgMT');
svB = document.getElementById('saveBtn');
svM = document.getElementById('saveMsg');
keySt = document.getElementById('keyStatus');
imgBtn = document.getElementById('imgBtn');
imgInput = document.getElementById('imgInput');

// Element refs for plan-ui.js
genPlanBtn = document.getElementById('genPlanBtn');
previewPlanBtn = document.getElementById('previewPlanBtn');
applyPlanBtn = document.getElementById('applyPlanBtn');
validateBtn = document.getElementById('validateBtn');
planBox = document.getElementById('planBox');
planFromInputBtn = document.getElementById('planFromInputBtn');
terminalBox = document.getElementById('terminalBox');
terminalOutput = document.getElementById('terminalOutput');
clearTerminalBtn = document.getElementById('clearTerminalBtn');

// ── Input Handling ─────────────────────────────────────────────────────────

var mi = document.getElementById('msgInp');
var sb = document.getElementById('sndBtn');

function send() {
    var t = mi.value.trim();
    if (!t || loading) return;
    mi.value = '';
    mi.style.height = 'auto';
    v.postMessage({ type: 'sendMessage', message: t });
}

on(sb, 'click', send);
on(mi, 'keydown', function (e) {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
});
on(mi, 'input', function () {
    mi.style.height = 'auto';
    mi.style.height = Math.min(mi.scrollHeight, 120) + 'px';
});

// ── Image Paste ─────────────────────────────────────────────────────────────

on(mi, 'paste', function (e) {
    var items = e.clipboardData && e.clipboardData.items;
    if (!items) return;
    for (var i = 0; i < items.length; i++) {
        var item = items[i];
        if (item.type.indexOf('image') === 0) {
            e.preventDefault();
            var blob = item.getAsFile();
            if (!blob) continue;
            var reader = new FileReader();
            reader.onload = function (ev) {
                var dataUri = ev.target.result;
                var mdImg = '![image](' + dataUri + ')';
                var start = mi.selectionStart, end = mi.selectionEnd;
                var before = mi.value.substring(0, start);
                var after = mi.value.substring(end);
                mi.value = before + mdImg + after;
                mi.focus();
                mi.selectionStart = mi.selectionEnd = start + mdImg.length;
                mi.style.height = 'auto';
                mi.style.height = Math.min(mi.scrollHeight, 120) + 'px';
            };
            reader.readAsDataURL(blob);
            break;
        }
    }
});

// ── Image Button ────────────────────────────────────────────────────────────

on(imgBtn, 'click', function () {
    if (imgInput) imgInput.click();
});
on(imgInput, 'change', function () {
    if (!imgInput || !imgInput.files || !imgInput.files.length) return;
    var file = imgInput.files[0];
    if (!file.type.startsWith('image/')) { showToast('請選擇圖片檔案', 'error', 2000); return; }
    var reader = new FileReader();
    reader.onload = function (ev) {
        var dataUri = ev.target.result;
        var mdImg = '![image](' + dataUri + ')';
        var start = mi.selectionStart, end = mi.selectionEnd;
        var before = mi.value.substring(0, start);
        var after = mi.value.substring(end);
        mi.value = before + mdImg + after;
        mi.focus();
        mi.selectionStart = mi.selectionEnd = start + mdImg.length;
        mi.style.height = 'auto';
        mi.style.height = Math.min(mi.scrollHeight, 120) + 'px';
    };
    reader.readAsDataURL(file);
    imgInput.value = '';
});

// ── Event Wiring ────────────────────────────────────────────────────────────

on(cP, 'change', function () {
    cE.value = eps[cP.value] || '';
    refreshModelDropdown();
});
on(cT, 'input', function () { tV.textContent = cT.value; tLab.textContent = cT.value; });
on(cR, 'change', function () { reGrp.style.display = cR.checked ? 'block' : 'none'; });
on(sBtn, 'click', toggleSettingsPanel);
on(wBtn, 'click', toggleWorkPanel);
on(svB, 'click', function () {
    svM.className = 'save-msg'; svM.textContent = '';
    v.postMessage({
        type: 'saveConfig', config: {
            provider: cP.value, apiKey: cK.value, model: cM.value, endpoint: cE.value,
            temperature: parseFloat(cT.value), timeoutSeconds: parseInt(cTo.value) || 120,
            reasoning: cR.checked, reasoningEffort: cRE.value,
            shell: cShell ? cShell.value : '',
            maxToolIterations: cfgMT ? parseInt(cfgMT.value) || 25 : 25
        }
    });
});
on(nsb, 'click', function () { v.postMessage({ type: 'newSession' }); });
on(genPlanBtn, 'click', requestPlanFromInput);
on(planFromInputBtn, 'click', requestPlanFromInput);
on(previewPlanBtn, 'click', function () { v.postMessage({ type: 'applyPlan', preview: true }); });
on(applyPlanBtn, 'click', function () {
    if (confirm('確定要套用目前方案到工作區嗎？')) v.postMessage({ type: 'applyPlan', preview: false });
});
on(validateBtn, 'click', function () { v.postMessage({ type: 'runValidation' }); });
on(clearTerminalBtn, 'click', clearTerminalOutput);

setPlanActionState(false);

// ── Welcome Chips ───────────────────────────────────────────────────────────

var chips = document.querySelectorAll('.welcome-chip');
chips.forEach(function (chip) {
    chip.addEventListener('click', function () {
        var prompt = this.getAttribute('data-prompt') || '';
        mi.value = prompt;
        mi.focus();
        mi.style.height = 'auto';
        mi.style.height = Math.min(mi.scrollHeight, 120) + 'px';
    });
});

// ── Image Lightbox ──────────────────────────────────────────────────────────

document.addEventListener('click', function (e) {
    var target = e.target;
    if (target.tagName !== 'IMG') return;
    if (!target.closest('.message-content')) return;
    if (target.classList.contains('lightbox')) { target.classList.remove('lightbox'); return; }
    if (target.naturalWidth < 40 || target.naturalHeight < 40) return;
    e.preventDefault(); e.stopPropagation();
    var overlay = document.createElement('div'); overlay.className = 'img-overlay';
    var big = document.createElement('img'); big.src = target.src; big.alt = target.alt || '';
    overlay.appendChild(big); document.body.appendChild(overlay);
    overlay.onclick = function () { overlay.remove(); };
    document.addEventListener('keydown', function escHandler(ev) {
        if (ev.key === 'Escape') { overlay.remove(); document.removeEventListener('keydown', escHandler); }
    }, { once: true });
});

// ── Message Handler ─────────────────────────────────────────────────────────

v.postMessage({ type: 'ready' });

window.addEventListener('message', function (ev) {
    var d = ev.data;
    switch (d.type) {
        case 'sessionsList':
            sessions = d.sessions;
            sid = d.activeSessionId;
            renderSessions();
            if (!sid) wel.style.display = 'flex';
            updateSessionLabel();
            break;
        case 'sessionCreated':
            sid = d.sessionId;
            wel.style.display = 'none';
            mi.disabled = false;
            sb.disabled = false;
            updateSessionLabel('就緒');
            showToast('新對話已建立', 'success', 2000);
            break;
        case 'sessionRecovered':
            sid = d.sessionId;
            updateSessionLabel();
            break;
        case 'sessionSwitched':
            sid = d.sessionId;
            wel.style.display = 'none';
            mi.disabled = false;
            sb.disabled = false;
            updateSessionLabel('切換至');
            renderSessions();
            break;
        case 'sessionDeleted':
            sid = null;
            wel.style.display = 'flex';
            mi.disabled = false;
            sb.disabled = false;
            sT.textContent = 'AI 引擎就緒';
            if (wLabel) { wLabel.textContent = '工作區已連線'; }
            showToast('對話已刪除', 'info', 2000);
            break;
        case 'sessionRenamed':
            var renamedSid = d.sessionId || '';
            for (var i = 0; i < sessions.length; i++) {
                var s = sessions[i];
                if ((typeof s === 'string' ? s : s.id) === renamedSid) {
                    if (typeof s === 'object' && s.title !== undefined) { s.title = d.title || getSessionTitle(s); }
                    else { sessions[i] = { id: renamedSid, title: d.title || ('#' + renamedSid.substring(0, 8)) }; }
                    break;
                }
            }
            var items = slist.querySelectorAll('.sidebar-item');
            for (var j = 0; j < items.length; j++) {
                var el = items[j];
                if (el.getAttribute('data-sid') === renamedSid) {
                    var txtEl = el.querySelector('.sidebar-item-text');
                    if (txtEl) { txtEl.textContent = d.title || ('#' + renamedSid.substring(0, 8)); }
                    break;
                }
            }
            updateSessionLabel();
            showToast('對話已重新命名', 'success', 2000);
            break;
        case 'clearMessages':
            var msgs = mc.querySelectorAll('.message');
            for (var k = msgs.length - 1; k >= 0; k--) { mc.removeChild(msgs[k]); }
            wel.style.display = 'flex';
            break;
        case 'message':
            wel.style.display = 'none';
            addMsg(d.message.role, d.message.content);
            break;
        case 'error':
            addMsg('error', d.message);
            showToast(d.message, 'error', 4000);
            break;
        case 'loading':
            loading = d.isLoading;
            li.style.display = d.isLoading ? 'flex' : 'none';
            sb.disabled = d.isLoading;
            mi.disabled = d.isLoading;
            break;
        case 'configData':
            if (d.config) {
                cP.value = d.config.provider || 'deepseek';
                cE.value = d.config.endpoint || '';
                cT.value = d.config.temperature ?? 0.3;
                tV.textContent = cT.value; tLab.textContent = cT.value;
                cTo.value = d.config.timeoutSeconds ?? 120;
                cR.checked = d.config.reasoning ?? false;
                cRE.value = d.config.reasoningEffort || 'high';
                reGrp.style.display = cR.checked ? 'block' : 'none';
                if (cShell) cShell.value = d.config.shell || '';
                if (cfgMT) cfgMT.value = d.config.maxToolIterations ?? 25;
                refreshModelDropdown(d.config.model);
                if (!cM.value && modelLists[cP.value]) cM.value = modelLists[cP.value][0];
                if (d.config.apiKeyConfigured) {
                    keySt.textContent = '已設定'; keySt.className = 'key-status ok';
                    cK.value = ''; cK.placeholder = '已設定，留空則維持原值';
                } else {
                    keySt.textContent = '未設定'; keySt.className = 'key-status no';
                    cK.value = ''; cK.placeholder = 'sk-...';
                }
            }
            if (d.error) { svM.className = 'save-msg err'; svM.textContent = d.error; }
            break;
        case 'configSaved':
            svM.className = d.success ? 'save-msg ok' : 'save-msg err';
            svM.textContent = d.success ? '✅ ' + d.message : '❌ ' + d.message;
            if (d.success) {
                keySt.textContent = '已設定'; keySt.className = 'key-status ok';
                cK.value = ''; cK.placeholder = '已設定，留空則維持原值';
                showToast(d.message, 'success', 2500);
            } else {
                showToast(d.message, 'error', 4000);
            }
            setTimeout(function () { svM.className = 'save-msg'; }, 3000);
            break;
        case 'planData':
            currentPlan = d.plan || null;
            renderPlan(currentPlan);
            if (wp && !wp.classList.contains('show')) { toggleWorkPanel(); }
            addMsg('assistant', '已生成修改方案，可先預覽套用再正式套用。');
            break;
        case 'applyResult':
            renderApplyResult(d);
            if (wp && !wp.classList.contains('show')) { toggleWorkPanel(); }
            addMsg('assistant', '套用完成：成功 ' + (d.applied || []).length + '，跳過 ' + (d.skipped || []).length + '。');
            break;
        case 'validationResult':
            renderValidation(d.result);
            if (wp && !wp.classList.contains('show')) { toggleWorkPanel(); }
            addMsg('assistant', '驗證完成：' + (d.result && d.result.allPassed ? '全部通過' : '有失敗，請查看結果區塊。'));
            break;
        case 'toolEvent':
            renderToolEvent(d.event);
            // Persist terminal state after each event (debounced)
            if (typeof saveTerminalStateDebounced === 'function') {
                saveTerminalStateDebounced();
            }
            break;
        case 'toolEventReset':
            clearTerminalOutput();
            break;
        case 'terminalState':
            if (d.state) { restoreTerminalState(d.state); }
            break;
        case 'streamChunk':
            renderStreamChunk(d.chunk);
            break;
    }
});

// ── Terminal State Persistence ──────────────────────────────────────────

var terminalSaveTimer = null;
function saveTerminalStateDebounced() {
    if (terminalSaveTimer) { clearTimeout(terminalSaveTimer); }
    terminalSaveTimer = setTimeout(function () {
        terminalSaveTimer = null;
        var state = serializeTerminalState();
        v.postMessage({ type: 'saveTerminalState', state: state });
    }, 500);
}
