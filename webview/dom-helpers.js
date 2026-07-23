// ── DOM Helpers ────────────────────────────────────────────────────────────

/**
 * Add an event listener safely.
 * @param {Element|null} el
 * @param {string} event
 * @param {Function} handler
 */
function on(el, event, handler) {
    if (el && el.addEventListener) el.addEventListener(event, handler);
}

/**
 * Create a DOM element with optional class and optional text.
 * @param {string} tag
 * @param {string} [className]
 * @param {string} [text]
 * @returns {HTMLElement}
 */
function makeNode(tag, className, text) {
    var el = document.createElement(tag);
    if (className) { el.className = className; }
    if (text !== undefined && text !== null) { el.textContent = String(text); }
    return el;
}

/**
 * Remove all child nodes of an element.
 * @param {Element} el
 */
function clearNode(el) {
    while (el && el.firstChild) { el.removeChild(el.firstChild); }
}