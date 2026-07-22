import * as vscode from 'vscode';
import * as path from 'path';
import * as os from 'os';
import { SidecarManager } from './sidecar';
import { ChatViewProvider } from './chatView';

let sidecarManager: SidecarManager | undefined;
let chatViewProvider: ChatViewProvider | undefined;

export function activate(context: vscode.ExtensionContext) {
    console.log('Ferrite extension activated');

    // Initialize the sidecar manager
    const binaryName = os.platform() === 'win32' ? 'ferrite-agent.exe' : 'ferrite-agent';
    const binaryPath = path.join(context.extensionPath, 'bin', binaryName);

    sidecarManager = new SidecarManager(binaryPath);
    sidecarManager.start();

    // Register the chat webview provider
    chatViewProvider = new ChatViewProvider(
        context.extensionUri,
        sidecarManager
    );

    context.subscriptions.push(
        vscode.window.registerWebviewViewProvider(
            'ferrite.chatView',
            chatViewProvider
        )
    );

    // Register commands
    context.subscriptions.push(
        vscode.commands.registerCommand('ferrite.openSettings', () => {
            vscode.commands.executeCommand(
                'workbench.action.openSettings',
                'ferrite'
            );
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('ferrite.newSession', async () => {
            const wsRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || '.';
            if (sidecarManager && sidecarManager.isRunning()) {
                const result = await sidecarManager.sendRequest('initialize', {
                    workspaceRoot: wsRoot,
                });
                if (chatViewProvider) {
                    chatViewProvider.postMessage({
                        type: 'sessionCreated',
                        sessionId: result.sessionId,
                    });
                }
                vscode.window.showInformationMessage(
                    '新對話已建立'
                );
            }
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('ferrite.clearSessions', async () => {
            if (sidecarManager && sidecarManager.isRunning()) {
                const result = await sidecarManager.sendRequest('listSessions', {});
                // sessions are { id: string; title: string }[] returned by the sidecar
                const sessions = result.sessions as Array<{ id: string; title: string }>;
                for (const session of sessions) {
                    await sidecarManager.sendRequest('removeSession', {
                        sessionId: session.id,
                    });
                }
                vscode.window.showInformationMessage(
                    `已清除 ${sessions.length} 個對話`
                );
            }
        })
    );

    // Watch configuration changes
    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration((e) => {
            if (e.affectsConfiguration('ferrite')) {
                syncConfigToSidecar();
            }
        })
    );

    // Initial config sync
    syncConfigToSidecar();

    function syncConfigToSidecar() {
        if (!sidecarManager || !sidecarManager.isRunning()) {
            return;
        }

        const config = vscode.workspace.getConfiguration('ferrite');
        const params: Record<string, unknown> = {
            provider: config.get<string>('provider', 'deepseek'),
            model: config.get<string>('model', 'deepseek-chat'),
            endpoint: config.get<string>('endpoint', 'https://api.deepseek.com'),
            temperature: config.get<number>('temperature', 0.3),
            timeoutSeconds: config.get<number>('timeoutSeconds', 120),
            reasoning: config.get<boolean>('reasoning', false),
        };
        // Only send apiKey if non-empty (preserve existing key)
        const apiKey = config.get<string>('apiKey', '');
        if (apiKey && apiKey.trim() !== '') {
            params.apiKey = apiKey;
        }

        sidecarManager.sendRequest('updateConfig', params).catch((err) => {
            console.error('Failed to sync config:', err);
        });
    }
}

export function deactivate() {
    if (sidecarManager) {
        sidecarManager.stop();
    }
}