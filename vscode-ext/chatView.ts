import * as vscode from 'vscode';
import * as path from 'path';
import { SidecarManager, ToolEvent } from './sidecar';
import { ChatMsg } from './constants';
import { getWebviewHtml } from './webview/content';

// ── In-memory diff content provider ─────────────────────────────────────────
// Lets the extension open VS Code's native diff editor with the result of an
// apply_diff dry-run preview: old (current on-disk file) vs new (previewed).
// ----------------------------------------------------------------------------

export class DiffContentProvider implements vscode.TextDocumentContentProvider {
    private _contents = new Map<string, string>();

    static scheme = 'ferrite-diff';

    onDidChangeEmitter = new vscode.EventEmitter<vscode.Uri>();
    onDidChange = this.onDidChangeEmitter.event;

    async provideTextDocumentContent(uri: vscode.Uri): Promise<string> {
        return this._contents.get(uri.toString()) ?? '';
    }

    setContent(uri: vscode.Uri, content: string): void {
        this._contents.set(uri.toString(), content);
        this.onDidChangeEmitter.fire(uri);
    }
}

let diffContentProvider: DiffContentProvider | undefined;

/**
 * Register the in-memory diff content provider with VS Code.
 * Called once during extension activation.
 */
export function registerDiffContentProvider(
    context: vscode.ExtensionContext
): DiffContentProvider {
    const provider = new DiffContentProvider();
    diffContentProvider = provider;
    context.subscriptions.push(
        vscode.workspace.registerTextDocumentContentProvider(
            DiffContentProvider.scheme,
            provider
        )
    );
    return provider;
}

export class ChatViewProvider implements vscode.WebviewViewProvider {
    private _view?: vscode.WebviewView;
    private _extensionUri: vscode.Uri;
    private _sidecar: SidecarManager;
    private _sessionId: string | null = null;
    private _latestPlan: any = null;
    private _latestDiffPreview: any = null;
    private _msgListenerDisposable: vscode.Disposable | null = null;

    constructor(extensionUri: vscode.Uri, sidecar: SidecarManager) {
        this._extensionUri = extensionUri;
        this._sidecar = sidecar;
        this._sidecar.onToolEvent((event: ToolEvent) => {
            this._post({ type: 'toolEvent', event });
        });
        this._sidecar.onStreamChunk((chunk: { content: string | null; done: boolean }) => {
            this._post({ type: 'streamChunk', chunk });
        });
    }

    public postMessage(message: any): void { if (this._view) this._view.webview.postMessage(message); }

    public resolveWebviewView(
        webviewView: vscode.WebviewView, _ctx: vscode.WebviewViewResolveContext,
        _t: vscode.CancellationToken
    ): void {
        const previousView = this._view;
        this._view = webviewView;
        (webviewView as any).retainContextWhenHidden = true;
        webviewView.webview.options = { enableScripts: true, localResourceRoots: [this._extensionUri] };

        // Only (re)initialize the webview when it's actually a *new* instance
        // (VS Code disposes & recreates the webview on certain lifecycle events).
        const isNewInstance = (webviewView !== previousView);

        if (isNewInstance) {
            // Dispose the old listener first to prevent duplicate handlers.
            if (this._msgListenerDisposable) {
                this._msgListenerDisposable.dispose();
                this._msgListenerDisposable = null;
            }

            webviewView.webview.html = getWebviewHtml(this._extensionUri, webviewView.webview.cspSource);

            this._msgListenerDisposable = webviewView.webview.onDidReceiveMessage(
                async (d: any) => {
                    switch (d.type) {
                        case 'ready':
                            await this._listSessions();
                            // If we already have an active session, restore its messages
                            // and terminal state into the fresh webview.
                            if (this._sessionId) {
                                await this._restoreSessionMessages();
                            }
                            break;
                        case 'sendMessage': await this._chat(d.message, d.agentConfig); break;
                        case 'stopGeneration': this._stopGeneration(); break;
                        case 'newSession': await this._newSession(); break;
                        case 'switchSession': await this._switchSession(d.sessionId); break;
                        case 'deleteSession': await this._deleteSession(d.sessionId); break;
                        case 'renameSession': await this._renameSession(d.sessionId, d.title); break;
                        case 'getConfig': await this._sendConfig(); break;
                        case 'saveConfig': await this._saveConfig(d.config); break;
                        case 'generatePlan': await this._generatePlan(d.goal); break;
                        case 'applyPlan': await this._applyPlan(Boolean(d.preview)); break;
                        case 'runValidation': await this._runValidation(); break;
                        case 'saveTerminalState': await this._saveTerminalState(d.state); break;
                        case 'applyDiffFromPreview': await this._applyDiffFromPreview(d.patch, d.fuzz); break;
                        case 'openDiffEditor': await this._openDiffEditor(d.file, d.previewData); break;
                    }
                },
            );
        }
    }

