// ── Session UI ─────────────────────────────────────────────────────────────

var slist, nsb;
var sp, sBtn;
var wp, wBtn;
var wStatus, wLabel;
var cP, cK, cE, cM;
var cT, tV, tLab;
var cTo, cR;
var cRE, reGrp;
var cShell, cfgMT;
var svB, svM;
var keySt;
var imgBtn, imgInput;

var v = acquireVsCodeApi();
var modelLists = MODEL_LISTS;
var eps = ENDPOINT_PRESETS;

var sid = null, loading = false, sessions = [];

// ── Panel Toggle ────────────────────────────────────────────────────────

function toggleSettingsPanel() {
    if (!sp || !sBtn) { console.error('settings panel/button missing'); return; }
    var opened = sp.classList.toggle('show');
    if (opened) {
        if (wp) { wp.classList.remove('show'); }
        if (wBtn) { wBtn.classList.remove('active'); }
        sBtn.classList.add('active');
        v.postMessage({ type: 'getConfig' });
    } else {
        sBtn.classList.remove('active');
    }
}

function toggleWorkPanel() {
    if (!wp || !wBtn) { console.error('work panel/button missing'); return; }
    var opened = wp.classList.toggle('show');
    if (opened) {
        if (sp) { sp.classList.remove('show'); }
        if (sBtn) { sBtn.classList.remove('active'); }
        wBtn.classList.add('active');
    } else {
        wBtn.classList.remove('active');
    }
}

// ── Model Dropdown ──────────────────────────────────────────────────────

function refreshModelDropdown(preferred) {
    var p = cP.value, list = modelLists[p] || [];
    var cur = preferred || cM.value;
    cM.innerHTML = '';
    list.forEach(function (m) { cM.innerHTML += '<option value="' + m + '"' + (m === cur ? ' selected' : '') + '>' + m + '</option>'; });
    if (!list.includes(cur) && list.length > 0) cM.value = list[0];
}

// ── Session Helpers ─────────────────────────────────────────────────────

function getSessionTitle(s) {
    return (s && s.title) ? s.title : '#' + (s.id || s).substring(0, 8);
}

function findSessionTitle(sidVal) {
    var found = sessions.find(function (s) { return (s.id || s) === sidVal; });
    return found ? getSessionTitle(found) : '#' + (sidVal || '').substring(0, 8);
}

function updateSessionLabel(prefix) {
    var label = prefix ? prefix + ' ' + findSessionTitle(sid) : findSessionTitle(sid) || 'AI 引擎就緒';
    sT.textContent = label;
    if (wLabel) { wLabel.textContent = '工作區已連線 · 對話 ' + findSessionTitle(sid); }
}

// ── Session List Rendering ──────────────────────────────────────────────

function renderSessions() {
    slist.innerHTML = '';
    if (sessions.length === 0) { slist.innerHTML = '<div class="no-sessions">尚無對話</div>'; return; }
    var reversed = sessions.slice().reverse();
    reversed.forEach(function (s) {
        var sidVal = typeof s === 'string' ? s : s.id;
        var d = document.createElement('div');
        d.className = 'sidebar-item' + (sidVal === sid ? ' active' : '');
        d.setAttribute('data-sid', sidVal);
        var txt = document.createElement('span');
        txt.className = 'sidebar-item-text';
        txt.textContent = getSessionTitle(s);
        var del = document.createElement('span');
        del.className = 'sidebar-item-del';
        del.textContent = '\u00D7';
        d.appendChild(txt);
        d.appendChild(del);

        var switching = false;
        txt.onclick = function (e) {
            e.stopPropagation();
            if (switching) return;
            switching = true;
            v.postMessage({ type: 'switchSession', sessionId: sidVal });
            setTimeout(function () { switching = false; }, 500);
        };

        var titleForRename = getSessionTitle(s);
        txt.oncontextmenu = function (e) {
            e.preventDefault();
            e.stopPropagation();
            if (switching) return;
            startSessionRename(d, txt, sidVal, titleForRename);
        };

        del.onclick = function (e) { e.stopPropagation(); v.postMessage({ type: 'deleteSession', sessionId: sidVal }); };
        slist.appendChild(d);
    });
}

function startSessionRename(container, textEl, sidVal, currentTitle) {
    var inp = document.createElement('input');
    inp.className = 'session-rename-input';
    inp.value = currentTitle;
    inp.style.width = '100%';
    inp.style.boxSizing = 'border-box';
    textEl.style.display = 'none';
    container.insertBefore(inp, textEl.nextSibling);
    inp.focus();
    inp.select();
    var commit = function () {
        var newTitle = inp.value.trim();
        container.removeChild(inp);
        textEl.style.display = '';
        if (newTitle && newTitle !== currentTitle) {
            v.postMessage({ type: 'renameSession', sessionId: sidVal, title: newTitle });
        }
    };
    inp.onblur = function () { commit(); };
    inp.onkeydown = function (e) {
        if (e.key === 'Enter') { e.preventDefault(); commit(); }
        else if (e.key === 'Escape') { inp.value = currentTitle; commit(); }
    };
}