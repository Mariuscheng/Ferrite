// ── Plan UI ───────────────────────────────────────────────────────────────

var genPlanBtn, previewPlanBtn, applyPlanBtn, validateBtn;
var planBox, planFromInputBtn;
var terminalBox, terminalOutput, clearTerminalBtn;
var currentPlan = null;

/**
 * Enable / disable plan action buttons based on whether a plan exists.
 * @param {boolean} hasPlan
 */
function setPlanActionState(hasPlan) {
    if (previewPlanBtn) { previewPlanBtn.disabled = !hasPlan; }
    if (applyPlanBtn) { applyPlanBtn.disabled = !hasPlan; }
}

/**
 * Take the current input value as the plan goal and clear the input.
 * @returns {string}
 */
function takePlanGoal() {
    var goal = mi.value.trim();
    if (!goal) {
        mi.focus();
        if (wStatus) { wStatus.textContent = '先在輸入框描述想要修改的內容'; }
        return '';
    }
    mi.value = '';
    return goal;
}

/**
 * Request a plan from the current input text.
 */
function requestPlanFromInput() {
    var goal = takePlanGoal();
    if (!goal) { return; }
    addMsg('user', '修改方案：' + goal);
    if (wp && !wp.classList.contains('show')) { toggleWorkPanel(); }
    if (wStatus) { wStatus.textContent = '正在依目前工作區建立方案'; }
    v.postMessage({ type: 'generatePlan', goal: goal });
}

/**
 * Add a collapsible section block to the plan box.
 * @param {string} title
 * @returns {HTMLElement}
 */
function addPlanSection(title) {
    var section = makeNode('div', 'plan-section');
    section.appendChild(makeNode('div', 'plan-section-title', title));
    planBox.appendChild(section);
    return section;
}

/**
 * Append a list (ordered or unordered) to a parent element.
 * @param {HTMLElement} parent
 * @param {string[]} items
 * @param {boolean} ordered
 */
function appendList(parent, items, ordered) {
    var list = makeNode(ordered ? 'ol' : 'ul', 'plan-list');
    items.forEach(function (item) { list.appendChild(makeNode('li', '', item)); });
    parent.appendChild(list);
}

/**
 * Render the edit plan data into the plan box.
 * @param {Object} plan
 */
function renderPlan(plan) {
    clearNode(planBox);
    if (!plan || typeof plan !== 'object') {
        planBox.appendChild(makeNode('div', 'work-empty', '模型沒有回傳可用的修改方案。'));
        setPlanActionState(false);
        return;
    }
    planBox.appendChild(makeNode('div', 'plan-summary', plan.summary || '已建立修改方案'));
    var steps = Array.isArray(plan.steps) ? plan.steps : [];
    if (steps.length) { appendList(addPlanSection('步驟'), steps, true); }
    var edits = Array.isArray(plan.edits) ? plan.edits : [];
    if (edits.length) {
        var editsSection = addPlanSection('預計修改');
        edits.forEach(function (edit) {
            var item = makeNode('div', 'plan-edit');
            item.appendChild(makeNode('div', '', edit.path || '未指定檔案'));
            if (edit.reason) { item.appendChild(makeNode('div', '', edit.reason)); }
            editsSection.appendChild(item);
        });
    }
    var validations = Array.isArray(plan.validationCommands) ? plan.validationCommands : [];
    if (validations.length) { appendList(addPlanSection('驗證'), validations, false); }
    var notes = Array.isArray(plan.notes) ? plan.notes : [];
    if (notes.length) { appendList(addPlanSection('注意事項'), notes, false); }
    if (!steps.length && !edits.length && !validations.length && !notes.length) {
        planBox.appendChild(makeNode('div', 'work-empty', '方案沒有包含可執行的修改內容。'));
    }
    if (wStatus) { wStatus.textContent = edits.length ? '已準備 ' + edits.length + ' 項修改' : '方案不包含檔案修改'; }
    setPlanActionState(edits.length > 0);
}

/**
 * Render the apply result (preview or real).
 * @param {Object} result
 */