    /**
     * Open the VS Code native diff editor for a file in the current preview.
     * Uses an in-memory TextDocumentContentProvider for the "new" side so the
     * user can inspect the previewed changes with full syntax highlighting.
     */
    private async _openDiffEditor(relPath: string, previewData?: any) {
        const wsRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '.';
        try {
            const originalUri = vscode.Uri.file(path.join(wsRoot, relPath));
            const doc = await vscode.workspace.openTextDocument(originalUri);
            const originalContent = doc.getText();

            // The "new" (preview) side is served from an in-memory provider.
            const newUri = vscode.Uri.parse(`${DiffContentProvider.scheme}:/${relPath}`);
            if (diffContentProvider) {
                diffContentProvider.setContent(newUri, originalContent); // placeholder; replaced below
            }

            // Use the preview data passed from the webview to build the "new" side.
            // Fall back to the last stored preview from _applyDiffFromPreview.
            const preview = (previewData && Array.isArray(previewData.previews))
                ? previewData
                : this._latestDiffPreview;
            if (preview && Array.isArray(preview.previews)) {
                const filePreview = preview.previews.find((p: any) => p.file === relPath);
                if (filePreview && filePreview.hunks_preview) {
                    const newContent = this._applyPreviewHunks(originalContent, filePreview.hunks_preview);
                    diffContentProvider!.setContent(newUri, newContent);
                }
            }

            await vscode.commands.executeCommand(
                'vscode.diff',
                doc.uri, // left: current on-disk file (original)
                newUri,  // right: in-memory previewed content
                `${relPath} — Ferrite Diff 預覽`
            );
        } catch (e: any) {
            vscode.window.showErrorMessage('無法開啟 Diff 預覽: ' + (e.message || '未知錯誤'));
        }
    }

    /**
     * Apply hunk previews (old/new text) to the original content to produce
     * the "new" file content for the native diff editor.
     */
    private _applyPreviewHunks(original: string, hunks: any[]): string {
        let result = original;
        // Apply hunks from the end to the start so line offsets don't shift.
        const applied = hunks
            .map((h, idx) => ({ h, idx }))
            .filter(({ h }) => h && h.applied && h.old_text !== '');
        for (const { h, idx } of applied.reverse()) {
            const oldText = String(h.old_text || '');
            const newText = String(h.new_text || '');
            const at = result.lastIndexOf(oldText);
            if (at >= 0) {
                result = result.slice(0, at) + newText + result.slice(at + oldText.length);
            }
        }
        return result;
    }

    private async _restoreSessionMessages(): Promise<void> {
        if (!this._sessionId) return;
        try {
            const r = await this._sidecar.sendRequest('getSessionMessages', { sessionId: this._sessionId });
            this._post({ type: 'clearMessages' });
            const msgs = (r.messages || []) as ChatMsg[];
            for (const m of msgs) {
                if (m.role === 'system' || m.role === 'tool') { continue; }
                const content = String(m.content || '').trim();
                if (m.role === 'assistant' && (content.startsWith('[Tool Call:') || content.length === 0)) { continue; }
                if (m.role === 'user' && content.startsWith('[Tool result:')) { continue; }
                this._post({ type: 'message', message: m });
            }
            if (r.terminalState) {
                this._post({ type: 'terminalState', state: r.terminalState });
            }
        } catch (e: any) {
            console.error('Failed to restore session messages:', e);
        }
    }

    private _stopGeneration(): void {
        this._post({ type: 'loading', isLoading: false });
        this._sidecar.sendRequest('stopGeneration', {}).catch(() => {});
    }

