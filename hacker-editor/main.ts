import { app, BrowserWindow, Menu, dialog, shell, ipcMain } from 'electron';
import * as path from 'path';
import * as fs from 'fs';
import { exec } from 'child_process';

let mainWindow: BrowserWindow | null = null;

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1000,
    height: 700,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      nodeIntegration: false,
      contextIsolation: true,
    },
  });

  if (process.env.NODE_ENV === 'development') {
    mainWindow.loadURL('http://localhost:5173');
  } else {
    mainWindow.loadFile(path.join(__dirname, 'index.html'));
  }

  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

app.on('ready', () => {
  createWindow();

  const template: Electron.MenuItemConstructorOptions[] = [
    {
      label: 'File',
      submenu: [
        { label: 'New', accelerator: 'CmdOrCtrl+N', click: () => mainWindow?.webContents.send('new-file') },
        { label: 'Open', accelerator: 'CmdOrCtrl+O', click: () => mainWindow?.webContents.send('open-file') },
        { label: 'Save', accelerator: 'CmdOrCtrl+S', click: () => mainWindow?.webContents.send('save-file') },
        { label: 'Save As', accelerator: 'CmdOrCtrl+Shift+S', click: () => mainWindow?.webContents.send('save-as-file') },
        { type: 'separator' },
        { label: 'Exit', accelerator: 'CmdOrCtrl+Q', click: () => app.quit() },
      ],
    },
    {
      label: 'Run',
      submenu: [
        { label: 'Run Script', accelerator: 'CmdOrCtrl+R', click: () => mainWindow?.webContents.send('run-script') },
        { label: 'Clear Console', click: () => mainWindow?.webContents.send('clear-console') },
      ],
    },
    {
      label: 'View',
      submenu: [
        { label: 'Toggle Console', click: () => mainWindow?.webContents.send('toggle-console') },
      ],
    },
    {
      label: 'Help',
      submenu: [
        { label: 'About', click: () => mainWindow?.webContents.send('show-about') },
      ],
    },
  ];

  const menu = Menu.buildFromTemplate(template);
  Menu.setApplicationMenu(menu);
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('activate', () => {
  if (mainWindow === null) {
    createWindow();
  }
});

// IPC handlers for file operations
ipcMain.handle('dialog:openFile', async () => {
  const { canceled, filePaths } = await dialog.showOpenDialog({
    properties: ['openFile'],
    filters: [{ name: 'Hacker Files', extensions: ['hacker'] }],
  });
  if (canceled) return null;
  return { filePath: filePaths[0], content: fs.readFileSync(filePaths[0], 'utf-8') };
});

ipcMain.handle('dialog:saveFile', async (event, content) => {
  const { canceled, filePath } = await dialog.showSaveDialog({
    filters: [{ name: 'Hacker Files', extensions: ['hacker'] }],
  });
  if (canceled) return null;
  fs.writeFileSync(filePath, content);
  return filePath;
});

ipcMain.handle('fs:readFile', (event, filePath) => fs.readFileSync(filePath, 'utf-8'));

ipcMain.handle('fs:writeFile', (event, { filePath, content }) => {
  fs.writeFileSync(filePath, content);
});

ipcMain.handle('run:script', async (event, filePath) => {
  return new Promise((resolve, reject) => {
    exec(`/usr/bin/hackerc run "${filePath}"`, (error, stdout, stderr) => {
      if (error) {
        reject(error.message + '\n' + stderr);
      } else {
        resolve(stdout + stderr);
      }
    });
  });
});
