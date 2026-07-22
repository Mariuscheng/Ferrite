import * as cp from 'child_process';
import * as fs from 'fs';
import * as readline from 'readline';

interface JsonRpcRequest {
    jsonrpc: string;
    id: string;
    method: string;
    params: Record<string, unknown>;
}

interface JsonRpcResponse {
    jsonrpc?: string;
    id?: string;
    method?: string;
    params?: unknown;
    result?: unknown;
    error?: {
        code: number;
        message: string;
    };
}

export interface ToolEvent {
    type: 'toolStart' | 'toolOutput' | 'toolComplete';
    tool: string;
    command?: string;
    stream?: 'stdout' | 'stderr';
    output?: string;
    exitCode?: number;
    success?: boolean;
}

interface PendingRequest {
    resolve: (value: unknown) => void;
    reject: (reason: Error) => void;
    timer: NodeJS.Timeout;
}

/** Milliseconds between heartbeats to detect hung sidecar processes. */
const HEARTBEAT_MS = 10_000;
/** Maximum stderr buffer size in characters. */
const MAX_STDERR_LEN = 8000;

export class SidecarManager {
    private binaryPath: string;
    private process: cp.ChildProcess | null = null;
    private rl: readline.Interface | null = null;
    private pendingRequests = new Map<string, PendingRequest>();
    private requestId = 0;
    private running = false;
    private restartAttempts = 0;
    private maxRestarts = 3;
    private lastStderr = '';
    private toolEventListeners = new Set<(event: ToolEvent) => void>();
    private streamChunkListeners = new Set<(chunk: { content: string | null; done: boolean }) => void>();
    /** Timestamp of the last received line from the sidecar (for heartbeat). */
    private lastActivity = 0;
    private heartbeatTimer: NodeJS.Timeout | null = null;
    /** Track recent restart timestamps for exponential backoff. */
    private restartTimestamps: number[] = [];

    constructor(binaryPath: string) {
        this.binaryPath = binaryPath;
    }

    start(): void {
        if (this.running) {
            return;
        }

        try {
            if (!fs.existsSync(this.binaryPath)) {
                throw new Error(`Sidecar binary not found: ${this.binaryPath}. Run build to copy ai-agent into bin/.`);
            }

            this.process = cp.spawn(this.binaryPath, [], {
                stdio: ['pipe', 'pipe', 'pipe'],
                windowsHide: true,
            });

            this.lastStderr = '';
            this.lastActivity = Date.now();

            this.rl = readline.createInterface({
                input: this.process.stdout!,
                crlfDelay: Infinity,
            });

            this.rl.on('line', (line: string) => {
                this.lastActivity = Date.now();
                this.handleResponse(line);
            });

            this.process.stderr?.on('data', (data: Buffer) => {
                const chunk = data.toString();
                this.lastStderr = (this.lastStderr + chunk).slice(-MAX_STDERR_LEN);
                console.error('[sidecar]', chunk.trim());
            });

            this.process.on('error', (err: Error) => {
                console.error('Sidecar process error:', err.message);
                this.cleanup();
            });

            this.process.on('exit', (code: number | null, signal: string | null) => {
                console.log(`Sidecar exited with code=${code} signal=${signal}`);
                this.cleanup();
                this.attemptRestart();
            });

            this.running = true;
            this.restartAttempts = 0;
            this.restartTimestamps = [];
            this.startHeartbeat();
            console.log('Sidecar process started');
        } catch (err) {
            console.error('Failed to start sidecar:', err);
            this.running = false;
        }
    }

    stop(): void {
        this.stopHeartbeat();
        if (this.process && this.running) {
            this.sendRequest('shutdown', {}).catch(() => {
                // Ignore errors during shutdown
            }).finally(() => {
                setTimeout(() => {
                    this.process?.kill();
                    this.cleanup();
                }, 1000);
            });
        }
    }

    isRunning(): boolean {
        return this.running && this.process !== null && !this.process.killed;
    }

    onToolEvent(listener: (event: ToolEvent) => void): () => void {
        this.toolEventListeners.add(listener);
        return () => this.toolEventListeners.delete(listener);
    }

    onStreamChunk(listener: (chunk: { content: string | null; done: boolean }) => void): () => void {
        this.streamChunkListeners.add(listener);
        return () => this.streamChunkListeners.delete(listener);
    }