    private async _listSessions() {
        try {
            const r = await this._sidecar.sendRequest('listSessions', {});
            this._post({ type: 'sessionsList', sessions: r.sessions || [], activeSessionId: this._sessionId });
        } catch (e: any) { this._post({ type: 'error', message: e.message }); }
    }

    private async _newSession() {
        try {
            const ws = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '.';
            const r = await this._sidecar.sendRequest('initialize', { workspaceRoot: ws });
            this._sessionId = r.sessionId;
            this._post({ type: 'clearMessages' });
            this._post({ type: 'sessionCreated', sessionId: this._sessionId });
            await this._listSessions();
        } catch (e: any) { this._post({ type: 'error', message: '建立失敗: ' + e.message }); }
    }

    private async _switchSession(sid: string) {
        this._sessionId = sid;
        this._post({ type: 'sessionSwitched', sessionId: sid });
        try {
            const r = await this._sidecar.sendRequest('getSessionMessages', { sessionId: sid });
            this._post({ type: 'clearMessages' });
            for (const m of (r.messages || []) as ChatMsg[]) {
                if (m.role === 'system') { continue; }
                if (m.role === 'tool') { continue; }
                const content = String(m.content || '').trim();
                if (m.role === 'assistant' && (content.startsWith('[Tool Call:') || content.length === 0)) { continue; }
                if (m.role === 'user' && content.startsWith('[Tool result:')) { continue; }
                this._post({ type: 'message', message: m });
            }
            if (r.terminalState) {
                this._post({ type: 'terminalState', state: r.terminalState });
            }
        } catch (e: any) { this._post({ type: 'error', message: e.message }); }
        await this._listSessions();
    }

    private async _deleteSession(sid: string) {
        try {
            await this._sidecar.sendRequest('removeSession', { sessionId: sid });
            if (this._sessionId === sid) { this._sessionId = null; this._post({ type: 'sessionDeleted', sessionId: sid }); this._post({ type: 'clearMessages' }); }
            await this._listSessions();
        } catch (e: any) { this._post({ type: 'error', message: e.message }); }
    }

    private async _renameSession(sid: string, title: string) {
        try {
            const r = await this._sidecar.sendRequest('renameSession', { sessionId: sid, title });
            this._post({ type: 'sessionRenamed', sessionId: sid, title: title, updated: r.updated });
        } catch (e: any) { this._post({ type: 'error', message: e.message }); }
    }

    private async _chat(msg: string, agentConfig?: any) {
        if (!this._sessionId) { await this._newSession(); }
        if (!this._sessionId) { this._post({ type: 'error', message: '無法建立對話' }); return; }
        const originalSessionId = this._sessionId;
        this._post({ type: 'message', message: { role: 'user', content: msg } });
        this._post({ type: 'loading', isLoading: true });
        try {
            const r = await this._chatWithStreamRecovery(msg);
            if (r.sessionId && originalSessionId !== r.sessionId) {
                this._sessionId = r.sessionId;
                this._post({ type: 'sessionRecovered', sessionId: this._sessionId });
                await this._listSessions();
            }
            if (r.content && r.content !== '[系統] 未取得回應') {
                this._post({ type: 'streamChunk', chunk: { content: null, done: true } });
            }
        } catch (e: any) { this._post({ type: 'error', message: '錯誤: ' + e.message }); }
        finally { this._post({ type: 'loading', isLoading: false }); }
    }

    private async _chatWithStreamRecovery(msg: string): Promise<any> {
        const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '.';
        const ideContext = this._collectIdeContext();
        try {
            return await this._sidecar.sendRequest('chatStream', {
                sessionId: this._sessionId,
                message: msg,
                workspaceRoot,
                ideContext,
            });
        } catch (e: any) {
            const text = String(e?.message || '');
            if (text.includes('Session') && text.includes('not found')) {
                const r = await this._sidecar.sendRequest('initialize', { workspaceRoot });
                this._sessionId = r.sessionId;
                return await this._sidecar.sendRequest('chatStream', {
                    sessionId: this._sessionId,
                    message: msg,
                    workspaceRoot,
                    ideContext,
                });
            }
            throw e;
        }
    }

