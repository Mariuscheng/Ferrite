// ── Terminal UI ────────────────────────────────────────────────────────────
// Standalone component for command execution output display.

var terminalBox, terminalOutput, clearTerminalBtn;

/// Terminal output line buffer (persisted across session switches).
var terminalLines = [];

/**
 * Clear all terminal output lines.
 */
function clearTerminalOutput() {
    terminalLines = [];
    clearNode(terminalOutput);
    if (terminalBox) { terminalBox.hidden = true; }
}

/**
 * Append a line to the terminal output area and keep it in the buffer.
 * @param {string} text
 * @param {string} [className]
 */
function appendTerminalOutput(text, className) {
    if (!terminalOutput || text === undefined || text === null) { return; }
    terminalLines.push({ text: String(text), className: className || '' });
    terminalOutput.appendChild(makeNode('span', className, String(text)));
    terminalOutput.scrollTop = terminalOutput.scrollHeight;
}

/**
 * Replay the buffered terminal lines into the DOM.
 * Called when restoring a session's terminal state.
 */
function replayTerminalOutput() {
    clearNode(terminalOutput);
    var lines = terminalLines;
    for (var i = 0; i < lines.length; i++) {
        var line = lines[i];
        terminalOutput.appendChild(makeNode('span', line.className, line.text));
    }
    terminalOutput.scrollTop = terminalOutput.scrollHeight;
    if (terminalBox && lines.length > 0) { terminalBox.hidden = false; }
}

/**
 * Serialize terminal state for persistence (stored in session metadata).
 * @returns {string} JSON string of terminal lines.
 */
function serializeTerminalState() {
    return JSON.stringify(terminalLines);
}

/**
 * Deserialize terminal state from JSON and replay into the DOM.
 * @param {string} json
 */
function restoreTerminalState(json) {
    try {
        var parsed = JSON.parse(json);
        if (Array.isArray(parsed)) {
            terminalLines = parsed;
            replayTerminalOutput();
        }
    } catch (_) {
        // Ignore parse errors; start with a fresh terminal.
        terminalLines = [];
    }
}

/**
 * Render a tool event (toolStart / toolOutput / toolComplete).
 * @param {Object} event
 */
function renderToolEvent(event) {
    if (!event || typeof event !== 'object') { return; }
    if (terminalBox) { terminalBox.hidden = false; }
    if (typeof toggleWorkPanel === 'function' && wp && !wp.classList.contains('show')) { toggleWorkPanel(); }
    if (event.type === 'toolStart') {
        var command = String(event.command || event.tool || 'command');
        appendTerminalOutput('$ ' + command + '\n', 'terminal-command');
        if (typeof wStatus !== 'undefined' && wStatus) { wStatus.textContent = '正在執行 ' + command; }
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
        if (typeof wStatus !== 'undefined' && wStatus) { wStatus.textContent = passed ? '命令執行完成' : '命令執行失敗'; }
    }
}