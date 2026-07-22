// ── Streaming Chat ─────────────────────────────────────────────────────────

var mc = document.getElementById('msgCont');
var wel = document.getElementById('welcome');
var li = document.getElementById('ldInd');
var sD = document.getElementById('sDot');
var sT = document.getElementById('sTxt');

// Streaming state
var streamBuffer = '';
var streamContentEl = null;

function renderStreamChunk(chunk) {
    if (!chunk) { return; }
    if (chunk.content) {
        streamBuffer += chunk.content;
        if (!streamContentEl) {
            wel.style.display = 'none';
            var div = makeNode('div', 'message assistant');
            var meta = makeNode('div', 'message-meta');
            var label = makeNode('span', '', 'AI Code Agent');
            meta.appendChild(label);
            var actions = makeNode('div', 'msg-actions');
            var copyBtn = makeNode('button', 'msg-action-btn', '📋');
            copyBtn.title = '複製整則訊息';
            (function () {
                var ct = streamBuffer;
                copyBtn.onclick = function (e) {
                    e.stopPropagation();
                    navigator.clipboard.writeText(ct).then(function () {
                        showToast('已複製到剪貼簿', 'success', 1800);
                    }).catch(function () {
                        showToast('複製失敗', 'error', 2000);
                    });
                };
            })();
            actions.appendChild(copyBtn);
            meta.appendChild(actions);
            div.appendChild(meta);
            streamContentEl = makeNode('div', 'message-content');
            div.appendChild(streamContentEl);
            mc.insertBefore(div, li);
        }
        streamContentEl.textContent = streamBuffer;
        mc.scrollTop = mc.scrollHeight;
    }
    if (chunk.done) {
        if (streamContentEl) {
            clearNode(streamContentEl);
            renderMarkdown(streamContentEl, streamBuffer);
        }
        streamBuffer = '';
        streamContentEl = null;
    }
}

function addMsg(r, c) {
    wel.style.display = 'none';
    var div = makeNode('div', 'message ' + r);
    if (r === 'assistant') {
        var meta = makeNode('div', 'message-meta');
        var label = makeNode('span', '', 'AI Code Agent');
        meta.appendChild(label);
        var actions = makeNode('div', 'msg-actions');
        var copyBtn = makeNode('button', 'msg-action-btn', '📋');
        copyBtn.title = '複製整則訊息';
        (function (ct) {
            copyBtn.onclick = function (e) {
                e.stopPropagation();
                navigator.clipboard.writeText(ct).then(function () {
                    showToast('已複製到剪貼簿', 'success', 1800);
                }).catch(function () {
                    showToast('複製失敗', 'error', 2000);
                });
            };
        })(String(c || ''));
        actions.appendChild(copyBtn);
        meta.appendChild(actions);
        div.appendChild(meta);
    }
    var content = makeNode('div', 'message-content');
    if (r === 'assistant') { renderMarkdown(content, String(c || '')); }
    else { content.textContent = String(c || ''); }
    div.appendChild(content);
    mc.insertBefore(div, li);
    mc.scrollTop = mc.scrollHeight;
}