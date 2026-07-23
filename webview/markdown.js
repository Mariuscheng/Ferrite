// ── Markdown Renderer ─────────────────────────────────────────────────────

var codeFence = '```';

/**
 * Render inline bold and code tokens.
 * @param {HTMLElement} target
 * @param {string} text
 */
function appendInline(target, text) {
    var cursor = 0;
    var codeMarker = '`';
    while (cursor < text.length) {
        var boldAt = text.indexOf('**', cursor);
        var codeAt = text.indexOf(codeMarker, cursor);
        var next = -1, kind = '';
        if (boldAt >= 0 && (codeAt < 0 || boldAt < codeAt)) { next = boldAt; kind = 'bold'; }
        else if (codeAt >= 0) { next = codeAt; kind = 'code'; }
        if (next < 0) { target.appendChild(document.createTextNode(text.slice(cursor))); break; }
        if (next > cursor) { target.appendChild(document.createTextNode(text.slice(cursor, next))); }
        var markerLength = kind === 'bold' ? 2 : 1;
        var close = text.indexOf(kind === 'bold' ? '**' : codeMarker, next + markerLength);
        if (close < 0) { target.appendChild(document.createTextNode(text.slice(next, next + markerLength))); cursor = next + markerLength; continue; }
        var node = makeNode(kind === 'bold' ? 'strong' : 'code', '', text.slice(next + markerLength, close));
        target.appendChild(node);
        cursor = close + markerLength;
    }
}

/**
 * Render full Markdown content into the given container.
 * Supports paragraphs, headings, blockquotes, lists, code fences,
 * tables, images, and line-numbered file content filtering.
 *
 * @param {HTMLElement} container
 * @param {string} content
 */
