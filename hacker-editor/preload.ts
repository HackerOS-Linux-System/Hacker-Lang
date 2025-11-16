import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('electronAPI', {
  openFile: () => ipcRenderer.invoke('dialog:openFile'),
  saveFile: (content) => ipcRenderer.invoke('dialog:saveFile', content),
  readFile: (filePath) => ipcRenderer.invoke('fs:readFile', filePath),
  writeFile: (filePath, content) => ipcRenderer.invoke('fs:writeFile', { filePath, content }),
  runScript: (filePath) => ipcRenderer.invoke('run:script', filePath),
  onNewFile: (callback) => ipcRenderer.on('new-file', callback),
  onOpenFile: (callback) => ipcRenderer.on('open-file', callback),
  onSaveFile: (callback) => ipcRenderer.on('save-file', callback),
  onSaveAsFile: (callback) => ipcRenderer.on('save-as-file', callback),
  onRunScript: (callback) => ipcRenderer.on('run-script', callback),
  onClearConsole: (callback) => ipcRenderer.on('clear-console', callback),
  onToggleConsole: (callback) => ipcRenderer.on('toggle-console', callback),
  onShowAbout: (callback) => ipcRenderer.on('show-about', callback),
});
