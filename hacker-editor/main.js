const { app, BrowserWindow, Menu, ipcMain, dialog } = require('electron');
const fs = require('fs');
const path = require('path');
const { exec } = require('child_process');

const HACKERC_BIN = '/usr/bin/hackerc';

app.disableHardwareAcceleration();

async function createWindow() {
  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
                                contextIsolation: true,
                                nodeIntegration: false
    }
  });
  await win.loadFile('index.html');
  // Menu will be set after renderer-ready
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

// Handle unhandled rejections
process.on('unhandledRejection', (reason, promise) => {
  console.error('Unhandled Rejection at:', promise, 'reason:', reason);
});

// IPC handlers
ipcMain.handle('show-open-dialog', async (event, options) => {
  const win = BrowserWindow.fromWebContents(event.sender);
  return await dialog.showOpenDialog(win, options);
});

ipcMain.handle('show-save-dialog', async (event, options) => {
  const win = BrowserWindow.fromWebContents(event.sender);
  return await dialog.showSaveDialog(win, options);
});

ipcMain.handle('read-file', (event, filePath) => {
  return fs.readFileSync(filePath, 'utf8');
});

ipcMain.handle('write-file', (event, filePath, content) => {
  fs.writeFileSync(filePath, content);
});

ipcMain.on('exec-command', (event, cmd) => {
  exec(cmd, (err, stdout, stderr) => {
    event.sender.send('exec-result', {err, stdout, stderr});
  });
});

ipcMain.handle('open-folder', async () => {
  const result = await dialog.showOpenDialog({ properties: ['openDirectory'] });
  if (!result.canceled) {
    const dir = result.filePaths[0];
    const tree = buildTree(dir, dir);
    return { dir, tree };
  }
  return { dir: null, tree: null };
});

function buildTree(baseDir, fullPath) {
  const stats = fs.statSync(fullPath);
  const name = path.basename(fullPath);
  if (stats.isDirectory()) {
    const children = fs.readdirSync(fullPath).map(child => buildTree(baseDir, path.join(fullPath, child)));
    return { name, children, fullPath: path.relative(baseDir, fullPath) || '.' };
  } else {
    return { name, fullPath };
  }
}

ipcMain.on('launch-repl', (event) => {
  exec(`xterm -e ${HACKERC_BIN} repl`, (err) => {
    event.sender.send('repl-result', {err, message: err ? err.message : null});
  });
});

ipcMain.on('show-about', (event) => {
  const win = BrowserWindow.fromWebContents(event.sender);
  dialog.showMessageBox(win, {
    type: 'info',
    title: 'About Hacker Lang IDE',
    message: 'Hacker Lang IDE v1.0\nBuilt with Electron and Monaco Editor\nFor Linux Hackeros',
    buttons: ['OK']
  });
});

// Set menu when renderer is ready
ipcMain.on('renderer-ready', (event) => {
  const win = BrowserWindow.fromWebContents(event.sender);
  if (win) {
    const menu = Menu.buildFromTemplate([
      {
        label: 'File',
        submenu: [
          { label: 'New File', click: () => win.webContents.send('new-file') },
                                        { label: 'Open', click: () => win.webContents.send('open-file') },
                                        { label: 'Open Folder', click: () => win.webContents.send('open-folder') },
                                        { label: 'Save', click: () => win.webContents.send('save-file') },
                                        { label: 'Save As...', click: () => win.webContents.send('save-as-file') },
                                        { type: 'separator' },
                                        { role: 'quit' }
        ]
      },
      {
        label: 'Actions',
        submenu: [
          { label: 'Run', click: () => win.webContents.send('run-script') },
                                        { label: 'Compile', click: () => win.webContents.send('compile-script') },
                                        { label: 'Check Syntax', click: () => win.webContents.send('check-syntax') },
                                        { label: 'Init Template', click: () => win.webContents.send('init-template') },
                                        { label: 'Clean Temps', click: () => win.webContents.send('clean-temps') },
                                        { label: 'Launch REPL', click: () => win.webContents.send('launch-repl') }
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
          { label: 'About', click: () => win.webContents.send('show-about') }
        ]
      }
    ]);
    Menu.setApplicationMenu(menu);
  }
});
