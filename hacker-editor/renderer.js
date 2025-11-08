require.config({ paths: { vs: 'https://cdnjs.cloudflare.com/ajax/libs/monaco-editor/0.47.0/min/vs' } });
require(['vs/editor/editor.main'], function () {
    // Register Hacker Lang
    monaco.languages.register({ id: 'hackerlang' });
    // Monarch tokenizer
    monaco.languages.setMonarchTokensProvider('hackerlang', {
        tokenizer: {
            root: [
                [/^\/\/.*/, 'dependency'],
                [/^#.*/, 'library'],
                [/^@[\w-]+=.*/, 'variable'],
                [/^>.*/, 'command'],
                [/^=\d+\s*>.*/, 'loop'],
                [/^\?\s*.+\s*>.*/, 'conditional'],
                [/^&.*/, 'background'],
                [/^!.*/, 'comment'],
                [/^\[.*\]$/, 'config-section'],
                [/^[\w-]+=.*$/, 'config-key']
            ]
        }
    });
    // Theme
    monaco.editor.defineTheme('hackerTheme', {
        base: 'vs-dark',
        inherit: true,
        rules: [
            { token: 'dependency', foreground: '4ec9b0' }, // Greenish
            { token: 'library', foreground: '569cd6' }, // Blue
            { token: 'variable', foreground: 'c586c0' }, // Purple
            { token: 'command', foreground: 'd4d4d4' }, // Light gray
            { token: 'loop', foreground: 'dcdcaa' }, // Yellow
            { token: 'conditional', foreground: '9cdcfe' },// Light blue
            { token: 'background', foreground: 'ce9178' }, // Orange
            { token: 'comment', foreground: '6a9955' }, // Green comment
            { token: 'config-section', foreground: 'd7ba7d', fontStyle: 'bold' },
            { token: 'config-key', foreground: 'ce9178' }
        ],
        colors: {
            'editor.background': '#1e1e1e',
            'editor.foreground': '#d4d4d4',
            'editorLineNumber.foreground': '#858585',
            'editorCursor.foreground': '#ffffff',
            'editor.selectionBackground': '#264f78'
        }
    });
    // Create editor
    window.editor = monaco.editor.create(document.getElementById('editor'), {
        value: '// Welcome to Hacker Lang IDE\n// Start coding here...\n',
        language: 'hackerlang',
        theme: 'hackerTheme',
        automaticLayout: true,
        minimap: { enabled: true },
        scrollBeyondLastLine: false,
        fontSize: 14,
        lineNumbers: 'on',
        roundedSelection: false,
        padding: { top: 10 }
    });
    // Update status bar on cursor change
    editor.onDidChangeCursorPosition(e => {
        document.getElementById('status-info').innerText = `Ln ${e.position.lineNumber}, Col ${e.position.column} | Hacker Lang`;
    });

    const HACKERC_BIN = '/usr/bin/hackerc';
    let currentFile = null;
    let rootDir = null;
    const openFiles = new Map(); // filePath => {model, tab, isUntitled: boolean}
    const outputDiv = document.getElementById('output');
    const statusMessage = document.getElementById('status-message');
    const sidebar = document.getElementById('sidebar');
    const tabs = document.getElementById('tabs');
    let untitledCount = 1;

    function updateStatus(msg) {
        statusMessage.innerText = msg;
    }
    function appendOutput(text, color = '#d4d4d4') {
        const span = document.createElement('span');
        span.style.color = color;
        span.innerText = text + '\n';
        outputDiv.appendChild(span);
        outputDiv.scrollTop = outputDiv.scrollHeight;
    }
    function runCommand(cmd, verbose = false) {
        if (verbose) cmd += ' --verbose';
        appendOutput(`Executing: ${cmd}`, '#569cd6');
        updateStatus('Running...');
        ipcRenderer.send('exec-command', cmd);
    }
    function buildFileTree(tree) {
        sidebar.innerHTML = '';
        const rootUl = document.createElement('ul');
        sidebar.appendChild(rootUl);
        createTree(tree, rootUl);
    }
    function createTree(node, parentEl) {
        const li = document.createElement('li');
        li.textContent = node.name;
        parentEl.appendChild(li);
        if (node.children) {
            li.style.fontWeight = 'bold';
            const ul = document.createElement('ul');
            li.appendChild(ul);
            node.children.forEach(child => createTree(child, ul));
        } else {
            li.addEventListener('click', () => openFileInEditor(node.fullPath));
        }
    }
    async function openFileInEditor(filePath) {
        if (openFiles.has(filePath)) {
            switchToFile(filePath);
            return;
        }
        try {
            const data = await ipcRenderer.invoke('read-file', filePath);
            const model = monaco.editor.createModel(data, 'hackerlang');
            const tab = createTab(filePath, false);
            openFiles.set(filePath, { model, tab, isUntitled: false });
            switchToFile(filePath);
            appendOutput(`Opened: ${filePath}`, '#4ec9b0');
        } catch (err) {
            appendOutput(`Error opening: ${err.message}`, '#f44747');
        }
    }
    function createNewFile() {
        const filePath = `Untitled-${untitledCount++}`;
        const model = monaco.editor.createModel('', 'hackerlang');
        const tab = createTab(filePath, true);
        openFiles.set(filePath, { model, tab, isUntitled: true });
        switchToFile(filePath);
        appendOutput(`New file: ${filePath}`, '#4ec9b0');
    }
    function createTab(filePath, isUntitled) {
        const tab = document.createElement('div');
        tab.classList.add('tab');
        const filenameSpan = document.createElement('span');
        filenameSpan.classList.add('filename');
        filenameSpan.textContent = isUntitled ? filePath : path.basename(filePath);
        const closeSpan = document.createElement('span');
        closeSpan.classList.add('close');
        closeSpan.textContent = '×';
        tab.appendChild(filenameSpan);
        tab.appendChild(closeSpan);
        tab.addEventListener('click', () => switchToFile(filePath));
        closeSpan.addEventListener('click', (e) => {
            e.stopPropagation();
            closeTab(filePath);
        });
        tabs.appendChild(tab);
        return tab;
    }
    function switchToFile(filePath) {
        const { model, tab } = openFiles.get(filePath);
        editor.setModel(model);
        currentFile = filePath;
        document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
        tab.classList.add('active');
        updateStatus('File switched');
    }
    function closeTab(filePath) {
        const { model, tab } = openFiles.get(filePath);
        tabs.removeChild(tab);
        model.dispose();
        openFiles.delete(filePath);
        if (currentFile === filePath) {
            currentFile = null;
            if (openFiles.size > 0) {
                const nextFile = Array.from(openFiles.keys())[0];
                switchToFile(nextFile);
            } else {
                editor.setModel(null);
            }
        }
    }
    // Functions
    async function openFile() {
        try {
            const result = await ipcRenderer.invoke('show-open-dialog', {
                properties: ['openFile'],
                filters: [{ name: 'Hacker Files', extensions: ['hacker'] }]
            });
            if (!result.canceled) {
                const filePath = result.filePaths[0];
                await openFileInEditor(filePath);
            }
        } catch (err) {
            appendOutput(`Dialog error: ${err.message}`, '#f44747');
        }
    }
    async function openFolder() {
        try {
            const { dir, tree } = await ipcRenderer.invoke('open-folder');
            if (dir) {
                rootDir = dir;
                buildFileTree(tree);
                appendOutput(`Opened folder: ${rootDir}`, '#4ec9b0');
            }
        } catch (err) {
            appendOutput(`Dialog error: ${err.message}`, '#f44747');
        }
    }
    async function saveFile() {
        if (!editor.getModel()) {
            appendOutput('No file open to save.', '#f44747');
            return;
        }
        if (!currentFile) {
            await saveAsFile();
            return;
        }
        const entry = openFiles.get(currentFile);
        if (entry.isUntitled) {
            await saveAsFile();
            return;
        }
        let content = editor.getValue();
        if (content === undefined) {
            content = '';
        }
        try {
            await ipcRenderer.invoke('write-file', currentFile, content);
            appendOutput(`Saved: ${currentFile}`, '#4ec9b0');
            updateStatus('Saved');
        } catch (err) {
            appendOutput(`Error saving: ${err.message}`, '#f44747');
        }
    }
    async function saveAsFile() {
        if (!editor.getModel()) {
            appendOutput('No file open to save.', '#f44747');
            return;
        }
        try {
            const result = await ipcRenderer.invoke('show-save-dialog', {
                filters: [{ name: 'Hacker Files', extensions: ['hacker'] }]
            });
            if (!result.canceled) {
                const newFilePath = result.filePath;
                let content = editor.getValue();
                if (content === undefined) {
                    content = '';
                }
                await ipcRenderer.invoke('write-file', newFilePath, content);
                appendOutput(`Saved as: ${newFilePath}`, '#4ec9b0');
                updateStatus('Saved');
                // Update tab if untitled
                const entry = openFiles.get(currentFile);
                if (entry && entry.isUntitled) {
                    closeTab(currentFile);
                    await openFileInEditor(newFilePath);
                } else {
                    currentFile = newFilePath;
                }
            }
        } catch (err) {
            appendOutput(`Dialog error: ${err.message}`, '#f44747');
        }
    }
    function runScript() {
        if (!currentFile) {
            appendOutput('No file open to run.', '#f44747');
            return;
        }
        const entry = openFiles.get(currentFile);
        if (entry.isUntitled) {
            appendOutput('Save the file first to run.', '#f44747');
            return;
        }
        runCommand(`${HACKERC_BIN} run ${currentFile}`);
    }
    async function compileScript() {
        if (!currentFile) {
            appendOutput('No file open to compile.', '#f44747');
            return;
        }
        const entry = openFiles.get(currentFile);
        if (entry.isUntitled) {
            appendOutput('Save the file first to compile.', '#f44747');
            return;
        }
        try {
            const result = await ipcRenderer.invoke('show-save-dialog', {
                title: 'Compile to',
                defaultPath: currentFile.replace('.hacker', '')
            });
            if (!result.canceled) {
                const outputPath = result.filePath;
                runCommand(`${HACKERC_BIN} compile ${currentFile} -o ${outputPath}`);
            }
        } catch (err) {
            appendOutput(`Dialog error: ${err.message}`, '#f44747');
        }
    }
    function checkSyntax() {
        if (!currentFile) {
            appendOutput('No file open to check.', '#f44747');
            return;
        }
        const entry = openFiles.get(currentFile);
        if (entry.isUntitled) {
            appendOutput('Save the file first to check.', '#f44747');
            return;
        }
        runCommand(`${HACKERC_BIN} check ${currentFile}`);
    }
    async function initTemplate() {
        try {
            const result = await ipcRenderer.invoke('show-save-dialog', {
                filters: [{ name: 'Hacker Files', extensions: ['hacker'] }]
            });
            if (!result.canceled) {
                const filePath = result.filePath;
                runCommand(`${HACKERC_BIN} init ${filePath}`);
                setTimeout(async () => {
                    try {
                        const data = await ipcRenderer.invoke('read-file', filePath);
                        openFileInEditor(filePath);
                        appendOutput(`Initialized and loaded: ${filePath}`, '#4ec9b0');
                        updateStatus('Template initialized');
                    } catch (err) {
                        appendOutput(`Error loading initialized file: ${err.message}`, '#f44747');
                    }
                }, 1000);
            }
        } catch (err) {
            appendOutput(`Dialog error: ${err.message}`, '#f44747');
        }
    }
    function cleanTemps() {
        runCommand(`${HACKERC_BIN} clean`);
    }
    function launchRepl() {
        ipcRenderer.send('launch-repl');
    }
    function showAbout() {
        ipcRenderer.send('show-about');
    }
    // IPC listeners
    ipcRenderer.on('exec-result', (event, {err, stdout, stderr}) => {
        if (err) {
            appendOutput(`Error: ${stderr || err.message}`, '#f44747');
        } else {
            appendOutput(stdout, '#d4d4d4');
        }
        updateStatus('Ready');
    });
    ipcRenderer.on('repl-result', (event, {err, message}) => {
        if (err) {
            appendOutput(`Error launching REPL: ${message}\n(Ensure xterm is installed)`, '#f44747');
        } else {
            appendOutput('REPL launched in new terminal.', '#4ec9b0');
        }
    });
    // Button event listeners
    document.getElementById('new').addEventListener('click', createNewFile);
    document.getElementById('open').addEventListener('click', openFile);
    document.getElementById('open-folder').addEventListener('click', openFolder);
    document.getElementById('save').addEventListener('click', saveFile);
    document.getElementById('run').addEventListener('click', runScript);
    document.getElementById('compile').addEventListener('click', compileScript);
    document.getElementById('check').addEventListener('click', checkSyntax);
    document.getElementById('init').addEventListener('click', initTemplate);
    document.getElementById('clean').addEventListener('click', cleanTemps);
    document.getElementById('repl').addEventListener('click', launchRepl);
    // IPC listeners for menu
    ipcRenderer.on('new-file', createNewFile);
    ipcRenderer.on('open-file', openFile);
    ipcRenderer.on('open-folder', openFolder);
    ipcRenderer.on('save-file', saveFile);
    ipcRenderer.on('save-as-file', saveAsFile);
    ipcRenderer.on('run-script', runScript);
    ipcRenderer.on('compile-script', compileScript);
    ipcRenderer.on('check-syntax', checkSyntax);
    ipcRenderer.on('init-template', initTemplate);
    ipcRenderer.on('clean-temps', cleanTemps);
    ipcRenderer.on('launch-repl', launchRepl);
    ipcRenderer.on('show-about', showAbout);
    // Signal ready
    ipcRenderer.send('renderer-ready');
});