function renderApplyResult(result) {
    clearNode(planBox);
    var applied = Array.isArray(result.applied) ? result.applied : [];
    var skipped = Array.isArray(result.skipped) ? result.skipped : [];
    planBox.appendChild(makeNode('div', 'plan-summary', result.preview ? '預覽完成' : '修改已套用'));
    if (applied.length) {
        var appliedSection = addPlanSection(result.preview ? '可套用修改' : '已套用修改');
        applied.forEach(function (edit) {
            var item = makeNode('div', 'plan-edit');
            item.appendChild(makeNode('div', '', edit.path || '未指定檔案'));
            if (edit.reason) { item.appendChild(makeNode('div', '', edit.reason)); }
            if (result.preview && edit.before !== undefined && edit.after !== undefined) {
                var before = makeNode('div', 'diff-block', 'Before: ' + edit.before);
                var after = makeNode('div', 'diff-block', 'After: ' + edit.after);
                item.appendChild(before); item.appendChild(after);
            }
            appliedSection.appendChild(item);
        });
    }
    if (skipped.length) {
        var skippedSection = addPlanSection('未處理');
        skipped.forEach(function (item) { skippedSection.appendChild(makeNode('div', 'plan-edit', (item.path || '未指定檔案') + '：' + (item.reason || '未知原因'))); });
    }
    if (!applied.length && !skipped.length) { planBox.appendChild(makeNode('div', 'work-empty', '沒有可以處理的修改。')); }
    if (wStatus) { wStatus.textContent = result.preview ? '請確認預覽後再套用' : '可執行驗證確認結果'; }
}

/**
 * Render validation results.
 * @param {Object} result
 */
function renderValidation(result) {
    clearNode(planBox);
    var passed = Boolean(result && result.allPassed);
    planBox.appendChild(makeNode('div', 'plan-summary', passed ? '驗證全部通過' : '驗證完成，請查看失敗項目'));
    var items = result && Array.isArray(result.results) ? result.results : [];
    if (!items.length) { planBox.appendChild(makeNode('div', 'work-empty', '沒有驗證輸出。')); }
    items.forEach(function (item) {
        var section = makeNode('div', 'plan-edit');
        section.appendChild(makeNode('div', '', item.success ? '通過：' + item.command : '失敗：' + item.command));
        var output = item.output || item.error || '';
        if (output) { section.appendChild(makeNode('div', 'diff-block', output)); }
        planBox.appendChild(section);
    });
    if (wStatus) { wStatus.textContent = passed ? '驗證通過' : '驗證有失敗項目'; }
}

/**
 * Clear all terminal output.
 */
function clearTerminalOutput() {
    clearNode(terminalOutput);
    if (terminalBox) { terminalBox.hidden = true; }
}

/**
 * Append a line to the terminal output area.
 * @param {string} text
 * @param {string} [className]
 */
function appendTerminalOutput(text, className) {
    if (!terminalOutput || text === undefined || text === null) { return; }
    terminalOutput.appendChild(makeNode('span', className, String(text)));
    terminalOutput.scrollTop = terminalOutput.scrollHeight;
}

/**
 * Render a tool event (toolStart / toolOutput / toolComplete).
 * @param {Object} event
 */
function renderToolEvent(event) {
    if (!event || typeof event !== 'object') { return; }
    if (terminalBox) { terminalBox.hidden = false; }
    if (wp && !wp.classList.contains('show')) { toggleWorkPanel(); }
    if (event.type === 'toolStart') {
        var command = String(event.command || event.tool || 'command');
        appendTerminalOutput('$ ' + command + '\n', 'terminal-command');
        if (wStatus) { wStatus.textContent = '正在執行 ' + command; }
        return;
    }
    if (event.type === 'toolOutput') {
        appendTerminalOutput(event.output || '', event.stream === 'stderr' ? 'terminal-line stderr' : 'terminal-line');
        return;
    }
    if (event.type === 'toolComplete') {
        if (event.output) { appendTerminalOutput(event.output + '\n', 'terminal-line stderr'); }
        var passed = event.success === true;
        var exitCode = typeof event.exitCode === 'number' ? event.exitCode : '?';
        appendTerminalOutput('[完成：' + (passed ? '通過' : '失敗') + '，exit ' + exitCode + ']\n', passed ? 'terminal-status ok' : 'terminal-status fail');
        if (wStatus) { wStatus.textContent = passed ? '命令執行完成' : '命令執行失敗'; }
    }
}