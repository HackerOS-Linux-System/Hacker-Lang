const fs = require('fs');
const path = require('path');
const fetch = require('node-fetch');
const { execSync } = require('child_process');

const args = process.argv.slice(2);
const action = args[0];
const libDir = path.join(process.env.HOME, '.hacker-lang', 'libs');
const packageListUrl = 'https://github.com/Bytes-Repository/bytes.io/blob/main/repository/bytes.io';

if (!fs.existsSync(libDir)) {
    fs.mkdirSync(libDir, { recursive: true });
}

async function fetchPackageList() {
    try {
        const response = await fetch(packageListUrl);
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const text = await response.text();
        const lines = text.split('\n');
        const packages = {};
        let in_config = false;
        for (const line of lines) {
            const trimmed = line.trim();
            if (trimmed === '[') {
                in_config = true;
                continue;
            }
            if (trimmed === ']') {
                in_config = false;
                continue;
            }
            if (in_config && trimmed.includes('=>')) {
                const [name, url] = trimmed.split('=>').map(s => s.trim());
                if (name && url) {
                    packages[name] = url;
                }
            }
        }
        return packages;
    } catch (err) {
        console.error(`Error fetching package list: ${err.message}`);
        process.exit(1);
    }
}

if (action === 'list') {
    console.log('Fetching available libraries...');
    fetchPackageList().then(packages => {
        console.log('Available libraries:');
        Object.keys(packages).forEach(lib => console.log(`- ${lib}`));
        const installed = fs.existsSync(libDir) ? fs.readdirSync(libDir).filter(f => fs.lstatSync(path.join(libDir, f)).isDirectory()) : [];
        console.log('\nInstalled libraries:');
        installed.forEach(lib => console.log(`- ${lib}`));
    });
} else if (action === 'install') {
    const libname = args[1];
    if (!libname) {
        console.error('Usage: hacker-library install <libname>');
        process.exit(1);
    }
    fetchPackageList().then(packages => {
        if (!packages[libname]) {
            console.error(`Library ${libname} not found in package list.`);
            process.exit(1);
        }
        const repoUrl = packages[libname];
        const libPath = path.join(libDir, libname);
        console.log(`Installing ${libname} from ${repoUrl}...`);
        try {
            if (fs.existsSync(libPath)) {
                console.log(`Removing existing ${libname}...`);
                execSync(`rm -rf ${libPath}`);
            }
            execSync(`git clone ${repoUrl} ${libPath}`, { stdio: 'inherit' });
            if (!fs.existsSync(path.join(libPath, 'main.hacker'))) {
                console.error(`Library ${libname} missing main.hacker`);
                execSync(`rm -rf ${libPath}`);
                process.exit(1);
            }
            console.log(`Installed ${libname} to ${libPath}`);
        } catch (err) {
            console.error(`Error installing ${libname}: ${err.message}`);
            process.exit(1);
        }
    });
} else if (action === 'update') {
    console.log('Checking for library updates...');
    const installed = fs.existsSync(libDir) ? fs.readdirSync(libDir).filter(f => fs.lstatSync(path.join(libDir, f)).isDirectory()) : [];
    fetchPackageList().then(packages => {
        installed.forEach(lib => {
            if (packages[lib]) {
                const libPath = path.join(libDir, lib);
                console.log(`Updating ${lib}...`);
                try {
                    execSync(`cd ${libPath} && git pull`, { stdio: 'inherit' });
                    console.log(`${lib} updated`);
                } catch (err) {
                    console.error(`Error updating ${lib}: ${err.message}`);
                }
            } else {
                console.log(`${lib}: No update info available`);
            }
        });
    });
} else {
    console.error('Usage: hacker-library [list|install|update] [libname]');
    process.exit(1);
}
