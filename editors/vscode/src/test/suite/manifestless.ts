import * as assert from 'node:assert/strict';
import * as fs from 'node:fs';
import * as vscode from 'vscode';

const TIMEOUT_MS = 30_000;

interface AranduExtensionApi {
    getRuntimeState(): { state: string; observedCrashCount: number };
}

export async function run(): Promise<void> {
    const serverPath = process.env.ARANDU_LSP_TEST_PATH;
    assert.ok(serverPath, 'ARANDU_LSP_TEST_PATH must point to the test server');
    assert.ok(fs.existsSync(serverPath), `arandu-lsp test binary missing: ${serverPath}`);

    const configuration = vscode.workspace.getConfiguration('arandu');
    await configuration.update('server.path', serverPath, vscode.ConfigurationTarget.Global);
    try {
        const workspace = vscode.workspace.workspaceFolders?.[0];
        assert.ok(workspace, 'manifestless Extension Host test requires a workspace folder');
        assert.equal(
            await vscode.workspace.fs.stat(vscode.Uri.joinPath(workspace.uri, 'Arandu.toml'))
                .then(() => true, () => false),
            false,
            'manifestless fixture must not contain Arandu.toml'
        );
        assert.equal(
            await vscode.workspace.fs.stat(vscode.Uri.joinPath(workspace.uri, 'arandu.toml'))
                .then(() => true, () => false),
            false,
            'manifestless fixture must not contain arandu.toml'
        );

        const extension = vscode.extensions.getExtension('arandu.arandu-lang');
        assert.ok(extension, 'Arandu extension was not installed in the Extension Host');
        await extension.activate();
        const api = extension.exports as AranduExtensionApi;
        await poll(() => api.getRuntimeState().state === 'single-file' ? true : undefined);

        const uri = vscode.Uri.joinPath(workspace.uri, 'main.aru');
        const diagnosticsChanged = waitForDiagnosticsChange(uri);
        const document = await vscode.workspace.openTextDocument(uri);
        await vscode.window.showTextDocument(document);
        const diagnostics = await diagnosticsChanged;
        assert.deepEqual(
            diagnostics,
            [],
            `valid standalone source importing the toolchain stdlib must not produce false diagnostics: ${uri.fsPath}`
        );

        const diagnosticUri = vscode.Uri.joinPath(workspace.uri, 'diagnostic-label.aru');
        await vscode.workspace.fs.writeFile(
            diagnosticUri,
            new TextEncoder().encode('func main(): void {}\nss\n')
        );
        const labeledDiagnosticsChanged = waitForDiagnosticsChange(diagnosticUri);
        const diagnosticDocument = await vscode.workspace.openTextDocument(diagnosticUri);
        await vscode.window.showTextDocument(diagnosticDocument);
        const labeledDiagnostics = await labeledDiagnosticsChanged;
        assert.ok(
            labeledDiagnostics.some(diagnostic =>
                (diagnostic.code === 'P001'
                    || (typeof diagnostic.code === 'object' && diagnostic.code.value === 'P001'))
                && diagnostic.message.includes('unexpected `ss`')),
            `VS Code must receive the parser's primary label in the displayed diagnostic message: ${JSON.stringify(labeledDiagnostics)}`
        );
        await vscode.workspace.fs.delete(diagnosticUri);
    } finally {
        await configuration.update('server.path', undefined, vscode.ConfigurationTarget.Global);
        await vscode.commands.executeCommand('workbench.action.closeAllEditors');
    }
}

function waitForDiagnosticsChange(uri: vscode.Uri): Promise<readonly vscode.Diagnostic[]> {
    return new Promise((resolve, reject) => {
        const timeout = setTimeout(() => {
            subscription.dispose();
            reject(new Error(`Timed out waiting for diagnostics for ${uri.fsPath}`));
        }, TIMEOUT_MS);
        const subscription = vscode.languages.onDidChangeDiagnostics(event => {
            if (!event.uris.some(changed => changed.toString() === uri.toString())) {
                return;
            }
            clearTimeout(timeout);
            subscription.dispose();
            resolve(vscode.languages.getDiagnostics(uri));
        });
    });
}

async function poll<T>(read: () => T | undefined | Promise<T | undefined>): Promise<T> {
    const deadline = Date.now() + TIMEOUT_MS;
    while (Date.now() < deadline) {
        const value = await read();
        if (value !== undefined) {
            return value;
        }
        await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error('Timed out waiting for the manifestless Arandu language server');
}
