import React, { useState, useEffect, useRef } from 'react';
import Editor from '@monaco-editor/react';
import { PanelGroup, Panel, PanelResizeHandle } from 'react-resizable-panels';
import styled from 'styled-components';
import * as monaco from 'monaco-editor';
import { editor } from 'monaco-editor';

const Container = styled.div`
  display: flex;
  flex-direction: column;
  height: 100vh;
  background-color: #F8F9FA;
  color: #212529;
`;

const Toolbar = styled.div`
  display: flex;
  background: #DEE2E6;
  border-bottom: 1px solid #CED4DA;
  padding: 5px;
  gap: 10px;
`;

const Button = styled.button`
  background: #FFFFFF;
  border: 1px solid #CED4DA;
  border-radius: 4px;
  padding: 5px 10px;
  color: #212529;
  cursor: pointer;
  &:hover {
    background: #E9ECEF;
  }
`;

const ConsoleArea = styled.textarea`
  background-color: #F8F9FA;
  color: #212529;
  border: none;
  padding: 10px;
  font-family: monospace;
  font-size: 10pt;
  resize: none;
`;

const StatusBar = styled.div`
  background: #DEE2E6;
  border-top: 1px solid #CED4DA;
  padding: 5px;
  text-align: left;
`;

declare global {
  interface Window {
    electronAPI: {
      openFile: () => Promise<{ filePath: string; content: string } | null>;
      saveFile: (content: string) => Promise<string | null>;
      readFile: (filePath: string) => Promise<string>;
      writeFile: (filePath: string, content: string) => Promise<void>;
      runScript: (filePath: string) => Promise<string>;
      onNewFile: (callback: () => void) => void;
      onOpenFile: (callback: () => void) => void;
      onSaveFile: (callback: () => void) => void;
      onSaveAsFile: (callback: () => void) => void;
      onRunScript: (callback: () => void) => void;
      onClearConsole: (callback: () => void) => void;
      onToggleConsole: (callback: () => void) => void;
      onShowAbout: (callback: () => void) => void;
    };
  }
}

