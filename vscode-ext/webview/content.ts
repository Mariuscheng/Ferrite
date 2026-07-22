import * as vscode from 'vscode';
import * as fs from 'fs';
import { MODEL_LISTS, ENDPOINT_PRESETS } from '../constants';

export function getNonce(): string {
    let t = '';
    const p = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    for (let i = 0; i < 64; i++) t += p.charAt(Math.floor(Math.random() * p.length));
    return t;
}

export function getWebviewHtml(extensionUri: vscode.Uri, cspSource: string): string {
    const nonce = getNonce();

    const webviewDir = vscode.Uri.joinPath(extensionUri, 'webview');
    const css = readFileSafely(vscode.Uri.joinPath(webviewDir, 'styles.css').fsPath);
    // Module order matters — dependencies must load first
    const modules = [
        readFileSafely(vscode.Uri.joinPath(webviewDir, 'toast.js').fsPath),
        readFileSafely(vscode.Uri.joinPath(webviewDir, 'dom-helpers.js').fsPath),
        readFileSafely(vscode.Uri.joinPath(webviewDir, 'markdown.js').fsPath),
        readFileSafely(vscode.Uri.joinPath(webviewDir, 'plan-ui.js').fsPath),
        readFileSafely(vscode.Uri.joinPath(webviewDir, 'session-ui.js').fsPath),
        readFileSafely(vscode.Uri.joinPath(webviewDir, 'stream.js').fsPath),
        readFileSafely(vscode.Uri.joinPath(webviewDir, 'client.js').fsPath),
    ];
    const js = modules.join('\n');

    const ml = JSON.stringify(MODEL_LISTS);
    const ep = JSON.stringify(ENDPOINT_PRESETS);

    return `<!DOCTYPE html><html lang="zh-TW"><head><meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${cspSource} data:; style-src ${cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}';">
<meta name="viewport" content="width=device-width,initial-scale=1.0"><title>AI Code Agent</title>
<style nonce="${nonce}">${css}</style></head><body>
<!-- Toast container -->
<div class="toast-container" id="toastContainer" aria-live="polite"></div>

<div class="header">
    <div class="header-brand">
        <span class="header-title"><span class="brand-icon">🤖</span>AI Code Agent</span>
        <span class="header-subtitle" id="workspaceLabel">工作區已連線</span>
    </div>
    <div class="header-actions">
        <button class="icon-btn" id="workBtn" type="button" title="變更工作區">⚡ 工作</button>
        <button class="icon-btn" id="settingsBtn" type="button" title="設定">⚙ 設定</button>
    </div>
</div>

<div class="settings-panel" id="settingsPanel">
    <div class="panel-heading"><div><h3>模型與連線設定</h3><p>設定會套用到新的請求</p></div></div>
    <div class="settings-card">
        <div class="settings-card-title">連線</div>
        <div class="form-group"><label>Provider</label><select id="cfgP"><option value="deepseek">DeepSeek</option><option value="openai">OpenAI</option><option value="anthropic">Anthropic</option><option value="ollama">Ollama</option></select></div>
        <div class="form-group"><label>API Key <span id="keyStatus" class="key-status no">未設定</span></label><input type="password" id="cfgK" placeholder="sk-..." /></div>
        <div class="form-group"><label>Endpoint</label><input type="text" id="cfgE" /></div>
        <div class="form-group"><label>Model</label><select id="cfgM"></select></div>
    </div>
    <div class="settings-card">
        <div class="settings-card-title">生成參數</div>
        <div class="form-group"><label>Temperature (<span id="tLab">0.3</span>)</label><div class="range-row"><input type="range" id="cfgT" min="0" max="2" step="0.1" value="0.3" /><span id="tVal">0.3</span></div></div>
        <div class="form-group"><label>Timeout (秒)</label><input type="number" id="cfgTo" min="10" max="600" value="120" /></div>
        <div class="form-group"><div class="toggle-row"><label>🔍 思考模式</label><label class="toggle"><input type="checkbox" id="cfgR" /><span class="slider"></span></label></div></div>
        <div class="form-group" id="reasoningEffortGroup" style="display:none;">
            <label>思考強度</label>
            <select id="cfgRE">
                <option value="low">Low → high</option>
                <option value="medium">Medium → high</option>
                <option value="high" selected>High</option>
                <option value="xhigh">XHigh → max</option>
            </select>
        </div>
        <div class="form-group">
            <label>Shell 命令模板 (<code>{cmd}</code> = 實際指令, 留空自動偵測)</label>
            <input type="text" id="cfgShell" placeholder="cmd /C {cmd}" />
        </div>
    </div>
    <button class="s-btn" id="saveBtn">💾 儲存設定</button>
    <div class="save-msg" id="saveMsg"></div>
</div>

<div class="work-panel" id="workPanel">
    <div class="panel-heading"><div><h3>變更工作區</h3><p id="workStatus">尚未生成方案</p></div></div>
    <div class="work-actions">
        <button class="btn" id="genPlanBtn" type="button">📋 規劃</button>
        <button class="btn" id="previewPlanBtn" type="button" disabled>👁 預覽</button>
        <button class="btn" id="applyPlanBtn" type="button" disabled>✅ 套用</button>
        <button class="btn" id="validateBtn" type="button">🧪 驗證</button>
    </div>
    <div class="plan-box" id="planBox"><div class="work-empty">在輸入框輸入需求後按「規劃」，會使用目前檔案、選取範圍與診斷資訊建立修改方案。</div></div>
    <div class="terminal-box" id="terminalBox" hidden>
        <div class="terminal-heading"><span>📺 執行輸出</span><button class="terminal-clear" id="clearTerminalBtn" type="button" title="清除終端輸出">清除</button></div>
        <pre class="terminal-output" id="terminalOutput" aria-live="polite"></pre>
    </div>
</div>

<div class="main-area">
    <div class="sidebar" id="sidebar">
        <div class="sidebar-section">💬 對話</div>
        <div id="sessionList"><div class="no-sessions">尚無對話</div></div>
        <button class="btn" id="newSessionBtn" style="margin-top:6px;width:100%;justify-content:center;">➕ 新增對話</button>
    </div>
    <div class="chat-area">
        <div class="messages" id="msgCont">
            <div class="welcome" id="welcome">
                <div class="welcome-icon">🤖</div>
                <h2 class="welcome-title">AI Code Agent</h2>
                <p class="welcome-desc">選取程式碼後提問，或直接描述你的需求</p>
                <div class="welcome-chips">
                    <button class="welcome-chip" data-prompt="請解釋這段程式碼的功能">💡 解釋程式碼</button>
                    <button class="welcome-chip" data-prompt="請幫我修復以下錯誤：">🐛 修復錯誤</button>
                    <button class="welcome-chip" data-prompt="請為這段程式碼撰寫單元測試">🧪 產生測試</button>
                    <button class="welcome-chip" data-prompt="請重構這段程式碼，提升可讀性">🔧 重構程式碼</button>
                    <button class="welcome-chip" data-prompt="請分析這個專案的架構">📐 專案分析</button>
                    <button class="welcome-chip" data-prompt="請撰寫這段程式碼的文件註解">📝 撰寫文件</button>
                </div>
            </div>
            <div class="loading" id="ldInd" style="display:none;"><div class="spinner"></div>AI 思考中...</div>
        </div>
        <div class="input-area"><textarea id="msgInp" placeholder="描述你要處理的工作... (Enter 發送，Shift+Enter 換行)" rows="2"></textarea><div class="composer-actions"><input type="file" id="imgInput" accept="image/*" hidden /><button class="plan-trigger" id="imgBtn" type="button" title="附加圖片">🖼</button><button class="plan-trigger" id="planFromInputBtn" type="button" title="依目前輸入建立修改方案">📋 規劃</button><button class="send-btn" id="sndBtn" type="button">➤ 發送</button></div></div>
    </div>
</div>
<div class="status-bar"><span class="status-dot on" id="sDot"></span><span id="sTxt">AI 引擎就緒</span></div>

<script nonce="${nonce}">
var MODEL_LISTS = ${ml};
var ENDPOINT_PRESETS = ${ep};
${js}
</script></body></html>`;
}

function readFileSafely(path: string): string {
    try {
        return fs.readFileSync(path, 'utf-8');
    } catch {
        console.error(`Failed to read file: ${path}`);
        return '';
    }
}