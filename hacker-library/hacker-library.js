#!/usr/bin/env node

// hacker-library.js - Library manager for Hacker Lang.
// Located at ~/.hacker-lang/bin/hacker-library.
// Handles install (downloads libraries), update (checks updates), list (shows available/installed).

const fs = require('fs');
const path = require('path');
const fetch = require('node-fetch');

const args = process.argv.slice(2);
const action = args[0];

const libDir = path.join(process.env.HOME, '.hacker-lang', 'libs');

if (!fs.existsSync(libDir)) {
    fs.mkdirSync(libDir, { recursive: true });
}

const availableLibs = ['util', 'net', 'crypto'];

if (action === 'list') {
    console.log('Available libraries:');
    availableLibs.forEach(lib => console.log(`- ${lib}`));
    const installed = fs.existsSync(libDir) ? fs.readdirSync(libDir).filter(f => f.endsWith('.hacker')) : [];
    console.log('\nInstalled libraries:');
    installed.forEach(lib => console.log(`- ${lib.replace('.hacker', '')}`));
} else if (action === 'install') {
    const libname = args[1];
    if (!libname) {
        console.error('Usage: hacker-library install <libname>');
        process.exit(1);
    }
    if (!availableLibs.includes(libname)) {
        console.error(`Library ${libname} not found. Available: ${availableLibs.join(', ')}`);
        process.exit(1);
    }
    const url = `https://example.com/hacker-lang/${libname}.hacker`; // Placeholder
    const filePath = path.join(libDir, `${libname}.hacker`);
    console.log(`Installing ${libname} to ${filePath}...`);
    fetch(url)
        .then(res => {
            if (!res.ok) throw new Error(`HTTP ${res.status}`);
            const file = fs.createWriteStream(filePath);
            res.body.pipe(file);
            file.on('finish', () => {
                file.close();
                console.log(`Installed ${libname}`);
            });
        })
        .catch(err => {
            console.error(`Error installing ${libname}: ${err.message}`);
            process.exit(1);
        });
} else if (action === 'update') {
    console.log('Checking for library updates...');
    availableLibs.forEach(lib => {
        const filePath = path.join(libDir, `${lib}.hacker`);
        console.log(`Update check for ${lib}: ${fs.existsSync(filePath) ? 'Up to date (placeholder)' : 'Not installed'}`);
    });
} else {
    console.error('Usage: hacker-library [list|install|update] [libname]');
    process.exit(1);
}
