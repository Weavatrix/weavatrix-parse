import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
    cpSync,
    existsSync,
    readFileSync,
    writeFileSync,
} from 'node:fs'
import { join, resolve } from 'node:path'

export function prepareBench(name, parserRoot, scratch, projectRoot) {
    const destination = join(scratch, `bench-${name}`)
    cpSync(join(projectRoot, 'tools', 'regression-bench'), destination, {recursive: true})
    const manifestFile = join(destination, 'Cargo.toml')
    const parserPath = resolve(parserRoot).replace(/\\/g, '/')
    const cargo = readFileSync(manifestFile, 'utf8')
        .replace('path = "../.."', `path = ${JSON.stringify(parserPath)}`)
    writeFileSync(manifestFile, cargo)
    return destination
}

export function buildBench(benchRoot, targetDir, projectRoot) {
    const manifest = join(benchRoot, 'Cargo.toml')
    execFileSync('cargo', ['build', '--release', '--manifest-path', manifest], {
        cwd: projectRoot,
        env: {...process.env, CARGO_TARGET_DIR: targetDir},
        stdio: 'inherit',
        timeout: 20 * 60_000,
        windowsHide: true,
    })
    return join(
        targetDir,
        'release',
        process.platform === 'win32'
            ? 'weavatrix-parse-regression-bench.exe'
            : 'weavatrix-parse-regression-bench',
    )
}

export function execBench(binary, roots, cwd, summaryOnly = false) {
    const args = summaryOnly ? ['--summary-only', ...roots] : roots
    return execFileSync(binary, args, {
        cwd,
        encoding: 'utf8',
        maxBuffer: 512 * 1024 * 1024,
        timeout: 20 * 60_000,
        windowsHide: true,
    })
}

export function git(args, cwd) {
    return execFileSync('git', args, {
        cwd,
        encoding: 'utf8',
        timeout: 2 * 60_000,
        windowsHide: true,
        stdio: ['ignore', 'pipe', 'pipe'],
    })
}

export function fingerprintChanges(root) {
    const status = git([
        'status',
        '--porcelain=v1',
        '--untracked-files=all',
        '--',
        'Cargo.toml',
        'src',
    ], root).split(/\r?\n/).filter(Boolean)
    const hash = createHash('sha256')
    const files = status.map((line) => {
        const rawPath = line.slice(3)
        const path = rawPath.includes(' -> ') ? rawPath.split(' -> ').at(-1) : rawPath
        const absolute = resolve(root, path)
        hash.update(`${line}\0`)
        if (existsSync(absolute)) hash.update(readFileSync(absolute))
        else hash.update('<deleted>')
        hash.update('\0')
        return {status: line.slice(0, 2), path}
    })
    return {
        files,
        sha256: files.length ? hash.digest('hex') : null,
    }
}