    async sendRequest(method: string, params: Record<string, unknown>): Promise<any> {
        if (!this.isRunning()) {
            throw new Error('Sidecar process is not running');
        }

        const id = (++this.requestId).toString();
        const request: JsonRpcRequest = {
            jsonrpc: '2.0',
            id,
            method,
            params,
        };

        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.pendingRequests.delete(id);
                reject(new Error(`Request ${method} timed out`));
            }, 300000); // 5 minute timeout for long operations

            this.pendingRequests.set(id, { resolve, reject, timer });

            const line = JSON.stringify(request) + '\n';
            this.process?.stdin?.write(line);
        });
    }

    // ----------------------------------------------------------------------
    // Heartbeat — logs a warning when the sidecar has been idle for an
    // extended period while requests are pending.  It does NOT kill the
    // process automatically; the per-request timeout (5 minutes) is the
    // authoritative guard against truly hung operations.
    // ----------------------------------------------------------------------
    private startHeartbeat(): void {
        this.stopHeartbeat();
        this.heartbeatTimer = setInterval(() => {
            if (!this.isRunning()) {
                return;
            }
            if (this.pendingRequests.size === 0) {
                return;
            }
            const idle = Date.now() - this.lastActivity;
            if (idle > 60_000) {
                console.warn(
                    `[sidecar] ${this.pendingRequests.size} request(s) pending for ${Math.round(idle / 1000)}s — waiting for AI response...`
                );
            }
        }, HEARTBEAT_MS);
    }

    private stopHeartbeat(): void {
        if (this.heartbeatTimer) {
            clearInterval(this.heartbeatTimer);
            this.heartbeatTimer = null;
        }
    }

    // ----------------------------------------------------------------------
    // Connection management
    // ----------------------------------------------------------------------
    private handleResponse(line: string): void {
        try {
            const trimmed = line.trim();
            if (!trimmed) {
                return;
            }

            const response: JsonRpcResponse = JSON.parse(trimmed);

            if (response.method === 'toolEvent' && isToolEvent(response.params)) {
                for (const listener of this.toolEventListeners) {
                    try {
                        listener(response.params);
                    } catch (error) {
                        console.error('Tool event listener failed:', error);
                    }
                }
                return;
            }

            if (response.method === 'streamChunk') {
                const params = response.params as { content?: string | null; done?: boolean } | undefined;
                if (params) {
                    for (const listener of this.streamChunkListeners) {
                        try {
                            listener({
                                content: params.content ?? null,
                                done: params.done ?? false,
                            });
                        } catch (error) {
                            console.error('Stream chunk listener failed:', error);
                        }
                    }
                }
                return;
            }

            if (response.id && this.pendingRequests.has(response.id)) {
                const pending = this.pendingRequests.get(response.id)!;
                clearTimeout(pending.timer);
                this.pendingRequests.delete(response.id);

                if (response.error) {
                    pending.reject(
                        new Error(response.error.message || 'Unknown RPC error')
                    );
                } else {
                    pending.resolve(response.result);
                }
            }
        } catch {
            // Ignore non-JSON lines
        }
    }

    private cleanup(): void {
        this.running = false;
        this.stopHeartbeat();
        this.rl?.close();
        this.rl = null;
        this.process = null;

        // Reject all pending requests
        for (const [id, pending] of this.pendingRequests) {
            clearTimeout(pending.timer);
            const detail = this.lastStderr.trim();
            const msg = detail
                ? `Sidecar process terminated. Details: ${detail}`
                : 'Sidecar process terminated';
            pending.reject(new Error(msg));
        }
        this.pendingRequests.clear();
    }

    // ----------------------------------------------------------------------
    // Exponential backoff restart
    // ----------------------------------------------------------------------
    private attemptRestart(): void {
        if (this.restartAttempts >= this.maxRestarts) {
            console.error('Maximum restart attempts reached. Giving up.');
            return;
        }

        this.restartAttempts++;
        const now = Date.now();
        this.restartTimestamps.push(now);

        // Keep only recent timestamps (last minute) to track restart frequency.
        this.restartTimestamps = this.restartTimestamps.filter(t => now - t < 60_000);

        // If we've restarted more than twice in the last minute, give up.
        if (this.restartTimestamps.length > 2) {
            console.error('Too many restart attempts in a short window. Giving up.');
            return;
        }

        // Exponential backoff: base 1s, doubling each attempt, max 30s.
        const delay = Math.min(1000 * Math.pow(2, this.restartAttempts - 1), 30_000);
        console.log(
            `Attempting restart ${this.restartAttempts}/${this.maxRestarts} in ${Math.round(delay / 1000)}s`
        );
        setTimeout(() => this.start(), delay);
    }
}

function isToolEvent(value: unknown): value is ToolEvent {
    if (!value || typeof value !== 'object') {
        return false;
    }

    const event = value as Record<string, unknown>;
    return typeof event.type === 'string'
        && typeof event.tool === 'string'
        && ['toolStart', 'toolOutput', 'toolComplete'].includes(event.type);
}