    private _collectIdeContext(): any {
        const editor = vscode.window.activeTextEditor;
        const doc = editor?.document;
        const selection = editor?.selection;
        const activeFile = doc ? doc.uri.fsPath : null;
        const selectedText = (doc && selection)
            ? doc.getText(selection).slice(0, 8000)
            : '';
        const tabs = vscode.window.tabGroups.all
            .flatMap((g) => g.tabs)
            .map((t: any) => {
                const input = t.input as any;
                const uri = input?.uri || input?.modified;
                return uri?.fsPath || t.label;
            })
            .filter(Boolean)
            .slice(0, 50);

        const diagnostics = vscode.languages.getDiagnostics()
            .slice(0, 50)
            .flatMap(([uri, ds]) => ds.slice(0, 3).map((d) => ({
                file: uri.fsPath,
                message: d.message,
                severity: d.severity,
                start: {
                    line: d.range.start.line + 1,
                    character: d.range.start.character + 1,
                },
                end: {
                    line: d.range.end.line + 1,
                    character: d.range.end.character + 1,
                },
            })));

        const cfg = vscode.workspace.getConfiguration('ferrite');

        return {
            workspaceRoot: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '.',
            activeFile,
            cursor: selection ? {
                line: selection.active.line + 1,
                character: selection.active.character + 1,
            } : null,
            selection: selection ? {
                startLine: selection.start.line + 1,
                startCharacter: selection.start.character + 1,
                endLine: selection.end.line + 1,
                endCharacter: selection.end.character + 1,
                selectedText,
            } : null,
            openTabs: tabs,
            diagnostics,
            config: {
                provider: cfg.get<string>('provider', 'deepseek'),
                model: cfg.get<string>('model', 'deepseek-chat'),
                endpoint: cfg.get<string>('endpoint', 'https://api.deepseek.com/v1'),
                reasoning: cfg.get<boolean>('reasoning', false),
            },
        };
    }

    private async _generatePlan(goal: string) {
        if (!this._sessionId) {
            await this._newSession();
        }
        if (!this._sessionId) {
            this._post({ type: 'error', message: '無法建立對話' });
            return;
        }

        const ideContext = this._collectIdeContext();
        this._post({ type: 'loading', isLoading: true });
        try {
            const r = await this._sidecar.sendRequest('generateEditPlan', {
                sessionId: this._sessionId,
                goal,
                ideContext,
            });
            this._latestPlan = r.plan || null;
            this._post({ type: 'planData', plan: this._latestPlan });
        } catch (e: any) {
            this._post({ type: 'error', message: '生成方案失敗: ' + e.message });
        } finally {
            this._post({ type: 'loading', isLoading: false });
        }
    }

    private async _applyPlan(preview: boolean) {
        if (!this._latestPlan || !Array.isArray(this._latestPlan.edits)) {
            this._post({ type: 'error', message: '尚未有可套用的修改方案' });
            return;
        }

        const wsRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '.';
        const edits = this._latestPlan.edits as Array<any>;
        const applied: Array<any> = [];
        const skipped: Array<any> = [];

        const workspaceEdit = new vscode.WorkspaceEdit();

        for (const e of edits) {
            const relPath = String(e.path || '').trim();
            const search = String(e.search || '');
            const replace = String(e.replace || '');

            if (!relPath || !search) {
                skipped.push({ path: relPath || '(empty)', reason: 'path/search 缺失' });
                continue;
            }

            const uri = vscode.Uri.file(path.join(wsRoot, relPath));
            try {
                const doc = await vscode.workspace.openTextDocument(uri);
                const text = doc.getText();
                const idx = text.indexOf(search);
                if (idx < 0) {
                    skipped.push({ path: relPath, reason: '找不到 search 片段' });
                    continue;
                }

                const beforeSnippet = text.slice(Math.max(0, idx - 80), Math.min(text.length, idx + search.length + 80));
                const afterSnippet = text.slice(Math.max(0, idx - 80), idx) + replace + text.slice(idx + search.length, Math.min(text.length, idx + search.length + 80));

                const start = doc.positionAt(idx);
                const end = doc.positionAt(idx + search.length);
                workspaceEdit.replace(uri, new vscode.Range(start, end), replace);
                applied.push({
                    path: relPath,
                    reason: e.reason || '',
                    before: beforeSnippet,
                    after: afterSnippet,
                });
            } catch (err: any) {
                skipped.push({ path: relPath, reason: err?.message || '無法開啟檔案' });
            }
        }

        if (!preview) {
            await vscode.workspace.applyEdit(workspaceEdit);
            await vscode.commands.executeCommand('workbench.action.files.saveAll');
        }

        this._post({
            type: 'applyResult',
            preview,
            applied,
            skipped,
        });
    }

