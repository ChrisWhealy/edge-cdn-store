#!/usr/bin/env node
// har2curl.mjs
// Usage: node har2curl.mjs input.har > replay.sh
// If no file is provided (or "-" is used), reads HAR from stdin.

import fs from 'fs/promises'
import { spawn } from 'child_process'

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
const usage = "Usage: ./har2curl.mjs <har_file>"

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Produce curl line from a URL string; returns null if not parseable
const curlCmd = urlStr =>
  new Promise((resolve, reject) => {
    const u = new URL(urlStr)

    // Ignore fragments
    let path = u.pathname + u.search;
    if (!path.startsWith('/')) path = '/' + path;

    const argList = [
      '-s',
      '-o', process.platform === 'win32' ? 'NUL' : '/dev/null',
      '-D', '-',
      '-k', `https://localhost:6143${path}`,
      `-H`, `Host: ${u.host}`
    ]
    const curl = spawn('curl', argList)

    let data = ''
    let error = ''

    curl.stdout.on('data', chunk => data += chunk)
    curl.stderr.on('data', chunk => error += chunk);
    curl.on('error', err => reject(err))
    curl.on('close', code => code !== 0
      ? reject(new Error(`curl exited with code ${code}: ${error}`))
      : resolve(data)
    )
  })

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
const main = async () => {
  const arg = process.argv[2]

  let input
  let har

  // We must be passed a file, we're not going to read directly from stdin
  if (arg && arg !== '-') {
    input = await fs.readFile(arg, 'utf8')
  } else {
    console.error(usage)
    process.exit(1)
  }

  try {
    har = JSON.parse(input)
  } catch (e) {
    console.error('Error: The supplied file does not contain valid JSON')
    process.exit(1)
  }

  const entries = har?.log?.entries

  if (!Array.isArray(entries)) {
    console.error('Error: Cannot find the log.entries[] array')
    process.exit(1)
  }

  for (const entry of entries) {
    const urlStr = entry?.request?.url

    if (typeof urlStr !== 'string' || urlStr.trim() === '') continue

    console.log(await curlCmd(urlStr))
  }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
main().catch(err => {
  console.error('Unexpected error:', err?.message || err)
  process.exit(1)
})