function renderMarkdown(container, content) {
    var lines = String(content || '').split('\r').join('').split('\n');
    var paragraph = [], list = null, listType = '', inCode = false, codeLines = [], codeLang = '';
    var inTable = false, tableRows = [], tableAlign = [];

    function flushParagraph() {
        if (!paragraph.length) { return; }
        var p = makeNode('p', '');
        appendInline(p, paragraph.join(' '));
        container.appendChild(p);
        paragraph = [];
    }
    function flushList() { if (list) { container.appendChild(list); list = null; listType = ''; } }
    function flushTable() {
        if (!inTable || tableRows.length < 1) { inTable = false; tableRows = []; tableAlign = []; return; }
        var table = makeNode('table', '');
        var thead = makeNode('thead', ''); var tr = makeNode('tr', '');
        tableRows[0].forEach(function (cell) {
            var th = makeNode('th', ''); appendInline(th, cell); tr.appendChild(th);
        });
        thead.appendChild(tr); table.appendChild(thead);
        var tbody = makeNode('tbody', '');
        for (var ri = 1; ri < tableRows.length; ri++) {
            var row = tableRows[ri];
            var tr2 = makeNode('tr', '');
            row.forEach(function (cell) {
                var td = makeNode('td', ''); appendInline(td, cell); tr2.appendChild(td);
            });
            tbody.appendChild(tr2);
        }
        table.appendChild(tbody); container.appendChild(table);
        inTable = false; tableRows = []; tableAlign = [];
    }
    function flushCode() {
        if (!inCode) { return; }
        var codeText = codeLines.join('\n');
        var wrapper = makeNode('div', 'code-block-wrapper');
        if (codeLang) {
            var header = makeNode('div', 'code-block-header');
            header.appendChild(makeNode('span', 'code-lang-label', codeLang));
            wrapper.appendChild(header);
        }
        var pre = makeNode('pre', ''), code = makeNode('code', '', codeText);
        pre.appendChild(code); wrapper.appendChild(pre);
        var copyBtn = makeNode('button', 'code-copy-btn', '📋 複製');
        (function (ct) {
            copyBtn.onclick = function (e) {
                e.stopPropagation();
                navigator.clipboard.writeText(ct).then(function () {
                    copyBtn.textContent = '✓ 已複製'; copyBtn.classList.add('copied');
                    setTimeout(function () { copyBtn.textContent = '📋 複製'; copyBtn.classList.remove('copied'); }, 2000);
                }).catch(function () {
                    copyBtn.textContent = '⚠ 失敗';
                    setTimeout(function () { copyBtn.textContent = '📋 複製'; }, 1500);
                });
            };
        })(codeText);
        if (codeLang) {
            var hdr = wrapper.querySelector('.code-block-header');
            if (hdr) hdr.appendChild(copyBtn);
            else wrapper.appendChild(copyBtn);
        } else {
            wrapper.appendChild(copyBtn);
        }
        container.appendChild(wrapper);
        inCode = false; codeLines = []; codeLang = '';
    }
    lines.forEach(function (line) {
        var trimmed = line.trim();
        if (trimmed.indexOf(codeFence) === 0) {
            flushParagraph(); flushList();
            if (inCode) { flushCode(); } else {
                inCode = true; codeLines = [];
                var langPart = trimmed.slice(3).trim();
                if (langPart.length > 0 && langPart.length < 20) codeLang = langPart;
            }
            return;
        }
        if (inCode) { codeLines.push(line); return; }
        if (!trimmed) { flushParagraph(); flushList(); flushTable(); return; }
        // Table detection
        var isTableRow = false;
        if (trimmed.indexOf('|') === 0 || trimmed.lastIndexOf('|') === trimmed.length - 1) {
            isTableRow = true;
        } else if (inTable && trimmed.indexOf('|') >= 0) {
            isTableRow = true;
        }
        if (isTableRow) {
            var cells = trimmed.replace(/^\|/, '').replace(/\|$/, '').split('|').map(function (c) { return c.trim(); });
            var isSep = cells.every(function (c) { return /^:?-{3,}:?$/.test(c); });
            if (isSep) { /* separator row */ } else {
                flushParagraph(); flushList(); flushCode();
                if (!inTable) { inTable = true; tableRows = []; tableAlign = []; }
                tableRows.push(cells);
            }
            return;
        }
        if (inTable) { flushTable(); }
        // Headings
        var level = 0; while (trimmed.charAt(level) === '#') { level++; }
        if (level > 0 && level <= 6 && trimmed.charAt(level) === ' ') { flushParagraph(); flushList(); var heading = makeNode('h' + level, ''); appendInline(heading, trimmed.slice(level + 1)); container.appendChild(heading); return; }
        // Horizontal rule
        if (trimmed.length >= 3 && trimmed.split('').every(function (ch) { return ch === '-'; })) { flushParagraph(); flushList(); container.appendChild(makeNode('hr', '')); return; }
        // Blockquote
        if (trimmed.indexOf('> ') === 0) { flushParagraph(); flushList(); var quote = makeNode('blockquote', ''); appendInline(quote, trimmed.slice(2)); container.appendChild(quote); return; }
        // Lists
        var bullet = trimmed.indexOf('- ') === 0 || trimmed.indexOf('* ') === 0 || trimmed.indexOf('+ ') === 0;
        var dot = trimmed.indexOf('. '), ordered = dot > 0 && trimmed.slice(0, dot).split('').every(function (ch) { return ch >= '0' && ch <= '9'; });
        if (bullet || ordered) {
            flushParagraph(); var type = ordered ? 'ol' : 'ul'; if (!list || listType !== type) { flushList(); list = makeNode(type, ''); listType = type; }
            var item = makeNode('li', ''); appendInline(item, trimmed.slice(ordered ? dot + 2 : 2)); list.appendChild(item); return;
        }
        // Images
        var imgMatch = /!\[([^\]]*)\]\(([^)]+)\)/.exec(trimmed);
        if (imgMatch) {
            flushParagraph(); flushList();
            var altText = imgMatch[1] || 'image';
            var imgUrl = imgMatch[2] || '';
            var img = makeNode('img', '');
            img.src = imgUrl; img.alt = altText; img.loading = 'lazy';
            img.onerror = function () { this.style.display = 'none'; };
            container.appendChild(img);
            var afterImg = trimmed.substring(imgMatch.index + imgMatch[0].length).trim();
            if (afterImg) { paragraph.push(afterImg); }
            return;
        }
        // Skip line-numbered file content (e.g. "1 | [package]")
        if (/^\d+\s+\|/.test(trimmed) && !inTable) { return; }
        paragraph.push(trimmed);
    });
    flushParagraph(); flushList(); flushTable(); flushCode();
}