import * as path from 'path';
import * as fs from 'fs';
import * as vscode from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;
let statusBarItem: vscode.StatusBarItem;

export function activate(context: vscode.ExtensionContext) {
    statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusBarItem.text = '$(sync~spin) Arandu';
    statusBarItem.tooltip = 'Arandu Language Server: Starting...';
    statusBarItem.command = 'arandu.showServerLogs';
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    // Command to show server logs
    const showLogsCommand = vscode.commands.registerCommand('arandu.showServerLogs', () => {
        if (client) {
            client.outputChannel.show();
        }
    });
    context.subscriptions.push(showLogsCommand);

    // Command to restart the server
    const restartCommand = vscode.commands.registerCommand('arandu.restartServer', async () => {
        if (client) {
            vscode.window.showInformationMessage('Restarting Arandu Language Server...');
            await client.stop();
            client = undefined;
        }
        startLanguageServer(context);
    });
    context.subscriptions.push(restartCommand);

    startLanguageServer(context);
}

function startLanguageServer(context: vscode.ExtensionContext) {
    statusBarItem.text = '$(sync~spin) Arandu';
    statusBarItem.tooltip = 'Arandu Language Server: Starting...';

    const serverPath = findServerPath(context);
    if (!serverPath) {
        vscode.window.showErrorMessage(
            'Could not find the "arandu-lsp" executable. Make sure to compile it (cargo build -p arandu_lsp) or configure the path in settings ("arandu.server.path").'
        );
        statusBarItem.text = '$(error) Arandu';
        statusBarItem.tooltip = 'Arandu Language Server: Executable not found';
        return;
    }

    const traceOutputChannel = vscode.window.createOutputChannel('Arandu Language Server Trace', { log: true });

    // If it is an absolute path or a command in PATH
    const serverOptions: ServerOptions = {
        run: { command: serverPath, transport: TransportKind.stdio },
        debug: { command: serverPath, transport: TransportKind.stdio }
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: 'file', language: 'arandu' }],
        synchronize: {
            // Notify the server about changes in '.aru' files
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.aru')
        },
        traceOutputChannel
    };

    client = new LanguageClient(
        'aranduLanguageServer',
        'Arandu Language Server',
        serverOptions,
        clientOptions
    );

    // Start the client. This will also start the server.
    client.start().then(() => {
        vscode.window.showInformationMessage('Arandu Language Server successfully activated.');
        statusBarItem.text = '$(check) Arandu';
        statusBarItem.tooltip = 'Arandu Language Server: Running';
    }).catch((err: unknown) => {
        vscode.window.showErrorMessage(`Failed to start Arandu Language Server: ${err}`);
        statusBarItem.text = '$(error) Arandu';
        statusBarItem.tooltip = `Arandu Language Server: Error — ${err}`;
    });
}

export function deactivate(): Thenable<void> | undefined {
    if (!client) {
        return undefined;
    }
    return client.stop();
}

function findServerPath(context: vscode.ExtensionContext): string | undefined {
    const config = vscode.workspace.getConfiguration('arandu');
    const customPath = config.get<string | null>('server.path');

    if (customPath) {
        if (fs.existsSync(customPath)) {
            return customPath;
        }
        // If it is just the binary name in PATH
        return customPath;
    }

    // Try to find in the target/debug or target/release folder of the workspace
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (workspaceFolders) {
        for (const folder of workspaceFolders) {
            const rootPath = folder.uri.fsPath;
            const debugPath = path.join(rootPath, 'target', 'debug', 'arandu-lsp');
            if (fs.existsSync(debugPath)) {
                return debugPath;
            }
            const releasePath = path.join(rootPath, 'target', 'release', 'arandu-lsp');
            if (fs.existsSync(releasePath)) {
                return releasePath;
            }
        }
    }

    // Also try to find by going up two directories from the extension path (if running within the Arandu Lang repository)
    const extensionRepoRoot = path.join(context.extensionPath, '..', '..');
    const debugPath = path.join(extensionRepoRoot, 'target', 'debug', 'arandu-lsp');
    if (fs.existsSync(debugPath)) {
        return debugPath;
    }
    const releasePath = path.join(extensionRepoRoot, 'target', 'release', 'arandu-lsp');
    if (fs.existsSync(releasePath)) {
        return releasePath;
    }

    // Also try to find by going up three directories from __dirname of the compiled script (highly robust for local dev testing)
    const scriptRepoRoot = path.join(__dirname, '..', '..', '..');
    const scriptDebugPath = path.join(scriptRepoRoot, 'target', 'debug', 'arandu-lsp');
    if (fs.existsSync(scriptDebugPath)) {
        return scriptDebugPath;
    }
    const scriptReleasePath = path.join(scriptRepoRoot, 'target', 'release', 'arandu-lsp');
    if (fs.existsSync(scriptReleasePath)) {
        return scriptReleasePath;
    }

    // Fallback: assume it is in the global PATH
    return 'arandu-lsp';
}
