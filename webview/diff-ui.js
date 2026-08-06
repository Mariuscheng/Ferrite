// ── Diff Viewer UI ─────────────────────────────────────────────────────────
// Renders unified diff previews with syntax-highlighted + / - lines,
// hunk-level accept/reject controls, and a file-level apply mechanism.
// All code is vanilla JavaScript; it operates on global DOM handles defined
// in client.js and the HTML template.

/// Container that holds the diff viewer panel DOM subtree.
var diffViewerContainer = null;

/// Current diff data being previewed (set by renderDiffViewer).
var currentDiffData = null;

// CSS class constants
var DIFF_CLASS = {
    container: 'diff-viewer',
    header: 'diff-viewer-header',
    fileLabel: 'diff-file-label',
    filePath: 'diff-file-path',
    controls: 'diff-controls',
    hunk: 'diff-hunk',
    hunkHeader: 'diff-hunk-header',
    hunkLines: 'diff-hunk-lines',
    line: 'diff-line',
    lineNum: 'diff-line-num',
    lineSign: 'diff-line-sign',
    lineText: 'diff-line-text',
    context: 'diff-context',
    added: 'diff-added',
    removed: 'diff-removed',
    hunkActions: 'diff-hunk-actions',
    btn: 'diff-btn',
    btnAccept: 'diff-btn-accept',
    btnReject: 'diff-btn-reject',
    btnApply: 'diff-btn-apply',
    btnCancel: 'diff-btn-cancel',
    summary: 'diff-summary',
    status: 'diff-status',
};

/**
 * Create the diff viewer panel DOM structure if it doesn't exist,
 * appending it inside the work panel area (wp), below the plan box.
 * Returns the root container element.
 */
function ensureDiffViewer() {
    if (diffViewerContainer) { return diffViewerContainer; }
    if (!wp) { return null; }

    diffViewerContainer = document.createElement('div');
    diffViewerContainer.className = DIFF_CLASS.container;
    diffViewerContainer.style.display = 'none';

    // Insert after planBox if present, otherwise after the work-actions row
    var insertAfter = planBox || wp.querySelector('.work-actions');
    if (insertAfter && insertAfter.parentNode) {
        insertAfter.parentNode.insertBefore(diffViewerContainer, insertAfter.nextSibling);
    } else {
        wp.appendChild(diffViewerContainer);
    }

    return diffViewerContainer;
}

/**
 * Render diff preview data into the diff viewer.
 * @param {Object} diffData  — output from the `apply_diff` tool dry-run result.
 */