    private async _runValidation() {
        const wsRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '.';
        const cmds = Array.isArray(this._latestPlan?.validationCommands)
            ? this._latestPlan.validationCommands
            : ['npm run compile', 'cargo build --release'];

        this._post({ type: 'toolEventReset' });
        this._post({ type: 'loading', isLoading: true });
        try {
            const r = await this._sidecar.sendRequest('runValidation', {
                workspaceRoot: wsRoot,
                commands: cmds,
            });
            this._post({ type: 'validationResult', result: r });
        } catch (e: any) {
            this._post({ type: 'error', message: '驗證失敗: ' + e.message });
        } finally {
            this._post({ type: 'loading', isLoading: false });
        }
    }

    private async _sendConfig() {
        try {
            const r = await this._sidecar.sendRequest('getConfig', {});
            this._post({ type: 'configData', config: {
                provider: r.provider || 'deepseek',
                apiKeyConfigured: (r.apiKeyConfigured === undefined) ? false : r.apiKeyConfigured,
                model: r.model || 'deepseek-chat',
                endpoint: r.endpoint || 'https://api.deepseek.com/v1',
                temperature: r.temperature ?? 0.3, timeoutSeconds: r.timeoutSeconds ?? 120,
                reasoning: r.reasoning ?? false, reasoningEffort: r.reasoningEffort || 'high',
                shell: r.shell || '',
                maxToolIterations: r.maxToolIterations ?? 25,
            }});
        } catch (e: any) { this._post({ type: 'configData', config: null, error: e.message }); }
    }

    private async _saveConfig(c: any) {
        try {
            const params: any = { ...c };
            const apiKey = typeof params.apiKey === 'string' ? params.apiKey.trim() : '';
            const isMaskedPlaceholder = apiKey === '' || apiKey === '••••••••' || apiKey.includes('•') || apiKey.includes('●');
            if (!apiKey || isMaskedPlaceholder) {
                delete params.apiKey;
            }
            await this._sidecar.sendRequest('updateConfig', params);
            this._post({ type: 'configSaved', success: true, message: '設定已儲存' });
        } catch (e: any) { this._post({ type: 'configSaved', success: false, message: e.message }); }
    }

    private async _applyDiffFromPreview(patch: string, fuzz: number) {
        const wsRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '.';
        this._post({ type: 'loading', isLoading: true });
        try {
            const preview = await this._sidecar.sendRequest('applyDiff', {
                workspaceRoot: wsRoot,
                patch,
                dryRun: true,
                fuzz: fuzz || 3,
            });
            const parsedPreview = typeof preview === 'string' ? JSON.parse(preview) : preview;
            this._latestDiffPreview = parsedPreview;
            this._post({
                type: 'diffPreview',
                result: parsedPreview,
                message: 'Diff 預覽：' + (parsedPreview.files || 0) + ' 個檔案，' + (parsedPreview.applicableHunks || 0) + '/' + (parsedPreview.totalHunks || 0) + ' 個區塊可套用',
            });

            const applied = await this._sidecar.sendRequest('applyDiff', {
                workspaceRoot: wsRoot,
                patch,
                dryRun: false,
                fuzz: fuzz || 3,
            });
            const parsedApplied = typeof applied === 'string' ? JSON.parse(applied) : applied;
            this._post({
                type: 'diffApplied',
                success: true,
                message: 'Diff 已套用：' + (parsedApplied.filesApplied || 0) + ' 個檔案成功',
                result: parsedApplied,
            });
        } catch (e: any) {
            this._post({
                type: 'diffApplied',
                success: false,
                message: 'Diff 套用失敗: ' + (e.message || '未知錯誤'),
            });
        } finally {
            this._post({ type: 'loading', isLoading: false });
        }
    }

    private async _saveTerminalState(state: string) {
        if (!this._sessionId) { return; }
        try {
            await this._sidecar.sendRequest('saveTerminalState', {
                sessionId: this._sessionId,
                state,
            });
        } catch (e: any) {
            console.error('Failed to save terminal state:', e);
        }
    }

    private _post(d: any) { if (this._view) this._view.webview.postMessage(d); }
}