import { createHash } from 'node:crypto'
import {
    mkdirSync,
    readdirSync,
    readFileSync,
    writeFileSync,
} from 'node:fs'
import { dirname, join } from 'node:path'

export function snapshotCorpus(sourceRoots, destination, capBytesPerLanguage) {
    const cap = Number(capBytesPerLanguage)
    const totals = new Map()
    const hash = createHash('sha256')
    let files = 0
    let bytes = 0
    const roots = sourceRoots.map((_root, index) => {
        const path = join(destination, String(index))
        mkdirSync(path, {recursive: true})
        return path
    })
    for (const [rootIndex, sourceRoot] of sourceRoots.entries()) {
        visitSnapshot(sourceRoot, sourceRoot, roots[rootIndex], rootIndex)
    }
    return {roots, files, bytes, sha256: hash.digest('hex')}

    function visitSnapshot(sourceRoot, path, snapshotRoot, rootIndex) {
        let entries
        try {
            entries = readdirSync(path, {withFileTypes: true})
        } catch {
            return
        }
        entries.sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0)
        for (const entry of entries) {
            if (entry.isSymbolicLink() || skipDirectory(entry.name)) continue
            const absolute = join(path, entry.name)
            if (entry.isDirectory()) {
                visitSnapshot(sourceRoot, absolute, snapshotRoot, rootIndex)
                continue
            }
            const language = languageFor(entry.name)
            if (!language || (totals.get(language) || 0) >= cap) continue
            let content
            try {
                content = readFileSync(absolute)
                new TextDecoder('utf-8', {fatal: true}).decode(content)
            } catch {
                continue
            }
            const relative = absolute.slice(sourceRoot.length).replace(/^[\\/]+/, '')
            const target = join(snapshotRoot, relative)
            mkdirSync(dirname(target), {recursive: true})
            writeFileSync(target, content)
            totals.set(language, (totals.get(language) || 0) + content.length)
            hash.update(`${rootIndex}/${relative.replace(/\\/g, '/')}\0`)
            hash.update(content)
            hash.update('\0')
            files += 1
            bytes += content.length
        }
    }
}

function languageFor(name) {
    const extension = name.includes('.') ? name.split('.').at(-1).toLowerCase() : ''
    return {
        js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript',
        ts: 'typescript', tsx: 'typescript', mts: 'typescript', cts: 'typescript',
        py: 'python', pyi: 'python', rs: 'rust', go: 'go', java: 'java',
        cs: 'csharp', c: 'c', h: 'c', cc: 'cpp', cpp: 'cpp', cxx: 'cpp',
        hpp: 'cpp', hxx: 'cpp', sql: 'sql', swift: 'swift', tf: 'terraform',
        hcl: 'terraform', xml: 'xml', md: 'markdown', markdown: 'markdown',
        sh: 'bash', bash: 'bash', zsh: 'bash',
    }[extension] || null
}

function skipDirectory(name) {
    return name.startsWith('.')
        || new Set([
            'node_modules',
            'target',
            'dist',
            'build',
            'coverage',
            'out',
            '__pycache__',
            'venv',
        ]).has(name)
}