function renderDiffViewer(diffData) {
    var container = ensureDiffViewer();
    if (!container) { return; }

    clearNode(container);
    currentDiffData = diffData;

    // ── Header ────────────────────────────────────────────────────────────────
    var header = makeNode('div', DIFF_CLASS.header);
    var summary = makeNode('div', DIFF_CLASS.summary);

    var files = diffData.files || 0;
    var total = diffData.totalHunks || 0;
    var applicable = diffData.applicableHunks || 0;
    var failed = total - applicable;

    summary.textContent = files + ' 個檔案 · ' + total + ' 個變更區塊'
        + (applicable < total ? '（' + applicable + ' 可套用, ' + failed + ' 無法匹配）' : '');

    var controls = makeNode('div', DIFF_CLASS.controls);

    var applyBtn = makeNode('button', [DIFF_CLASS.btn, DIFF_CLASS.btnApply].join(' '), '套用所有變更');
    applyBtn.onclick = function () { confirmApplyAll(); };
    controls.appendChild(applyBtn);

    var cancelBtn = makeNode('button', [DIFF_CLASS.btn, DIFF_CLASS.btnCancel].join(' '), '關閉');
    cancelBtn.onclick = function () { closeDiffViewer(); };
    controls.appendChild(cancelBtn);

    header.appendChild(summary);
    header.appendChild(controls);
    container.appendChild(header);

    // ── File-level sections ───────────────────────────────────────────────────
    var previews = Array.isArray(diffData.previews) ? diffData.previews : [];
    previews.forEach(function (preview, fileIdx) {
        var fileSection = makeNode('div', '');

        var fileLabel = makeNode('div', DIFF_CLASS.fileLabel);
        var fileIcon = makeNode('span', '', '📄 ');
        var filePath = makeNode('span', DIFF_CLASS.filePath, preview.file || 'unknown');
        fileLabel.appendChild(fileIcon);
        fileLabel.appendChild(filePath);

        // Per-file status
        var allOk = (preview.hunks_preview || []).every(function (h) { return h.applied; });
        var statusEl = makeNode('span', [DIFF_CLASS.status, allOk ? '' : 'diff-status-warn'].join(' '),
            allOk ? '✓ 全部可套用' : '⚠ 部分無法匹配');
        fileLabel.appendChild(statusEl);

        // Open in VS Code native diff editor — pass the current diff preview
        // data so the extension can build the "new" side without re-fetching.
        var nativeDiffBtn = makeNode('button', DIFF_CLASS.btn, '⇄ 原生 Diff');
        nativeDiffBtn.title = '在 VS Code 原生 Diff 編輯器開啟此檔案';
        nativeDiffBtn.onclick = function () {
            if (typeof v !== 'undefined') {
                v.postMessage({
                    type: 'openDiffEditor',
                    file: preview.file,
                    previewData: currentDiffData || null,
                });
            }
        };
        fileLabel.appendChild(nativeDiffBtn);

        fileSection.appendChild(fileLabel);

        // ── Hunks ──────────────────────────────────────────────────────────────
        var hunks = preview.hunks_preview || [];
        hunks.forEach(function (hunk, hunkIdx) {
            var hunkEl = makeNode('div', DIFF_CLASS.hunk);
            if (!hunk.applied) { hunkEl.classList.add('diff-hunk-failed'); }

            // Hunk header
            var hunkHeader = makeNode('div', DIFF_CLASS.hunkHeader);
            var hunkLabel = makeNode('span', '', hunk.header || ('Hunk #' + (hunkIdx + 1)));
            hunkHeader.appendChild(hunkLabel);

            if (!hunk.applied && hunk.error) {
                var errLabel = makeNode('span', 'diff-hunk-error', '⚠ ' + hunk.error);
                hunkHeader.appendChild(errLabel);
            }

            var hunkActions = makeNode('div', DIFF_CLASS.hunkActions);
            if (hunk.applied) {
                var acceptBtn = makeNode('button', [DIFF_CLASS.btn, DIFF_CLASS.btnAccept].join(' '), '✓');
                acceptBtn.title = '接受此區塊';
                acceptBtn.onclick = function () {
                    toggleHunkSelection(hunkEl, fileIdx, hunkIdx, true);
                };
                hunkActions.appendChild(acceptBtn);

                var rejectBtn = makeNode('button', [DIFF_CLASS.btn, DIFF_CLASS.btnReject].join(' '), '✗');
                rejectBtn.title = '略過此區塊';
                rejectBtn.onclick = function () {
                    toggleHunkSelection(hunkEl, fileIdx, hunkIdx, false);
                };
                hunkActions.appendChild(rejectBtn);
            }
            hunkHeader.appendChild(hunkActions);
            hunkEl.appendChild(hunkHeader);

            // Hunk lines
            var linesEl = makeNode('div', DIFF_CLASS.hunkLines);

            // Merge old and new lines side by side
            if (hunk.old_text && hunk.new_text) {
                renderSideBySide(linesEl, hunk);
            } else {
                // Fallback: just show old/new text separately
                if (hunk.old_text) {
                    var oldBlock = makeNode('div', DIFF_CLASS.removed);
                    oldBlock.textContent = hunk.old_text;
                    linesEl.appendChild(oldBlock);
                }
                if (hunk.new_text) {
                    var newBlock = makeNode('div', DIFF_CLASS.added);
                    newBlock.textContent = hunk.new_text;
                    linesEl.appendChild(newBlock);
                }
            }

            hunkEl.appendChild(linesEl);
            fileSection.appendChild(hunkEl);
        });

        container.appendChild(fileSection);
    });

    container.style.display = 'block';
    if (wp && !wp.classList.contains('show')) { toggleWorkPanel(); }
    if (typeof wStatus !== 'undefined' && wStatus) {
        wStatus.textContent = '預覽 ' + files + ' 個檔案的 diff 變更';
    }
}

/**
 * Render old/new text side by side for a hunk.
 */