function registerHackerLanguage() {
  monaco.languages.register({ id: 'hacker' });

  monaco.languages.setMonarchTokensProvider('hacker', {
    tokenizer: {
      root: [
        [/^\/\/.*/, { token: 'keyword', next: '@rest' }],
        [/^#.*/, { token: 'keyword', next: '@rest' }],
        [/^@.*/, { token: 'variable', next: '@rest' }],
        [/^>.*/, { token: 'string', next: '@rest' }],
        [/^=.*/, { token: 'operator', next: '@rest' }],
        [/^\?.*/, { token: 'type', next: '@rest' }],
        [/^&.*/, { token: 'type', next: '@rest' }],
        [/^!.*/, { token: 'comment', next: '@rest' }],
        [/^\[.*/, { token: 'annotation', next: '@rest' }],
        [/^\].*/, { token: 'annotation', next: '@rest' }],
        [/\$\w+/, 'variable'],
        [/!.*$/, 'comment'],
      ],
      rest: [
        [/$/, 'root', '@pop'],
      ],
    },
    ignoreCase: false,
  });

  monaco.editor.defineTheme('hackerTheme', {
    base: 'vs',
    inherit: true,
    rules: [
      { token: 'keyword', foreground: '007BFF', fontStyle: 'bold' },
      { token: 'variable', foreground: '6F42C1' },
      { token: 'string', foreground: '28A745' },
      { token: 'comment', foreground: 'DC3545', fontStyle: 'italic' },
      { token: 'annotation', foreground: '17A2B8' },
      { token: 'operator', foreground: 'FD7E14' },
      { token: 'type', foreground: '6F42C1' },
    ],
    colors: {},
  });
}

const App: React.FC = () => {
  const [content, setContent] = useState('');
  const [currentFile, setCurrentFile] = useState<string | null>(null);
  const [isModified, setIsModified] = useState(false);
  const [consoleOutput, setConsoleOutput] = useState('');
  const [showConsole, setShowConsole] = useState(true);
  const [statusMessage, setStatusMessage] = useState('');
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);

  useEffect(() => {
    registerHackerLanguage();

    window.electronAPI.onNewFile(newFile);
    window.electronAPI.onOpenFile(openFile);
    window.electronAPI.onSaveFile(saveFile);
    window.electronAPI.onSaveAsFile(saveAsFile);
    window.electronAPI.onRunScript(runScript);
    window.electronAPI.onClearConsole(clearConsole);
    window.electronAPI.onToggleConsole(toggleConsole);
    window.electronAPI.onShowAbout(showAbout);

    return () => {
      // Clean up listeners if needed
    };
  }, []);

  const handleEditorDidMount = (editor: editor.IStandaloneCodeEditor) => {
    editorRef.current = editor;
    editor.onDidChangeModelContent(() => {
      setIsModified(true);
    });
  };

  const maybeSave = async (): Promise<boolean> => {
    if (!isModified) return true;
    const response = confirm('The document has been modified. Do you want to save your changes?');
    if (response) {
      await saveFile();
      return !isModified;
    }
    return confirm('Discard changes?');
  };

  const newFile = async () => {
    if (await maybeSave()) {
      setContent('');
      setCurrentFile(null);
      setIsModified(false);
      setStatusMessage('New file created');
    }
  };

  const openFile = async () => {
    if (await maybeSave()) {
      const result = await window.electronAPI.openFile();
      if (result) {
        setContent(result.content);
        setCurrentFile(result.filePath);
        setIsModified(false);
        setStatusMessage(`Opened ${result.filePath}`);
      }
    }
  };

  const saveFile = async () => {
    if (currentFile) {
      await window.electronAPI.writeFile(currentFile, content);
      setIsModified(false);
      setStatusMessage(`Saved ${currentFile}`);
    } else {
      await saveAsFile();
    }
  };

  const saveAsFile = async () => {
    const filePath = await window.electronAPI.saveFile(content);
    if (filePath) {
      setCurrentFile(filePath);
      setIsModified(false);
      setStatusMessage(`Saved ${filePath}`);
    }
  };

  const runScript = async () => {
    if (!currentFile) {
      if (await maybeSave()) {
        await saveAsFile();
      }
      if (!currentFile) return;
    }
    setConsoleOutput((prev) => prev + `Running script: ${currentFile}\n`);
    try {
      const output = await window.electronAPI.runScript(currentFile);
      setConsoleOutput((prev) => prev + output + '\nExecution completed successfully!\n');
      setStatusMessage('Run successful');
    } catch (error) {
      setConsoleOutput((prev) => prev + `${error}\nExecution failed.\n`);
      setStatusMessage('Run failed');
      alert(`Run Error: ${error}`);
    }
  };

  const clearConsole = () => {
    setConsoleOutput('');
    setStatusMessage('Console cleared');
  };

  const toggleConsole = () => {
    setShowConsole(!showConsole);
  };

  const showAbout = () => {
    alert('Hacker Editor v0.0.7 - Advanced IDLE for .hacker language.\nBuilt with Electron, React, TypeScript, and Monaco Editor.\nFeatures: Syntax highlighting, console output, toolbar, and more.');
  };

  const handleEditorChange = (value: string | undefined) => {
    setContent(value || '');
    setIsModified(true);
  };

  return (
    <Container>
      <Toolbar>
        <Button onClick={newFile}>New</Button>
        <Button onClick={openFile}>Open</Button>
        <Button onClick={saveFile}>Save</Button>
        <Button onClick={runScript}>Run</Button>
      </Toolbar>
      <PanelGroup direction="vertical">
        <Panel defaultSize={70}>
          <Editor
            height="100%"
            defaultLanguage="hacker"
            theme="hackerTheme"
            value={content}
            onChange={handleEditorChange}
            onMount={handleEditorDidMount}
            options={{
              fontFamily: 'monospace',
              fontSize: 11,
            }}
          />
        </Panel>
        <PanelResizeHandle />
        {showConsole && (
          <Panel defaultSize={30}>
            <ConsoleArea value={consoleOutput} readOnly />
          </Panel>
        )}
      </PanelGroup>
      <StatusBar>{statusMessage}</StatusBar>
    </Container>
  );
};

export default App;
