// hacker-library.js - Expanded library manager in JavaScript.
// Now supports 'list' (hardcoded libs) and 'install' (simulates download using fetch to a placeholder URL).
// Expand in future to real repos.
// #!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const https = require('https');  // Use https for downloads

const args = process.argv.slice(2);
const action = args[0];

const libDir = path.join(process.env.HOME, '.hackeros', 'hacker-lang', 'libs');

if (!fs.existsSync(libDir)) {
    fs.mkdirSync(libDir, { recursive: true });
}

const availableLibs = ['util', 'net', 'crypto'];  // Hardcoded for now

if (action === 'list') {
    console.log('Available libraries:');
    availableLibs.forEach(lib => console.log(`- ${lib}`));
} else if (action === 'install') {
    const libname = args[1];
    if (!libname) {
        console.error('Usage: hacker-library install <libname>');
        process.exit(1);
    }
    if (!availableLibs.includes(libname)) {
        console.error(`Library ${libname} not found.`);
        process.exit(1);
    }
    // Simulate download: fetch a placeholder file
    const url = `https://example.com/${libname}.hacker`;  // Placeholder URL, replace with real in future
    const filePath = path.join(libDir, `${libname}.hacker`);
    const file = fs.createWriteStream(filePath);
    https.get(url, response => {
        response.pipe(file);
        file.on('finish', () => {
            file.close();
            console.log(`Installed ${libname} to ${filePath}`);
        });
    }).on('error', err => {
        fs.unlink(filePath);
        console.error(`Error installing ${libname}: ${err.message}`);
    });
} else {
    console.error('Unknown action. Use list or install.');
    process.exit(1);
}
