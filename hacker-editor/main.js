// main.js - Electron main process
const { app, BrowserWindow, Menu } = require('electron');
const path = require('path');

function createWindow() {
  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    webPreferences: {
      nodeIntegration: true,
      contextIsolation: false,
      enableRemoteModule: true
    }
  });

  win.loadFile('index.html');

  // Add menu
  const menu = Menu.buildFromTemplate([
    {
      label: 'File',
      submenu: [
        { label: 'Open', click: () => win.webContents.executeJavaScript('openFile()') },
        { label: 'Save', click: () => win.webContents.executeJavaScript('saveFile()') },
        { label: 'Save As...', click: () => win.webContents.executeJavaScript('saveAsFile()') },
        { type: 'separator' },
        { role: 'quit' }
      ]
    },
    {
      label: 'Actions',
      submenu: [
        { label: 'Run', click: () => win.webContents.executeJavaScript('runScript()') },
        { label: 'Compile', click: () => win.webContents.executeJavaScript('compileScript()') },
        { label: 'Check Syntax', click: () => win.webContents.executeJavaScript('checkSyntax()') },
        { label: 'Init Template', click: () => win.webContents.executeJavaScript('initTemplate()') },
        { label: 'Clean Temps', click: () => win.webContents.executeJavaScript('cleanTemps()') },
        { label: 'Launch REPL', click: () => win.webContents.executeJavaScript('launchRepl()') }
      ]
    },
    {
      label: 'View',
      submenu: [
        { role: 'reload' },
        { role: 'forceReload' },
        { type: 'separator' },
        { role: 'toggleDevTools' }
      ]
    },
    {
      label: 'Help',
      submenu: [
        { label: 'About', click: () => win.webContents.executeJavaScript('showAbout()') }
      ]
    }
  ]);
  Menu.setApplicationMenu(menu);
}

app.whenReady().then(createWindow);

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    createWindow();
  }
});
