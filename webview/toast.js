// ── Toast Notification System ──────────────────────────────────────────────
var toastContainer = document.getElementById('toastContainer');

function showToast(message, type, duration) {
    type = type || 'info';
    duration = duration || 3500;
    var toast = document.createElement('div');
    toast.className = 'toast ' + type;
    var icon = type === 'success' ? '\u2713 ' : type === 'error' ? '\u2717 ' : '\u2139 ';
    toast.innerHTML = '<span>' + icon + '</span><span>' + message + '</span><span class="toast-close">\u00D7</span>';
    toast.querySelector('.toast-close').onclick = function () { toast.remove(); };
    if (toastContainer) { toastContainer.appendChild(toast); }
    setTimeout(function () { if (toast.parentNode) toast.remove(); }, duration);
}