function renderSideBySide(linesEl, hunk) {
    var oldLines = hunk.old_text.split('\n');
    var newLines = hunk.new_text.split('\n');

    var maxLen = Math.max(oldLines.length, newLines.length);

    var table = document.createElement('table');
    table.className = 'diff-lines-table';

    for (var i = 0; i < maxLen; i++) {
        var tr = document.createElement('tr');
        // Old side
        if (i < oldLines.length) {
            var tdOld = makeNode('td', DIFF_CLASS.removed);
            var oldSign = makeNode('span', DIFF_CLASS.lineSign, '-');
            var oldText = makeNode('span', DIFF_CLASS.lineText, oldLines[i] || '');
            tdOld.appendChild(oldSign);
            tdOld.appendChild(oldText);
            tr.appendChild(tdOld);
        } else {
            tr.appendChild(makeNode('td', 'diff-empty', ''));
        }
        // New side
        if (i < newLines.length) {
            var tdNew = makeNode('td', DIFF_CLASS.added);
            var newSign = makeNode('span', DIFF_CLASS.lineSign, '+');
            var newText = makeNode('span', DIFF_CLASS.lineText, newLines[i] || '');
            tdNew.appendChild(newSign);
            tdNew.appendChild(newText);
            tr.appendChild(tdNew);
        } else {
            tr.appendChild(makeNode('td', 'diff-empty', ''));
        }
        table.appendChild(tr);
    }

    linesEl.appendChild(table);
}

/**
 * Toggle a hunk's selected state (green border = accepted, red/strikethrough = rejected).
 * Rejected hunks are skipped during apply.
 */
function toggleHunkSelection(hunkEl, fileIdx, hunkIdx, accepted) {
    if (!currentDiffData) { return; }
    var previews = currentDiffData.previews || [];
    var preview = previews[fileIdx];
    if (!preview) { return; }
    var hunks = preview.hunks_preview || [];
    var hunk = hunks[hunkIdx];
    if (!hunk) { return; }

    // Toggle selection flag on the hunk data
    if (accepted) {
        hunk.selected = !hunk.selected;
        hunk.rejected = false;
    } else {
        hunk.rejected = !hunk.rejected;
        hunk.selected = false;
    }

    // Update visual state
    hunkEl.classList.remove('diff-hunk-selected', 'diff-hunk-rejected');
    if (hunk.selected) {
        hunkEl.classList.add('diff-hunk-selected');
    } else if (hunk.rejected) {
        hunkEl.classList.add('diff-hunk-rejected');
    }
}

/**
 * Confirm and apply all accepted hunks (non-rejected) to the workspace.
 */
function confirmApplyAll() {
    if (!currentDiffData) { return; }
    if (!confirm('確定要將所有已接受的變更區塊套用到工作區嗎？\n被標記為跳過的區塊將不會套用。\n原始檔案會自動備份為 .ferrite-bak。')) { return; }

    // Build accepted patch from current diff data
    var patchLines = [];
    var previews = currentDiffData.previews || [];

    previews.forEach(function (preview) {
        var hunks = preview.hunks_preview || [];
        var includedHunks = hunks.filter(function (h) { return h.applied && !h.rejected; });
        if (includedHunks.length === 0) { return; }

        patchLines.push('--- a/' + preview.file);
        patchLines.push('+++ b/' + preview.file);
        includedHunks.forEach(function (h) {
            // Regenerate the hunk patch from the structured hunk lines,
            // preserving context / added / removed markers exactly.
            patchLines.push(h.header);
            var hunkLines = h.lines || [];
            if (hunkLines.length > 0) {
                hunkLines.forEach(function (dl) {
                    var kind = dl.kind;
                    var text = dl.text || '';
                    if (kind === 'context') {
                        patchLines.push(' ' + text);
                    } else if (kind === 'added') {
                        patchLines.push('+' + text);
                    } else if (kind === 'removed') {
                        patchLines.push('-' + text);
                    } else {
                        patchLines.push(' ' + text);
                    }
                });
            } else {
                // Fallback (older sidecar without structured lines):
                // reconstruct from old/new text without context.
                (h.old_text || '').split('\n').forEach(function (l) { patchLines.push('-' + l); });
                (h.new_text || '').split('\n').forEach(function (l) { patchLines.push('+' + l); });
            }
        });
    });

    var patch = patchLines.join('\n');
    if (!patch.trim()) {
        showToast('沒有可套用的變更', 'error', 3000);
        return;
    }

    // Close diff viewer and send apply request
    closeDiffViewer();
    if (typeof v !== 'undefined') {
        v.postMessage({ type: 'applyDiffFromPreview', patch: patch, fuzz: 3 });
        if (typeof wStatus !== 'undefined' && wStatus) {
            wStatus.textContent = '正在套用 diff 變更...';
        }
    }
}

/**
 * Close the diff viewer panel.
 */
function closeDiffViewer() {
    if (diffViewerContainer) {
        diffViewerContainer.style.display = 'none';
        clearNode(diffViewerContainer);
    }
    currentDiffData = null;
}