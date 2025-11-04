{
  "name": "hacker-library",
  "version": "0.0.1",
  "description": "Hacker Lang - A simple scripting language for Debian-based Linux systems",
  "main": "hacker-library.js",
  "bin": {
    "hacker-library": "./hacker-library.js"
  },
  "scripts": {
    "start": "node hacker-library.js",
    "test": "echo \"No tests implemented yet\" && exit 0",
    "build": "pkg . --targets node18-linux-x64 --output dist/hacker-library"
  },
  "keywords": [
    "hacker-lang",
    "scripting",
    "linux",
    "debian"
  ],
  "author": "HackerOS Team",
  "license": "MIT",
  "dependencies": {
    "node-fetch": "^2.6.7"
  },
  "devDependencies": {
    "pkg": "^5.8.0"
  }
}
